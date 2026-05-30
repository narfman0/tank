use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub matrix: MatrixConfig,
    pub audio: AudioConfig,
    pub stt: SttConfig,
    pub tts: TtsConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub username: String,
    pub password: String,
    pub session_file: String,
    #[serde(default)]
    pub rooms: Vec<RoomConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoomConfig {
    pub id: String,
    #[serde(default)]
    pub listen: bool,
    #[serde(default)]
    pub send: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AudioConfig {
    #[serde(default = "default_device")]
    pub input_device: String,
    #[serde(default = "default_device")]
    pub output_device: String,
    pub wake_word_model: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SttConfig {
    pub model: String,
    pub model_path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TtsConfig {
    #[serde(default = "default_tts_provider")]
    pub provider: String,
    pub piper_binary: Option<String>,
    pub piper_voice: Option<String>,
    pub elevenlabs: Option<ElevenLabsConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ElevenLabsConfig {
    pub api_key: String,
    pub voice_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub device_id: String,
    pub user_id: String,
    pub homeserver: String,
}

fn default_device() -> String {
    "default".to_string()
}

fn default_tts_provider() -> String {
    "piper".to_string()
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config from {}", path.display()))?;
        toml::from_str(&content).context("failed to parse config.toml")
    }

    pub fn session_path(&self, config_dir: &Path) -> PathBuf {
        config_dir.join(&self.matrix.session_file)
    }

    pub fn load_session(&self, config_dir: &Path) -> Option<Session> {
        let path = self.session_path(config_dir);
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save_session(&self, config_dir: &Path, session: &Session) -> Result<()> {
        let path = self.session_path(config_dir);
        let content = serde_json::to_string_pretty(session)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
