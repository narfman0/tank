use anyhow::Result;
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct SttEngine {
    ctx: WhisperContext,
}

impl SttEngine {
    pub fn new(model_path: &Path) -> Result<Self> {
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap(),
            WhisperContextParameters::default(),
        )?;
        Ok(Self { ctx })
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let mut state = self.ctx.create_state()?;
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
}
