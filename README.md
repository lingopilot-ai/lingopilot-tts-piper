# lingopilot-tts-piper

A fast, local TTS server that reads JSON from stdin and outputs PCM audio. Powered by [Piper](https://github.com/rhasspy/piper) + [eSpeak-NG](https://github.com/espeak-ng/espeak-ng). Usable standalone or as a subprocess for any application.

## Quick Start

### 1. Download a Piper voice

```bash
# Example: English US female voice
mkdir -p ~/piper-voices/en_US-hfc_female-medium
cd ~/piper-voices/en_US-hfc_female-medium
curl -LO https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/hfc_female/medium/en_US-hfc_female-medium.onnx
curl -LO https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/hfc_female/medium/en_US-hfc_female-medium.onnx.json
```

### 2. Locate eSpeak-NG data

After building, the eSpeak runtime data is at:
```
target/debug/espeak-runtime/    (debug build)
target/release/espeak-runtime/  (release build)
```

### 3. Run the sidecar

```bash
# Start the sidecar (it stays alive, waiting for requests on stdin)
./lingopilot-tts-piper
```

On startup, it prints a ready signal to stdout:
```json
{"type":"ready","version":"0.1.0"}
```

### 4. Send a TTS request

Send a single JSON line to stdin:
```json
{"text":"Hello, how are you?","language":"en","voice":"en_US-hfc_female-medium","speed":1.0,"model_dir":"/home/user/piper-voices/en_US-hfc_female-medium","espeak_data_dir":"target/release/espeak-runtime"}
```

The sidecar responds on stdout with:
```json
{"type":"audio","byte_length":95040,"sample_rate":22050,"channels":1}
```

Immediately after this JSON line, `byte_length` raw bytes of **PCM16 LE mono** audio follow on stdout.

### 5. Full end-to-end example (Bash)

```bash
#!/bin/bash
# Synthesize "Hello world" and save as a WAV file.

SIDECAR="./target/release/lingopilot-tts-piper"
VOICE_DIR="$HOME/piper-voices/en_US-hfc_female-medium"
ESPEAK_DIR="./target/release/espeak-runtime"
OUTPUT="hello.wav"

# Start sidecar, send request, capture binary output
REQUEST='{"text":"Hello world!","language":"en","voice":"en_US-hfc_female-medium","speed":1.0,"model_dir":"'$VOICE_DIR'","espeak_data_dir":"'$ESPEAK_DIR'"}'

echo "$REQUEST" | $SIDECAR > output.raw 2>/dev/null

# The first line of output.raw is the JSON header, followed by raw PCM bytes.
# In practice, your host app reads the JSON header to learn byte_length and
# sample_rate, then reads exactly that many bytes of PCM16 LE audio.
```

### 6. End-to-end example (Rust host)

```rust
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

fn main() {
    // Spawn the sidecar
    let mut child = Command::new("./lingopilot-tts-piper")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start sidecar");

    let stdin = child.stdin.as_mut().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    // Wait for ready signal
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    println!("Sidecar: {}", line.trim());
    // Output: Sidecar: {"type":"ready","version":"0.1.0"}

    // Send a TTS request
    let request = r#"{"text":"Good morning!","language":"en","voice":"en_US-hfc_female-medium","speed":1.0,"model_dir":"/path/to/voice","espeak_data_dir":"/path/to/espeak-runtime"}"#;
    writeln!(stdin, "{}", request).unwrap();
    stdin.flush().unwrap();

    // Read the JSON response header
    line.clear();
    reader.read_line(&mut line).unwrap();
    println!("Response: {}", line.trim());
    // Output: Response: {"type":"audio","byte_length":72000,"sample_rate":22050,"channels":1}

    // Parse byte_length from JSON and read that many bytes of PCM16 audio
    let header: serde_json::Value = serde_json::from_str(&line).unwrap();
    let byte_length = header["byte_length"].as_u64().unwrap() as usize;
    let mut audio = vec![0u8; byte_length];
    reader.read_exact(&mut audio).unwrap();

    println!("Received {} bytes of PCM16 audio", audio.len());
    // Output: Received 72000 bytes of PCM16 audio

    // The audio is raw PCM16 LE, mono, at the sample_rate from the header.
    // You can play it, write a WAV, or stream it to an audio device.
}
```

## Protocol Reference

### Lifecycle

```
Host                          Sidecar
 |                               |
 |--- spawn process ------------>|
 |                               |--- {"type":"ready"} ---> stdout
 |                               |
 |--- {"text":"..."} --> stdin   |
 |                               |--- {"type":"audio"} ---> stdout
 |                               |--- [PCM16 bytes] ------> stdout
 |                               |
 |--- {"text":"..."} --> stdin   |  (sidecar stays alive for next request)
 |                               |--- {"type":"audio"} ---> stdout
 |                               |--- [PCM16 bytes] ------> stdout
 |                               |
 |--- close stdin -------------->|  (sidecar exits cleanly)
```

### Request Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `text` | string | yes | Text to synthesize |
| `language` | string | yes | eSpeak language code (e.g. `"en"`, `"de"`, `"pt-br"`) |
| `voice` | string | yes | Piper voice ID (e.g. `"en_US-hfc_female-medium"`) |
| `speed` | float | no | Speed multiplier, default `1.0` |
| `model_dir` | string | yes | Absolute path to directory containing the `.onnx` + `.onnx.json` files |
| `espeak_data_dir` | string | yes | Absolute path to eSpeak runtime data (contains `espeak-ng-data/`) |

### Response Types

| Type | Fields | Description |
|------|--------|-------------|
| `ready` | `version` | Sent once on startup. Sidecar is ready to accept requests. |
| `audio` | `byte_length`, `sample_rate`, `channels` | Audio header. Followed by `byte_length` raw bytes of PCM16 LE mono. |
| `error` | `message` | Something went wrong. No audio follows. Sidecar stays alive. |

### Audio Format

The raw audio after an `audio` response is:
- **Encoding:** PCM16 signed, little-endian
- **Channels:** 1 (mono)
- **Sample rate:** reported in the `audio` header (typically 22050 Hz)
- **Byte order:** little-endian (each sample is 2 bytes, LSB first)

## Building

### Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.74+ stable | Compiler |
| CMake | 3.28+ | eSpeak-NG build |
| Ninja | any | CMake generator |
| LLVM | any | `libclang.dll` for bindgen |
| Visual Studio 2022 | (Windows) | MSVC toolchain + C++ workload |

### Build (Windows)

Use the provided PowerShell script:
```powershell
.\build_windows.ps1            # debug build
.\build_windows.ps1 -Release   # release build
```

The script auto-detects Visual Studio, LLVM, and Ninja. It sets all required environment variables.

### Build (Linux / macOS)

```bash
cargo build --release
```

Ensure `cmake`, `ninja`, and `libclang` are installed via your package manager.

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `LINGOPILOT_TTS_LOG` | Log level (`debug`, `info`, `warn`). Logs go to **stderr** (not stdout). |
| `ORT_DYLIB_PATH` | Path to `onnxruntime.dll` / `libonnxruntime.so` if not next to the binary. |

## Piper Voice Models

Download voice models from [Piper voices on HuggingFace](https://huggingface.co/rhasspy/piper-voices).

Each voice requires two files in the same directory:
- `<voice_id>.onnx` — the neural network model
- `<voice_id>.onnx.json` — the config (sample rate, phoneme mapping, etc.)

Browse available voices: https://rhasspy.github.io/piper-samples/

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
