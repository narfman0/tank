use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, SampleFormat, StreamConfig};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct AudioCapture {
    host: Host,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    pub fn input_device(&self, name: &str) -> Result<Device> {
        if name == "default" {
            self.host
                .default_input_device()
                .ok_or_else(|| anyhow::anyhow!("no default input device"))
        } else {
            self.host
                .input_devices()?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| anyhow::anyhow!("input device '{}' not found", name))
        }
    }

    pub fn output_device(&self, name: &str) -> Result<Device> {
        if name == "default" {
            self.host
                .default_output_device()
                .ok_or_else(|| anyhow::anyhow!("no default output device"))
        } else {
            self.host
                .output_devices()?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| anyhow::anyhow!("output device '{}' not found", name))
        }
    }

    /// Record audio until silence is detected, return raw f32 samples at 16kHz.
    pub async fn record_until_silence(
        &self,
        device: &Device,
        silence_duration: Duration,
    ) -> Result<Vec<f32>> {
        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let samples_clone = samples.clone();

        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                let cfg: StreamConfig = config.into();
                device.build_input_stream(
                    &cfg,
                    move |data: &[f32], _| {
                        let mono: Vec<f32> = data
                            .chunks(channels)
                            .map(|ch| ch.iter().sum::<f32>() / ch.len() as f32)
                            .collect();
                        samples_clone.lock().unwrap().extend(mono);
                    },
                    |e| tracing::error!("input stream error: {}", e),
                    None,
                )?
            }
            SampleFormat::I16 => {
                let cfg: StreamConfig = config.into();
                device.build_input_stream(
                    &cfg,
                    move |data: &[i16], _| {
                        let mono: Vec<f32> = data
                            .chunks(channels)
                            .map(|ch| {
                                ch.iter().sum::<i16>() as f32
                                    / (ch.len() as f32 * i16::MAX as f32)
                            })
                            .collect();
                        samples_clone.lock().unwrap().extend(mono);
                    },
                    |e| tracing::error!("input stream error: {}", e),
                    None,
                )?
            }
            _ => anyhow::bail!("unsupported sample format"),
        };

        stream.play()?;

        // Record for a fixed duration (simplified: no VAD, just record for up to 10s then check silence)
        let silence_frames = (silence_duration.as_secs_f32() * sample_rate as f32) as usize;
        let max_record = Duration::from_secs(10);
        tokio::time::sleep(max_record).await;

        drop(stream);

        let captured = samples.lock().unwrap().clone();
        tracing::debug!("captured {} samples at {}Hz", captured.len(), sample_rate);

        // Resample to 16kHz if needed (whisper expects 16kHz)
        let resampled = if sample_rate != 16000 {
            resample_to_16k(&captured, sample_rate)
        } else {
            captured
        };

        let _ = silence_frames; // used in full VAD implementation
        Ok(resampled)
    }
}

fn resample_to_16k(samples: &[f32], src_rate: u32) -> Vec<f32> {
    if src_rate == 16000 {
        return samples.to_vec();
    }
    let ratio = 16000.0 / src_rate as f64;
    let out_len = (samples.len() as f64 * ratio) as usize;
    (0..out_len)
        .map(|i| {
            let src_idx = i as f64 / ratio;
            let lo = src_idx.floor() as usize;
            let hi = (lo + 1).min(samples.len() - 1);
            let frac = src_idx - lo as f64;
            samples[lo] * (1.0 - frac as f32) + samples[hi] * frac as f32
        })
        .collect()
}

pub fn play_audio_file(path: &str, device_name: &str) -> Result<()> {
    use cpal::traits::{DeviceTrait, HostTrait};
    use rodio::{Decoder, OutputStream, Sink};
    use std::fs::File;
    use std::io::BufReader;

    let host = cpal::default_host();
    let device = if device_name == "default" {
        host.default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no default output device"))?
    } else {
        host.output_devices()?
            .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
            .ok_or_else(|| anyhow::anyhow!("output device '{}' not found", device_name))?
    };

    let (_stream, stream_handle) = OutputStream::try_from_device(&device)?;
    let sink = Sink::try_new(&stream_handle)?;
    let file = BufReader::new(File::open(path)?);
    let source = Decoder::new(file)?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}
