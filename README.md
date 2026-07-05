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
| `audio.wake_word_model` | Path to `.rpw` wake word model file |
| `audio.input` | Enable microphone/wake-word/STT pipeline (default `true`; set `false` for speaker-only nodes) |
| `audio.output` | Enable TTS/Matrix-response pipeline (default `true`; set `false` for mic-only nodes) |
| `stt.model` | Whisper model name (e.g. `base.en`) |
| `stt.model_path` | Directory containing `<model>.bin` |
| `tts.provider` | `"piper"` or `"elevenlabs"` |
| `tts.piper_binary` | Path to piper binary |
| `tts.piper_voice` | Path to `.onnx` voice model |
| `tts.elevenlabs.api_key` | ElevenLabs API key |
| `tts.elevenlabs.voice_id` | ElevenLabs voice ID |

## Wake word

Wake word detection uses **rustpotter v2**, loaded from git (`tag = "v2.0.0"`) because v2 was yanked from crates.io. Model files use the `.rpw` format.

**Creating a model:**

- Install `rustpotter-cli` (the v2 branch) and record several WAV samples of your wake phrase, then build:
  ```sh
  rustpotter-cli build-wakeword --name "hey-tank" --samples hey1.wav hey2.wav hey3.wav -o hey-tank.rpw
  ```
- Alternatively, use the rustpotter v2 Rust API directly to construct a `Wakeword` from WAV samples and save it.

Set `audio.wake_word_model` in `config.toml` to the path of your `.rpw` file.

**rustpotter v3 remains blocked** — `rustpotter 3.x` pulls in `candle-core 0.2.2`, which has an unresolved `rand 0.8`/`0.9` transitive dependency conflict.

## License

MIT — see [LICENSE](LICENSE).
