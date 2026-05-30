use anyhow::Result;
use rustpotter::{Rustpotter, RustpotterConfig, WakewordLoad};
use std::path::Path;
use tokio::sync::mpsc;

pub struct WakeWordDetector {
    potter: Rustpotter,
}

impl WakeWordDetector {
    pub fn new(model_path: &Path) -> Result<Self> {
        let config = RustpotterConfig::default();
        let mut potter = Rustpotter::new(&config)?;
        potter.add_wakeword_from_file("wake", model_path.to_str().unwrap())?;
        Ok(Self { potter })
    }

    /// Returns true when wake word is detected in this chunk.
    pub fn process_chunk(&mut self, samples: &[i16]) -> bool {
        if let Some(detection) = self.potter.process_i16(samples) {
            tracing::debug!("wake word detected: score={}", detection.score);
            true
        } else {
            false
        }
    }
}
