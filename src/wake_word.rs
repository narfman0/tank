// Wake word detection using rustpotter v2.
// rustpotter v2 is loaded from git (tag v2.0.0) because v2 was yanked from crates.io.
// Note: rustpotter v3 remains blocked — candle-core 0.2.2 has a rand 0.8/0.9 transitive
// dep conflict that is unresolved as of this writing.

use anyhow::Result;
use rustpotter::{Rustpotter, RustpotterConfig};
use std::path::Path;

/// Wake word detector backed by rustpotter v2.
/// Pass the path to a `.rpw` model file built with rustpotter-cli or the rustpotter v2 API.
pub struct WakeWordDetector {
    detector: Rustpotter,
}

impl WakeWordDetector {
    pub fn new(model_path: &Path) -> Result<Self> {
        let mut detector = Rustpotter::new(&RustpotterConfig::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        detector
            .add_wakeword_from_file(model_path.to_str().unwrap())
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Self { detector })
    }

    /// Returns true when rustpotter detects the wake word in the supplied samples.
    pub fn process_chunk(&mut self, samples: &[i16]) -> bool {
        self.detector.process_i16(samples).is_some()
    }
}
