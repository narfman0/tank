mod audio;
mod config;
mod matrix;
mod stt;
mod tts;
mod wake_word;

use anyhow::{Context, Result};
use cpal::traits::DeviceTrait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::info;

use config::Config;

#[derive(Debug)]
struct Args {
    config_path: PathBuf,
}

impl Args {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut config_path = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" | "-c" => {
                    config_path = args.next().map(PathBuf::from);
                }
                _ => {}
            }
        }
        Self {
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

    let args = Args::parse();
    let config_dir = args
        .config_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    info!("loading config from {}", args.config_path.display());
    let config = Arc::new(Config::load(&args.config_path).context("failed to load config")?);

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

    // Channel for Matrix → main loop (TTS responses)
    let (response_tx, mut response_rx) = mpsc::channel::<String>(32);
    matrix
        .start_sync(response_tx)
        .await
        .context("failed to start matrix sync")?;

    // Audio setup
    let audio = audio::AudioCapture::new();
    let input_device = audio.input_device(&config.audio.input_device)?;
    info!(
        "using input device: {}",
        input_device.name().unwrap_or_default()
    );

    // Wake word detector
    let wake_model_path = PathBuf::from(&config.audio.wake_word_model);
    let mut wake_word = wake_word::WakeWordDetector::new(&wake_model_path)
        .context("failed to load wake word model")?;

    // STT engine
    let model_path = PathBuf::from(&config.stt.model_path).join(format!("{}.bin", config.stt.model));
    let stt = stt::SttEngine::new(&model_path).context("failed to load whisper model")?;

    // TTS engine
    let tts = tts::TtsEngine::new(config.tts.clone());

    info!("tank ready, listening for wake word...");

    // Main loop
    loop {
        // 1. Listen for wake word on a 30ms audio frame
        let wake_chunk = capture_short_chunk(&input_device)?;
        if !wake_word.process_chunk(&wake_chunk) {
            continue;
        }

        info!("wake word detected, recording...");

        // 2. Record utterance until silence
        let samples = audio
            .record_until_silence(&input_device, Duration::from_secs(2))
            .await?;

        if samples.is_empty() {
            continue;
        }

        // 3. STT
        let transcript = match stt.transcribe(&samples) {
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

        // 5. Wait for response (with timeout)
        let response = tokio::time::timeout(Duration::from_secs(30), response_rx.recv()).await;
        match response {
            Ok(Some(text)) => {
                info!("response: {}", text);
                // 6. TTS playback
                if let Err(e) = tts.speak(&text).await {
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
