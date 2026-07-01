use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{Decoder, OutputStream, Sink};
use std::io::BufReader;
use std::process::Command;
use tempfile::NamedTempFile;

use crate::config::TtsConfig;

pub struct TtsEngine {
    config: TtsConfig,
    output_device: String,
}

impl TtsEngine {
    pub fn new(config: TtsConfig, output_device: String) -> Self {
        Self { config, output_device }
    }

    pub async fn speak(&self, text: &str) -> Result<()> {
        match self.config.provider.as_str() {
            "speaches" => {
                match self.speak_speaches(text).await {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        if self.config.local_fallback {
                            tracing::warn!("speaches TTS failed ({}), using piper fallback", e);
                            self.speak_piper(text)
                        } else {
                            Err(e)
                        }
                    }
                }
            }
            "piper" => self.speak_piper(text),
            "elevenlabs" => self.speak_elevenlabs(text).await,
            other => anyhow::bail!("unknown TTS provider: {}", other),
        }
    }

    async fn speak_speaches(&self, text: &str) -> Result<()> {
        let url = self
            .config
            .server_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("speaches TTS requires [tts] server_url"))?;

        let endpoint = format!("{}/v1/audio/speech", url.trim_end_matches('/'));
        let client = reqwest::Client::new();

        let resp = client
            .post(&endpoint)
            .json(&serde_json::json!({
                "model": self.config.server_model,
                "input": text,
                "voice": self.config.server_voice,
                "response_format": "mp3"
            }))
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("speaches TTS error: {}", resp.status());
        }

        let bytes = resp.bytes().await?;
        let tmp = NamedTempFile::with_suffix(".mp3")?;
        std::fs::write(tmp.path(), &bytes)?;
        play_audio_file(tmp.path().to_str().unwrap(), &self.output_device)?;
        Ok(())
    }

    fn speak_piper(&self, text: &str) -> Result<()> {
        let binary = self.config.piper_binary.as_deref().unwrap_or("/usr/bin/piper");
        let voice = self.config.piper_voice.as_deref().unwrap_or("en_US-ryan-medium.onnx");

        let output = NamedTempFile::with_suffix(".wav")?.into_temp_path();
        let output_path = output.to_str().unwrap().to_string();

        let status = Command::new(binary)
            .args(["--model", voice, "--output_file", &output_path])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })?;

        if !status.success() {
            anyhow::bail!("piper exited with status {}", status);
        }

        play_audio_file(&output_path, &self.output_device)?;
        Ok(())
    }

    async fn speak_elevenlabs(&self, text: &str) -> Result<()> {
        let config = self
            .config
            .elevenlabs
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("elevenlabs config missing"))?;

        let client = reqwest::Client::new();
        let url = format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}",
            config.voice_id
        );

        let resp = client
            .post(&url)
            .header("xi-api-key", &config.api_key)
            .json(&serde_json::json!({
                "text": text,
                "model_id": "eleven_monolingual_v1",
                "voice_settings": { "stability": 0.5, "similarity_boost": 0.5 }
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("elevenlabs error: {}", resp.status());
        }

        let bytes = resp.bytes().await?;
        let tmp = NamedTempFile::with_suffix(".mp3")?;
        std::fs::write(tmp.path(), &bytes)?;
        play_audio_file(tmp.path().to_str().unwrap(), &self.output_device)?;
        Ok(())
    }
}

fn resolve_output_device(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if name == "default" {
        host.default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no default output device"))
    } else {
        host.output_devices()?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .ok_or_else(|| anyhow::anyhow!("output device '{}' not found", name))
    }
}

fn play_audio_file(path: &str, device_name: &str) -> Result<()> {
    let device = resolve_output_device(device_name)?;
    let (_stream, stream_handle) = OutputStream::try_from_device(&device)?;
    let sink = Sink::try_new(&stream_handle)?;
    let file = BufReader::new(std::fs::File::open(path)?);
    let source = Decoder::new(file)?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}
