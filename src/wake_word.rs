// NOTE: rustpotter = "3" is blocked by a rand 0.8/0.9 transitive dep conflict in candle-core 0.2.2.
// Using a simple energy + keyword stub until rustpotter upgrades candle-core.
//
// To restore rustpotter-based detection:
//   1. Uncomment rustpotter in Cargo.toml when candle-core >= 0.7 is supported
//   2. Replace this module with the implementation below (commented out)
//
// ```rust (rustpotter-based implementation for when deps are fixed)
// use rustpotter::{Rustpotter, RustpotterConfig, WakewordLoad};
// pub struct WakeWordDetector { potter: Rustpotter }
// impl WakeWordDetector {
//     pub fn new(model_path: &Path) -> Result<Self> {
//         let mut potter = Rustpotter::new(&RustpotterConfig::default())?;
//         potter.add_wakeword_from_file("wake", model_path.to_str().unwrap())?;
//         Ok(Self { potter })
//     }
//     pub fn process_chunk(&mut self, samples: &[i16]) -> bool {
//         self.potter.process_i16(samples).is_some()
//     }
// }
// ```

use anyhow::Result;
use std::path::Path;

/// Energy-threshold wake word stub.
/// In production, replace with rustpotter once candle-core dep conflict is resolved.
pub struct WakeWordDetector {
    threshold: f32,
    window: Vec<f32>,
    window_capacity: usize,
}

impl WakeWordDetector {
    pub fn new(_model_path: &Path) -> Result<Self> {
        tracing::warn!(
            "using energy-threshold wake word stub — rustpotter blocked by candle-core rand conflict"
        );
        Ok(Self {
            threshold: 0.02,
            window: Vec::new(),
            window_capacity: 8000, // 0.5s at 16kHz
        })
    }

    /// Returns true when the RMS energy of the last half-second crosses the threshold.
    /// This is a placeholder: it will trigger on any loud sound, not on a specific phrase.
    pub fn process_chunk(&mut self, samples: &[i16]) -> bool {
        let normalized: Vec<f32> = samples
            .iter()
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();

        self.window.extend_from_slice(&normalized);
        if self.window.len() > self.window_capacity {
            let excess = self.window.len() - self.window_capacity;
            self.window.drain(..excess);
        }

        if self.window.len() < self.window_capacity {
            return false;
        }

        let rms = (self.window.iter().map(|s| s * s).sum::<f32>() / self.window.len() as f32)
            .sqrt();
        rms > self.threshold
    }
}
