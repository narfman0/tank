//! Interactive configuration wizard — `tank wizard [out.toml]`

use anyhow::{Context, Result};
use matrix_sdk::{config::SyncSettings, Client};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::audio::{play_audio_file, AudioCapture};

/// Collected answers, assembled into TOML at the end.
#[derive(Default)]
struct Answers {
    homeserver: String,
    username: String,
    password: String,
    room_id: String,
    room_label: String,
    input_device: String,
    output_device: String,
    wake_word_model: String,
    stt_server_url: String,
    stt_model: String,
    tts_server_url: String,
    tts_model: String,
    tts_voice: String,
}

/// Entry point called by `tank wizard [out.toml]`.
pub async fn run(out_path: PathBuf) -> Result<()> {

    println!("\n\x1b[1m🛠  tank configuration wizard\x1b[0m");
    println!("Each step runs a live test. Ctrl-C to abort; nothing is written until the end.\n");

    let mut a = Answers::default();

    step_matrix(&mut a).await?;
    step_audio(&mut a)?;
    step_wake_word(&mut a)?;
    step_stt(&mut a).await?;
    step_tts(&mut a).await?;
    step_write(&a, &out_path)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Step 1 — Matrix: login test + room picker
// ---------------------------------------------------------------------------

async fn step_matrix(a: &mut Answers) -> Result<()> {
    header(1, "Matrix", "login + room picker");

    loop {
        a.homeserver = prompt_default("Homeserver URL", "https://matrix.org");
        a.username = prompt("Matrix user ID (@you:server.com)");
        a.password = prompt_password("Password");

        print!("  → logging in… ");
        io::stdout().flush().ok();

        match matrix_login(&a.homeserver, &a.username, &a.password).await {
            Ok(client) => {
                println!("\x1b[32mok\x1b[0m");
                println!("  → syncing room list…");
                // One sync pass populates the joined-room list with names.
                client
                    .sync_once(SyncSettings::default())
                    .await
                    .context("initial sync failed")?;

                let rooms = client.joined_rooms();
                if rooms.is_empty() {
                    println!("  \x1b[33m! no joined rooms found. Join a room in your client, then retry.\x1b[0m");
                    if !yes_no("Retry login?", true) {
                        anyhow::bail!("no rooms to pick from");
                    }
                    continue;
                }

                // List rooms by human name so the user need not know the ID.
                let mut labels = Vec::new();
                for room in &rooms {
                    let name = match room.display_name().await {
                        Ok(n) => n.to_string(),
                        Err(_) => "(unnamed)".to_string(),
                    };
                    labels.push(name);
                }

                println!("\n  Joined rooms:");
                for (i, (room, name)) in rooms.iter().zip(&labels).enumerate() {
                    println!("    [{}] {}  \x1b[90m{}\x1b[0m", i + 1, name, room.room_id());
                }

                let idx = prompt_index("Pick a room", rooms.len());
                a.room_id = rooms[idx].room_id().to_string();
                a.room_label = labels[idx].clone();
                println!("  \x1b[32m✓\x1b[0m selected \"{}\"", a.room_label);
                return Ok(());
            }
            Err(e) => {
                println!("\x1b[31mfailed\x1b[0m");
                for (i, cause) in e.chain().enumerate() {
                    if i == 0 {
                        println!("  \x1b[31m✗\x1b[0m {}", cause);
                    } else {
                        println!("    caused by: {}", cause);
                    }
                }
                if !yes_no("Try again?", true) {
                    anyhow::bail!("matrix login aborted");
                }
            }
        }
    }
}

async fn matrix_login(homeserver: &str, username: &str, password: &str) -> Result<Client> {
    let client = Client::builder()
        .homeserver_url(homeserver)
        .build()
        .await
        .with_context(|| format!("could not reach homeserver '{}'", homeserver))?;
    client
        .matrix_auth()
        .login_username(username, password)
        .initial_device_display_name("tank-wizard")
        .send()
        .await
        .with_context(|| format!("login failed for '{}' on '{}'", username, homeserver))?;
    Ok(client)
}

// ---------------------------------------------------------------------------
// Step 2 — Audio: device pickers + live mic capture
// ---------------------------------------------------------------------------

fn step_audio(a: &mut Answers) -> Result<()> {
    header(2, "Audio", "device selection + mic test");

    let cap = AudioCapture::new();

    let inputs = cap.list_input_devices().unwrap_or_default();
    a.input_device = pick_device("input (microphone)", &inputs);

    let outputs = cap.list_output_devices().unwrap_or_default();
    a.output_device = pick_device("output (speaker)", &outputs);

    // Live mic test: capture 1s and report the peak amplitude.
    if yes_no("Run 1-second mic test now?", true) {
        let device = cap
            .input_device(&a.input_device)
            .context("could not open selected input device")?;
        println!("  → speak now… capturing 1s");
        let peak = cap.capture_peak(&device, Duration::from_secs(1))?;
        let bar = meter(peak);
        println!("  peak amplitude: {:.3}  {}", peak, bar);
        if peak < 0.01 {
            println!("  \x1b[33m! very low signal — mic may be muted or the wrong device.\x1b[0m");
        } else {
            println!("  \x1b[32m✓\x1b[0m mic is picking up sound");
        }
    }
    Ok(())
}

fn pick_device(kind: &str, names: &[String]) -> String {
    if names.is_empty() {
        println!("  no {} devices enumerated; using \"default\"", kind);
        return "default".to_string();
    }
    println!("\n  Available {} devices:", kind);
    println!("    [0] default");
    for (i, n) in names.iter().enumerate() {
        println!("    [{}] {}", i + 1, n);
    }
    let idx = prompt_index_zero(&format!("Pick {} device", kind), names.len() + 1);
    if idx == 0 {
        "default".to_string()
    } else {
        names[idx - 1].clone()
    }
}

// ---------------------------------------------------------------------------
// Step 3 — Wake word: file existence check
// ---------------------------------------------------------------------------

fn step_wake_word(a: &mut Answers) -> Result<()> {
    header(3, "Wake word", "model file check");

    loop {
        let path = prompt_default("Path to .rpw model", "models/roger.rpw");
        if Path::new(&path).is_file() {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!("  \x1b[32m✓\x1b[0m found ({} bytes)", size);
            a.wake_word_model = path;
            return Ok(());
        }
        println!("  \x1b[31m✗\x1b[0m not found at \"{}\"", path);
        if !yes_no("Enter a different path?", true) {
            // Allow proceeding anyway (user may create it later).
            a.wake_word_model = path;
            println!("  \x1b[33m! continuing without a wake-word model present.\x1b[0m");
            return Ok(());
        }
    }
}

// ---------------------------------------------------------------------------
// Step 4 — STT: speaches /health + ASR model picker
// ---------------------------------------------------------------------------

async fn step_stt(a: &mut Answers) -> Result<()> {
    header(4, "STT", "speaches health + model pick");

    let client = reqwest::Client::new();
    loop {
        a.stt_server_url = prompt_default("Speaches server URL", "http://192.168.1.11:8000");
        let base = a.stt_server_url.trim_end_matches('/').to_string();

        print!("  → GET {}/health … ", base);
        io::stdout().flush().ok();
        match http_ok(&client, &format!("{}/health", base)).await {
            Ok(()) => println!("\x1b[32mhealthy\x1b[0m"),
            Err(e) => {
                println!("\x1b[31munreachable\x1b[0m");
                for (i, cause) in e.chain().enumerate() {
                    if i == 0 {
                        println!("  \x1b[31m✗\x1b[0m {}", cause);
                    } else {
                        println!("    caused by: {}", cause);
                    }
                }
                if yes_no("Retry with a different URL?", true) {
                    continue;
                }
                a.stt_model = prompt_default(
                    "ASR model name (server unverified)",
                    "Systran/faster-distil-whisper-small.en",
                );
                return Ok(());
            }
        }

        match list_models(&client, &base, Some("asr")).await {
            Ok(models) if !models.is_empty() => {
                println!("\n  Available ASR models:");
                for (i, m) in models.iter().enumerate() {
                    println!("    [{}] {}", i + 1, m);
                }
                let idx = prompt_index("Pick an ASR model", models.len());
                a.stt_model = models[idx].clone();
            }
            _ => {
                println!("  (could not list ASR models; enter manually)");
                a.stt_model = prompt_default(
                    "ASR model name",
                    "Systran/faster-distil-whisper-small.en",
                );
            }
        }
        println!("  \x1b[32m✓\x1b[0m STT → {}", a.stt_model);
        return Ok(());
    }
}

// ---------------------------------------------------------------------------
// Step 5 — TTS: Kokoro voice list + live playback
// ---------------------------------------------------------------------------

async fn step_tts(a: &mut Answers) -> Result<()> {
    header(5, "TTS", "voice list + playback test");

    let client = reqwest::Client::new();

    // Default to the same server used for STT for convenience.
    let default_url = if a.stt_server_url.is_empty() {
        "http://192.168.1.11:8000".to_string()
    } else {
        a.stt_server_url.clone()
    };
    a.tts_server_url = prompt_default("Speaches server URL (TTS)", &default_url);
    let base = a.tts_server_url.trim_end_matches('/').to_string();

    a.tts_model = prompt_default("TTS model", "speaches-ai/Kokoro-82M-v1.0-ONNX");

    // Try to list Kokoro voices; fall back to a known set if the endpoint is absent.
    let voices = match list_voices(&client, &base, &a.tts_model).await {
        Ok(v) if !v.is_empty() => v,
        _ => {
            println!("  (voice list endpoint unavailable; using built-in Kokoro list)");
            default_kokoro_voices()
        }
    };

    println!("\n  Available voices:");
    for (i, v) in voices.iter().enumerate() {
        println!("    [{}] {}", i + 1, v);
    }
    let idx = prompt_index("Pick a voice", voices.len());
    a.tts_voice = voices[idx].clone();

    // Live playback test through the chosen output device.
    if yes_no(&format!("Play \"Tank is ready.\" in voice '{}'?", a.tts_voice), true) {
        print!("  → synthesizing… ");
        io::stdout().flush().ok();
        match synth_and_play(&client, &base, &a.tts_model, &a.tts_voice, &a.output_device).await {
            Ok(()) => println!("\x1b[32mplayed\x1b[0m"),
            Err(e) => {
                println!("\x1b[31mfailed\x1b[0m");
                for (i, cause) in e.chain().enumerate() {
                    if i == 0 { println!("  \x1b[31m✗\x1b[0m {}", cause); }
                    else { println!("    caused by: {}", cause); }
                }
            }
        }
    }
    println!("  \x1b[32m✓\x1b[0m TTS → {} / {}", a.tts_model, a.tts_voice);
    Ok(())
}

async fn synth_and_play(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    voice: &str,
    output_device: &str,
) -> Result<()> {
    let resp = client
        .post(format!("{}/v1/audio/speech", base))
        .json(&serde_json::json!({
            "model": model,
            "input": "Tank is ready.",
            "voice": voice,
            "response_format": "mp3"
        }))
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("TTS HTTP {}", resp.status());
    }
    let bytes = resp.bytes().await?;
    let tmp = tempfile::NamedTempFile::with_suffix(".mp3")?;
    std::fs::write(tmp.path(), &bytes)?;
    play_audio_file(tmp.path().to_str().unwrap(), output_device)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Step 6 — Write config
// ---------------------------------------------------------------------------

fn step_write(a: &Answers, out_path: &Path) -> Result<()> {
    header(6, "Write config", "preview + confirm");

    let toml = render_toml(a);
    println!("\n\x1b[90m{}\x1b[0m", "─".repeat(60));
    print!("{}", toml);
    println!("\x1b[90m{}\x1b[0m", "─".repeat(60));

    if out_path.exists() && !yes_no(&format!("{} exists. Overwrite?", out_path.display()), false) {
        println!("  aborted; nothing written.");
        return Ok(());
    }
    if !yes_no(&format!("Write to {}?", out_path.display()), true) {
        println!("  aborted; nothing written.");
        return Ok(());
    }

    std::fs::write(out_path, toml)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    println!("  \x1b[32m✓\x1b[0m wrote {}", out_path.display());
    println!("\n  Next: cargo run --bin tank -- --config {}\n", out_path.display());
    Ok(())
}

fn render_toml(a: &Answers) -> String {
    // Hand-rendered so the output is commented and ordered like the example.
    format!(
        r#"[matrix]
homeserver = "{homeserver}"
username = "{username}"
password = "{password}"   # only used for initial login; session is cached after
session_file = "session.json"

[[matrix.rooms]]
id = "{room_id}"   # {room_label}
listen = true
send = true

[audio]
input_device = "{input_device}"
output_device = "{output_device}"
wake_word_model = "{wake_word_model}"
input = true
output = true

[stt]
server_url = "{stt_url}"
server_model = "{stt_model}"
local_fallback = true
model = "base.en"
model_path = "models/"

[tts]
provider = "speaches"
server_url = "{tts_url}"
server_model = "{tts_model}"
server_voice = "{tts_voice}"
local_fallback = true
piper_binary = "/usr/bin/piper"
piper_voice = "en_US-ryan-medium.onnx"
"#,
        homeserver = a.homeserver,
        username = a.username,
        password = a.password,
        room_id = a.room_id,
        room_label = a.room_label,
        input_device = a.input_device,
        output_device = a.output_device,
        wake_word_model = a.wake_word_model,
        stt_url = a.stt_server_url,
        stt_model = a.stt_model,
        tts_url = a.tts_server_url,
        tts_model = a.tts_model,
        tts_voice = a.tts_voice,
    )
}

// ---------------------------------------------------------------------------
// speaches HTTP helpers
// ---------------------------------------------------------------------------

async fn http_ok(client: &reqwest::Client, url: &str) -> Result<()> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("HTTP {}", resp.status())
    }
}

