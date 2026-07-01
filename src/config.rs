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
    #[serde(default = "default_true")]
    pub input: bool,
    #[serde(default = "default_true")]
    pub output: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SttConfig {
    /// Local whisper model name (used when server is absent or as fallback)
    #[serde(default = "default_stt_model")]
    pub model: String,
    /// Directory containing `<model>.bin` for local whisper
    #[serde(default = "default_model_path")]
    pub model_path: String,
    /// Speaches (or compatible) server URL, e.g. "http://192.168.1.11:8000"
    pub server_url: Option<String>,
    /// ASR model name to request from the server
    #[serde(default = "default_stt_server_model")]
    pub server_model: String,
    /// Fall back to local whisper if the server is unreachable
    #[serde(default = "default_true")]
    pub local_fallback: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TtsConfig {
    /// "speaches", "piper", or "elevenlabs"
    #[serde(default = "default_tts_provider")]
    pub provider: String,
    /// Speaches server URL (used when provider = "speaches")
    pub server_url: Option<String>,
    /// TTS model on the speaches server
    #[serde(default = "default_tts_server_model")]
    pub server_model: String,
    /// Voice ID on the speaches server (Kokoro voice name)
    #[serde(default = "default_tts_server_voice")]
    pub server_voice: String,
    /// Fall back to piper if speaches server is unreachable
    #[serde(default = "default_true")]
    pub local_fallback: bool,
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

fn default_device() -> String { "default".to_string() }
fn default_true() -> bool { true }
fn default_stt_model() -> String { "base.en".to_string() }
fn default_model_path() -> String { "models/".to_string() }
fn default_stt_server_model() -> String { "Systran/faster-distil-whisper-small.en".to_string() }
fn default_tts_provider() -> String { "speaches".to_string() }
fn default_tts_server_model() -> String { "speaches-ai/Kokoro-82M-v1.0-ONNX".to_string() }
fn default_tts_server_voice() -> String { "am_michael".to_string() }

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
