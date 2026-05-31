use anyhow::Result;
use std::path::Path;

use crate::config::SttConfig;

pub enum SttEngine {
    Local(whisper_rs::WhisperContext),
    Remote { client: reqwest::Client, url: String },
}

impl SttEngine {
    pub fn new(config: &SttConfig) -> Result<Self> {
        if let Some(url) = &config.server_url {
            Ok(SttEngine::Remote {
                client: reqwest::Client::new(),
                url: url.clone(),
            })
        } else {
            use whisper_rs::{WhisperContext, WhisperContextParameters};
            let model_path = Path::new(&config.model_path).join(format!("{}.bin", config.model));
            let ctx = WhisperContext::new_with_params(
                model_path.to_str().unwrap(),
                WhisperContextParameters::default(),
            )?;
            Ok(SttEngine::Local(ctx))
        }
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        match self {
            SttEngine::Local(ctx) => transcribe_local(ctx, samples),
            SttEngine::Remote { client, url } => {
                // We need to block here since transcribe is a sync method.
                // Use tokio runtime handle if available, else create a new one.
                let rt = tokio::runtime::Handle::try_current();
                let client = client.clone();
                let url = url.clone();
                let wav_bytes = encode_wav_mono_16k(samples);
                if let Ok(handle) = rt {
                    tokio::task::block_in_place(|| {
                        handle.block_on(transcribe_remote(&client, &url, wav_bytes))
                    })
                } else {
                    let rt = tokio::runtime::Runtime::new()?;
                    rt.block_on(transcribe_remote(&client, &url, wav_bytes))
                }
            }
        }
    }
}

fn transcribe_local(ctx: &whisper_rs::WhisperContext, samples: &[f32]) -> Result<String> {
    use whisper_rs::{FullParams, SamplingStrategy};

    let mut state = ctx.create_state()?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state.full(params, samples)?;

    let num_segments = state.full_n_segments()?;
    let mut text = String::new();
    for i in 0..num_segments {
        text.push_str(state.full_get_segment_text(i)?.trim());
        text.push(' ');
    }
    Ok(text.trim().to_string())
}

async fn transcribe_remote(client: &reqwest::Client, url: &str, wav_bytes: Vec<u8>) -> Result<String> {
    let endpoint = format!("{}/inference", url.trim_end_matches('/'));
    let part = reqwest::multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = client.post(&endpoint).multipart(form).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("remote STT error: {}", resp.status());
    }

    let json: serde_json::Value = resp.json().await?;
    let text = json["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("remote STT response missing 'text' field"))?
        .trim()
        .to_string();
    Ok(text)
}

/// Encode f32 samples (assumed 16kHz mono) as a minimal WAV file in memory.
/// Format: 16kHz, mono, 16-bit PCM little-endian.
fn encode_wav_mono_16k(samples: &[f32]) -> Vec<u8> {
    let sample_rate: u32 = 16000;
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_len = samples.len() * 2; // 2 bytes per sample (i16)
    let file_size = 36 + data_len; // 44-byte header minus 8 for RIFF chunk preamble

    let mut buf: Vec<u8> = Vec::with_capacity(44 + data_len);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(file_size as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    buf.extend_from_slice(&1u16.to_le_bytes());  // PCM format
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_len as u32).to_le_bytes());

    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }

    buf
}