/// List model ids from `/v1/models`, optionally filtering by task substring
/// (speaches tags models with a `task` field, e.g. "automatic-speech-recognition").
async fn list_models(
    client: &reqwest::Client,
    base: &str,
    filter: Option<&str>,
) -> Result<Vec<String>> {
    let resp = client
        .get(format!("{}/v1/models", base))
        .timeout(Duration::from_secs(10))
        .send()
        .await?;
    let json: serde_json::Value = resp.json().await?;
    let data = json["data"].as_array().cloned().unwrap_or_default();
    let want_asr = matches!(filter, Some("asr"));
    let mut out = Vec::new();
    for m in data {
        let id = m["id"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        if want_asr {
            // Keep entries whose task/id looks like speech recognition.
            let task = m["task"].as_str().unwrap_or("");
            let looks_asr = task.contains("speech-recognition")
                || task.contains("transcription")
                || id.to_lowercase().contains("whisper");
            let looks_tts = task.contains("text-to-speech")
                || id.to_lowercase().contains("kokoro");
            if looks_tts && !looks_asr {
                continue;
            }
        }
        out.push(id);
    }
    Ok(out)
}

/// List TTS voices for a model. speaches exposes them under
/// `/v1/audio/speech/voices`; if that 404s the caller falls back to a static list.
async fn list_voices(
    client: &reqwest::Client,
    base: &str,
    model: &str,
) -> Result<Vec<String>> {
    let resp = client
        .get(format!("{}/v1/audio/speech/voices", base))
        .query(&[("model_id", model)])
        .timeout(Duration::from_secs(10))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    let json: serde_json::Value = resp.json().await?;
    // Accept either a bare array of strings or array of objects with a name/voice field.
    let mut out = Vec::new();
    if let Some(arr) = json.as_array() {
        for v in arr {
            if let Some(s) = v.as_str() {
                out.push(s.to_string());
            } else if let Some(s) = v["voice"].as_str().or_else(|| v["id"].as_str()) {
                out.push(s.to_string());
            }
        }
    }
    Ok(out)
}

fn default_kokoro_voices() -> Vec<String> {
    // Common Kokoro-82M English voices shipped by speaches.
    ["am_michael", "am_adam", "af_bella", "af_sarah", "af_nicole", "bf_emma", "bm_george"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// tiny interactive prompt helpers
// ---------------------------------------------------------------------------

fn header(n: u8, title: &str, sub: &str) {
    println!("\n\x1b[1;36m[{}/6] {}\x1b[0m — {}", n, title, sub);
}

fn read_line() -> String {
    let mut s = String::new();
    // read_line returns Ok(0) at EOF; treat a closed stdin as an abort rather
    // than looping forever on the empty string it would otherwise yield.
    match io::stdin().read_line(&mut s) {
        Ok(0) => {
            println!("\n  (stdin closed) aborting.");
            std::process::exit(1);
        }
        _ => s.trim().to_string(),
    }
}

fn prompt(label: &str) -> String {
    loop {
        print!("  {}: ", label);
        io::stdout().flush().ok();
        let v = read_line();
        if !v.is_empty() {
            return v;
        }
    }
}

fn prompt_default(label: &str, default: &str) -> String {
    print!("  {} [{}]: ", label, default);
    io::stdout().flush().ok();
    let v = read_line();
    if v.is_empty() {
        default.to_string()
    } else {
        v
    }
}

fn prompt_password(label: &str) -> String {
    // No termios dependency; note the echo so the user can choose their setting.
    print!("  {} (visible): ", label);
    io::stdout().flush().ok();
    read_line()
}

/// 1-based selection returning a 0-based index.
fn prompt_index(label: &str, len: usize) -> usize {
    loop {
        print!("  {} [1-{}]: ", label, len);
        io::stdout().flush().ok();
        if let Ok(n) = read_line().parse::<usize>() {
            if (1..=len).contains(&n) {
                return n - 1;
            }
        }
        println!("  enter a number between 1 and {}", len);
    }
}

/// 0-based selection over `len` entries (used where [0] is a valid choice).
fn prompt_index_zero(label: &str, len: usize) -> usize {
    loop {
        print!("  {} [0-{}]: ", label, len - 1);
        io::stdout().flush().ok();
        if let Ok(n) = read_line().parse::<usize>() {
            if n < len {
                return n;
            }
        }
        println!("  enter a number between 0 and {}", len - 1);
    }
}

fn yes_no(label: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    print!("  {} [{}]: ", label, hint);
    io::stdout().flush().ok();
    match read_line().to_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        _ => false,
    }
}

fn meter(peak: f32) -> String {
    let filled = (peak.clamp(0.0, 1.0) * 20.0) as usize;
    format!("[{}{}]", "#".repeat(filled), "-".repeat(20 - filled))
}
