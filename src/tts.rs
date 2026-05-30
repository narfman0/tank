use anyhow::Result;
use rodio::{Decoder, OutputStream, Sink};
use std::io::BufReader;
use std::process::Command;
use tempfile::NamedTempFile;

use crate::config::TtsConfig;

pub struct TtsEngine {
    config: TtsConfig,
}

impl TtsEngine {
    pub fn new(config: TtsConfig) -> Self {
        Self { config }
    }

    pub async fn speak(&self, text: &str) -> Result<()> {
        match self.config.provider.as_str() {
            "piper" => self.speak_piper(text),
            "elevenlabs" => self.speak_elevenlabs(text).await,
            other => anyhow::bail!("unknown TTS provider: {}", other),
        }
    }

    fn speak_piper(&self, text: &str) -> Result<()> {
        let binary = self
            .config
            .piper_binary
            .as_deref()
            .unwrap_or("/usr/bin/piper");
        let voice = self
            .config
            .piper_voice
            .as_deref()
            .unwrap_or("en_US-ryan-medium.onnx");

        let output = NamedTempFile::new()?.into_temp_path();
        let output_path = output.to_str().unwrap().to_string();

        let status = Command::new(binary)
            .args([
                "--model",
                voice,
                "--output_file",
                &output_path,
            ])
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

        play_wav_file(&output_path)?;
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
        let tmp = NamedTempFile::new()?;
        std::fs::write(tmp.path(), &bytes)?;
        play_wav_file(tmp.path().to_str().unwrap())?;
        Ok(())
    }
}

fn play_wav_file(path: &str) -> Result<()> {
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    let file = BufReader::new(std::fs::File::open(path)?);
    let source = Decoder::new(file)?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}
