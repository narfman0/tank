use tank::{audio, config, matrix, stt, tts, wake_word, wizard};

use anyhow::{Context, Result};
use cpal::traits::DeviceTrait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::info;

use config::Config;

#[derive(Debug)]
enum Command {
    Run { config_path: PathBuf },
    Wizard { out_path: PathBuf },
    GenerateConfig { out_path: PathBuf },
}

impl Command {
    fn parse() -> Self {
        let mut argv = std::env::args().skip(1).peekable();
        match argv.peek().map(|s| s.as_str()) {
            Some("wizard") => {
                argv.next();
                let out_path = argv.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("config.toml"));
                return Command::Wizard { out_path };
            }
            Some("generate-config") => {
                argv.next();
                let out_path = argv.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("config.toml"));
                return Command::GenerateConfig { out_path };
            }
            _ => {}
        }
        let mut config_path = None;
        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--config" | "-c" => {
                    config_path = argv.next().map(PathBuf::from);
                }
                _ => {}
            }
        }
        Command::Run {
            config_path: config_path.unwrap_or_else(|| PathBuf::from("config.toml")),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tank=info".parse()?),
        )
        .init();

    let config_path = match Command::parse() {
        Command::Wizard { out_path } => return wizard::run(out_path).await,
        Command::GenerateConfig { out_path } => {
            if out_path.exists() {
                eprintln!("'{}' already exists, not overwriting. Delete it first or specify a different path.", out_path.display());
                std::process::exit(1);
            }
            std::fs::write(&out_path, include_str!("../config.example.toml"))
                .with_context(|| format!("failed to write {}", out_path.display()))?;
            println!("Wrote default config to '{}'. Edit it, then run: tank --config {}", out_path.display(), out_path.display());
            return Ok(());
        }
        Command::Run { config_path } => config_path,
    };
    let config_dir = config_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    if !config_path.exists() {
        eprintln!("No config found at '{}'.", config_path.display());
        eprintln!("  Generate a default:  tank generate-config");
        eprintln!("  Interactive wizard:  tank wizard");
        std::process::exit(1);
    }

    info!("loading config from {}", config_path.display());
    let config = Arc::new(Config::load(&config_path).context("failed to load config")?);

    let do_input = config.audio.input;
    let do_output = config.audio.output;

    if !do_input && !do_output {
        tracing::warn!("both input and output are disabled — nothing to do, exiting");
        return Ok(());
    }

    // Matrix client setup
    let store_path = config_dir.join("matrix-store");
    std::fs::create_dir_all(&store_path)?;

    let matrix = Arc::new(
        matrix::MatrixVoiceClient::new(config.clone(), &store_path)
            .await
            .context("failed to create matrix client")?,
    );

    matrix
        .authenticate(&config_dir)
        .await
        .context("matrix authentication failed")?;

    // Channel for Matrix → main loop (TTS responses); only used when output is enabled
    let (response_tx, mut response_rx) = mpsc::channel::<String>(32);
    if do_output {
        matrix
            .start_sync(response_tx)
            .await
            .context("failed to start matrix sync")?;
    }

    // Input-side setup (wake word + STT)
    let audio;
    let input_device;
    let mut wake_word;
    let stt;

    if do_input {
        audio = Some(audio::AudioCapture::new());
        let dev = audio.as_ref().unwrap().input_device(&config.audio.input_device)?;
        info!("using input device: {}", dev.name().unwrap_or_default());

        let wake_model_path = PathBuf::from(&config.audio.wake_word_model);
        wake_word = Some(
            wake_word::WakeWordDetector::new(&wake_model_path)
                .context("failed to load wake word model")?,
        );

        stt = Some(
            stt::SttEngine::new(&config.stt).context("failed to initialize STT engine")?,
        );

        input_device = Some(dev);
    } else {
        audio = None;
        input_device = None;
        wake_word = None;
        stt = None;
    }

    // Output-side setup (TTS)
    let tts = if do_output {
        Some(tts::TtsEngine::new(
            config.tts.clone(),
            config.audio.output_device.clone(),
        ))
    } else {
        None
    };

    match (do_input, do_output) {
        (true, _) => info!("tank ready, listening for wake word..."),
        (false, true) => info!("tank ready in output-only mode, listening on Matrix..."),
        _ => unreachable!(),
    }

    // Main loop
    loop {
        if do_input {
            let dev = input_device.as_ref().unwrap();
            let ww = wake_word.as_mut().unwrap();
            let stt_engine = stt.as_ref().unwrap();
            let audio_cap = audio.as_ref().unwrap();

            // 1. Listen for wake word on a 30ms audio frame
            let wake_chunk = capture_short_chunk(dev)?;
            if !ww.process_chunk(&wake_chunk) {
                continue;
            }

            info!("wake word detected, recording...");

            // 2. Record utterance until silence
            let samples = audio_cap
                .record_until_silence(dev, Duration::from_secs(2))
                .await?;

            if samples.is_empty() {
                continue;
            }

            // 3. STT
            let transcript = match stt_engine.transcribe(&samples) {
                Ok(t) if !t.is_empty() => t,
                Ok(_) => {
                    info!("empty transcription, ignoring");
                    continue;
                }
                Err(e) => {
                    tracing::error!("STT error: {}", e);
                    continue;
                }
            };
            info!("transcribed: {}", transcript);

            // 4. Send to Matrix
            if let Err(e) = matrix.send_message(&transcript).await {
                tracing::error!("failed to send message: {}", e);
            }

            if !do_output {
                // Input-only: nothing more to do for this utterance
                continue;
            }

            // 5. Wait for response (with timeout)
            let response =
                tokio::time::timeout(Duration::from_secs(30), response_rx.recv()).await;
            match response {
                Ok(Some(text)) => {
                    info!("response: {}", text);
                    // 6. TTS playback
                    if let Err(e) = tts.as_ref().unwrap().speak(&text).await {
                        tracing::error!("TTS error: {}", e);
                    }
                }
                Ok(None) => {
                    tracing::warn!("response channel closed");
                    break;
                }
                Err(_) => {
                    tracing::warn!("timeout waiting for response");
                }
            }
        } else {
            // Output-only: just wait for Matrix messages and speak them
            match response_rx.recv().await {
                Some(text) => {
                    info!("received: {}", text);
                    if let Err(e) = tts.as_ref().unwrap().speak(&text).await {
                        tracing::error!("TTS error: {}", e);
                    }
                }
                None => {
                    tracing::warn!("response channel closed");
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Capture a short chunk (~30ms) of i16 samples for wake word detection.
fn capture_short_chunk(device: &cpal::Device) -> Result<Vec<i16>> {
    use cpal::traits::DeviceTrait;
    use cpal::SampleFormat;
    use std::sync::{Arc, Mutex};

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    // 30ms worth of samples
    let _chunk_size = (sample_rate as usize * 30) / 1000;

    let buf: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_clone = buf.clone();

    let stream = match config.sample_format() {
        SampleFormat::I16 => {
            let cfg: cpal::StreamConfig = config.into();
            device.build_input_stream(
                &cfg,
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels)
                        .map(|ch| ch[0])
                        .collect();
                    buf_clone.lock().unwrap().extend(mono);
                },
                |e| tracing::error!("stream error: {}", e),
                None,
            )?
        }
        SampleFormat::F32 => {
            let cfg: cpal::StreamConfig = config.into();
            device.build_input_stream(
                &cfg,
                move |data: &[f32], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels)
                        .map(|ch| (ch[0] * i16::MAX as f32) as i16)
                        .collect();
                    buf_clone.lock().unwrap().extend(mono);
                },
                |e| tracing::error!("stream error: {}", e),
                None,
            )?
        }
        _ => anyhow::bail!("unsupported sample format"),
    };

    use cpal::traits::StreamTrait;
    stream.play()?;
    std::thread::sleep(Duration::from_millis(30));
    drop(stream);

    let result = buf.lock().unwrap().clone();
    Ok(result)
}
