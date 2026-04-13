# lingopilot-tts-piper

A fast, local TTS server that reads JSON from stdin and outputs PCM audio. Powered by [Piper](https://github.com/rhasspy/piper) + [eSpeak-NG](https://github.com/espeak-ng/espeak-ng). Usable standalone or as a subprocess for any application.

## How It Works

`lingopilot-tts-piper` runs as a **long-lived subprocess**. The host process sends JSON requests via **stdin** (one per line) and receives responses via **stdout**.

### Protocol

**Request** (stdin, JSON per line):
```json
{
  "text": "Hello, world!",
  "language": "en",
  "voice": "en_US-hfc_female-medium",
  "speed": 1.0,
  "model_dir": "/path/to/piper/voices/en_US-hfc_female-medium",
  "espeak_data_dir": "/path/to/espeak-ng-data"
}
```

**Response** (stdout, JSON per line):
```json
{"type": "audio", "byte_length": 44100, "sample_rate": 22050, "channels": 1}
```

After the `audio` JSON line, `byte_length` raw bytes of **PCM16 LE mono** audio follow on stdout.

**Error response:**
```json
{"type": "error", "message": "eSpeak init failed: ..."}
```

**Ready signal** (sent once on startup):
```json
{"type": "ready", "version": "0.1.0"}
```

## Building

### Prerequisites

- **Rust 1.74+** (stable)
- **CMake 3.28+**
- **LLVM** (`libclang.dll` — needed by bindgen for eSpeak-NG)
- **Visual Studio 2022** (Windows) with "Desktop development with C++"

### Build

```bash
cargo build --release
```

The binary is at `target/release/lingopilot-tts-piper` (or `.exe` on Windows).

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `LINGOPILOT_TTS_LOG` | Log level (e.g. `debug`, `info`, `warn`). Logs go to stderr. |
| `ORT_DYLIB_PATH` | Path to `onnxruntime.dll` / `libonnxruntime.so` if not in system PATH. |

## Piper Voice Models

Download voice models from the [Piper releases](https://github.com/rhasspy/piper/releases) or the [Piper voices repository](https://huggingface.co/rhasspy/piper-voices).

Each voice needs:
- `<voice_id>.onnx` — the ONNX model
- `<voice_id>.onnx.json` — the config file

## License

This project is licensed under the **GNU General Public License v3.0** — see [LICENSE](LICENSE).

This is because [eSpeak-NG](https://github.com/espeak-ng/espeak-ng) (used for phonemization) is GPL v3. [Piper](https://github.com/rhasspy/piper) itself is MIT-licensed.

### Third-Party Licenses

| Component | License |
|-----------|---------|
| [Piper TTS](https://github.com/rhasspy/piper) | MIT |
| [piper-rs](https://github.com/thewh1teagle/piper-rs) | MIT |
| [eSpeak-NG](https://github.com/espeak-ng/espeak-ng) | GPL v3 |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) | MIT |
| [ort](https://github.com/pykeio/ort) | MIT / Apache 2.0 |

## Used By

- [LingoPilot](https://lingopilot.ai) — Floating Bilingual Writing Tutor (uses this as a TTS sidecar for languages not covered by Kokoro TTS)
