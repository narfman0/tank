# tank

A Matrix voice terminal: speak → wake word → STT → Matrix message → receive response → TTS playback.

You talk to it, it sends your words as a Matrix message, waits for a reply from a bot, and reads the reply aloud.

## Requirements

- Rust 1.75+
- [piper](https://github.com/rhasspy/piper) binary in PATH or configured path (for local TTS)
- Whisper model files (GGML format, e.g. from [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp))
- A Matrix account and homeserver
- A microphone

## Setup

```sh
cp config.example.toml config.toml
# edit config.toml with your Matrix credentials, room IDs, and paths
cargo build --release
./target/release/tank --config config.toml
```

Download a whisper model:
```sh
mkdir -p models
wget -O models/base.en.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```

## Config reference

| Field | Description |
|---|---|
| `matrix.homeserver` | Matrix homeserver URL |
| `matrix.username` | Your Matrix user ID (`@user:server.com`) |
| `matrix.password` | Password (used only for initial login; session is cached after) |
| `matrix.session_file` | Path to persist the login session JSON |
| `matrix.rooms[].id` | Room ID (`!room:server.com`) |
| `matrix.rooms[].listen` | Receive TTS responses from this room |
| `matrix.rooms[].send` | Send STT transcriptions to this room |
| `audio.input_device` | CPAL input device name, or `"default"` |
| `audio.output_device` | CPAL output device name, or `"default"` |
| `audio.wake_word_model` | Path to wake word model file (currently unused; energy VAD is active) |
| `stt.model` | Whisper model name (e.g. `base.en`) |
| `stt.model_path` | Directory containing `<model>.bin` |
| `tts.provider` | `"piper"` or `"elevenlabs"` |
| `tts.piper_binary` | Path to piper binary |
| `tts.piper_voice` | Path to `.onnx` voice model |
| `tts.elevenlabs.api_key` | ElevenLabs API key |
| `tts.elevenlabs.voice_id` | ElevenLabs voice ID |

## Wake word

Currently uses energy-threshold VAD (any loud sound triggers recording) because `rustpotter 3.x` has a transitive dependency conflict via `candle-core`. The `wake_word_model` config field is reserved for when that dep is resolved or an alternative library is wired in.

To adjust sensitivity, the threshold is set in `src/wake_word.rs` (`threshold: 0.02`). Lower = more sensitive.

## License

MIT — see [LICENSE](LICENSE).
