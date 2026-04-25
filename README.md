# lingopilot-tts-piper

A fast, local TTS sidecar that reads newline-delimited JSON requests from `stdin`, writes JSON headers to `stdout`, and, on successful synthesis, writes raw PCM16 audio bytes immediately after the `audio` header. Powered by [Piper](https://github.com/rhasspy/piper) + [eSpeak-NG](https://github.com/espeak-ng/espeak-ng).

`README.md` is the canonical public contract for this sidecar. If the implementation and this document disagree, treat that as a defect.

## Platform Support

| Platform | GitHub Actions validation | Official GitHub Release asset | Current status |
|----------|---------------------------|-------------------------------|----------------|
| Windows x86_64 | `cargo check --locked`, `cargo test --locked`, `cargo build --release --locked` | Yes | Current downloadable artifact target |
| Linux x86_64 | `cargo check --locked`, `cargo test --locked`, `cargo build --release --locked` | No | CI-validated compile target |
| macOS | `cargo check --locked`, `cargo test --locked`, `cargo build --release --locked` | No | CI-validated compile target |

Linux and macOS are validated as compile targets in CI, but they are not yet official release artifact targets.

## Differences from `lingopilot-tts-kokoro`

| Area | `lingopilot-tts-piper` | `lingopilot-tts-kokoro` |
|------|-------------------------|-------------------------|
| `speed` range | `0.5` to `2.0` inclusive | `0.5` to `5.5` inclusive |
| Sample rate | Voice-dependent, typically `22050` | Fixed `24000` |
| `model_dir` layout | Per-voice `<voice>.onnx` and `<voice>.onnx.json` files | Shared bundle with one model plus `voices*.bin` |
| eSpeak linkage | Static linkage through `espeak-rs-sys` | Runtime loading via `libloading` |
| Binary license | `GPL-3.0-only` | `Apache-2.0` |
| eSpeak data handling | Startup-selected runtime also bridged through internal `PIPER_ESPEAKNG_DATA_DIRECTORY` | Startup-only selection without a repository-owned runtime env var |

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

```text
target/debug/espeak-runtime/    (debug build)
target/release/espeak-runtime/  (release build)
```

### 3. Start the sidecar

```powershell
.\target\release\lingopilot-tts-piper.exe
```

The sidecar takes no CLI arguments. At startup it auto-discovers the eSpeak runtime as `espeak-runtime/` next to the binary (the layout produced by the build and by the packaged release archive).

On successful startup, the sidecar emits exactly one newline-delimited `ready` JSON object on `stdout`:

```json
{"op":"ready","version":"0.1.7","sample_rate":22050,"channels":1,"encoding":"pcm16le","ops":["synthesize","phonemize"]}
```

The `version` value comes from the package version at build time. If startup validation fails, the sidecar writes `Startup error: ...` to `stderr`, exits with a non-zero status, and emits no `ready` message.

## Release Distribution

Release binaries are published as GitHub Release assets for version tags in the format `v<crate-version>`.

Current release contract:

- Tag format: `v<crate-version>`
- Windows asset name: `lingopilot-tts-piper-v<version>-windows-x86_64.zip`
- Checksum manifest: `lingopilot-tts-piper-v<version>-sha256.txt`
- Download base: `https://github.com/lingopilot-ai/lingopilot-tts-piper/releases/download/v<version>/`

Example URLs for `v0.1.3`:

```text
https://github.com/lingopilot-ai/lingopilot-tts-piper/releases/download/v0.1.3/lingopilot-tts-piper-v0.1.3-windows-x86_64.zip
https://github.com/lingopilot-ai/lingopilot-tts-piper/releases/download/v0.1.3/lingopilot-tts-piper-v0.1.3-sha256.txt
```

The Windows zip contains one top-level folder named after the asset:

```text
lingopilot-tts-piper-v0.1.3-windows-x86_64/
  lingopilot-tts-piper.exe
  espeak-runtime/
  README.md
  LICENSE
  THIRD_PARTY_LICENSES.txt
```

Release operator flow:

1. Ensure the local Windows release build, packaging, and packaged startup smoke test pass.
2. Create and push a `v<crate-version>` tag.
3. Wait for `.github/workflows/release.yml` to publish the GitHub Release assets.
4. Download and verify the published asset plus checksum.
5. Mark the release validated only after the downloaded zip passes the packaged startup smoke test.

Local Windows validation commands:

```powershell
.\build_windows.ps1 -Release
.\scripts\Package-WindowsRelease.ps1 -Version v0.1.3
.\scripts\Test-WindowsReleaseArchive.ps1 -ZipPath .\dist\lingopilot-tts-piper-v0.1.3-windows-x86_64.zip
.\scripts\Verify-Readiness.ps1 -Packaged
```

Optional publish helper for the Git branch + release tag flow:

```powershell
.\scripts\Publish-ReleaseTag.ps1 -Version v0.1.3
```

If you also want the script to stage and create the release-preparation commit first:

```powershell
.\scripts\Publish-ReleaseTag.ps1 -Version v0.1.3 -CommitMessage "Bump version to 0.1.2"
```

Published release verification command:

```powershell
.\scripts\Verify-PublishedRelease.ps1 -Version v0.1.3
```

Manual PowerShell checksum verification example:

```powershell
$version = "v0.1.3"
$asset = "lingopilot-tts-piper-$version-windows-x86_64.zip"
$checksum = "lingopilot-tts-piper-$version-sha256.txt"
$baseUrl = "https://github.com/lingopilot-ai/lingopilot-tts-piper/releases/download/$version"

Invoke-WebRequest -Uri "$baseUrl/$asset" -OutFile $asset
Invoke-WebRequest -Uri "$baseUrl/$checksum" -OutFile $checksum

$expected = (Get-Content $checksum).Split("  ")[0].Trim()
$actual = (Get-FileHash $asset -Algorithm SHA256).Hash.ToLowerInvariant()

if ($actual -ne $expected) {
    throw "SHA-256 mismatch for $asset"
}
```

## Protocol Contract

### Lifecycle

```text
Host                          Sidecar
 |                               |
 |--- spawn process ------------>|
 |                               |--- {"op":"ready", ...} ---------> stdout
 |                               |
 |--- {"op":"synthesize", ...}\n -> stdin
 |                               |--- {"op":"audio", ...} ---------> stdout
 |                               |--- [PCM16 bytes] ---------------> stdout
 |                               |--- {"op":"done", ...} ----------> stdout
 |                               |
 |--- {"op":"phonemize", ...}\n -> stdin
 |                               |--- {"op":"phonemes", "words": [...]} -> stdout
 |                               |
 |--- {"op":"synthesize", ...}\n -> stdin
 |                               |--- {"op":"error", ...} ---------> stdout
 |                               |
 |--- close stdin -------------->|  (sidecar exits cleanly)
```

### Startup Contract

Start the sidecar with no arguments:

```text
lingopilot-tts-piper
```

Rules:

- The sidecar takes no CLI arguments.
- At startup it auto-discovers the eSpeak runtime as `espeak-runtime/` next to the binary executable.
- The discovered directory must exist and contain `espeak-ng-data/`.
- The eSpeak runtime is process-scoped. To change it, start a new sidecar process with a different on-disk layout.
- Any unexpected startup argument fails startup before `ready`.

If startup validation fails:

- no protocol JSON is written to `stdout`
- an operator-facing `Startup error: ...` line is written to `stderr`
- the process exits non-zero

### Request Framing

- The host sends exactly one JSON object per line on `stdin`.
- Each request must be terminated by `\n`.
- Empty lines are ignored.
- Requests are decoded with strict field checking. Unknown fields are rejected.
- Closing `stdin` terminates the process cleanly.

### Request Schema — `synthesize`

| Field | Type | Required | Contract |
|-------|------|----------|----------|
| `op` | string | yes | Must be `"synthesize"`. |
| `id` | string | yes | Client-chosen correlation id. 1 to 128 bytes. |
| `text` | string | yes | Text to synthesize. Must contain at least one non-whitespace character and be at most `8192` Unicode scalar values. |
| `voice_model_path` | string | yes | Absolute path to the `<voice>.onnx` file. |
| `voice_config_path` | string | yes | Absolute path to the `<voice>.onnx.json` file. |
| `speaker_id` | integer | no | Optional speaker slot; defaults to `0`. |
| `speed` | number | no | Speed multiplier. Defaults to `1.0`. Must be finite; clamped to `[0.5, 2.0]` inclusive. |

Example `synthesize` request:

```json
{"op":"synthesize","id":"r1","text":"Hello, how are you?","voice_model_path":"C:\\voices\\en_US-hfc_female-medium\\en_US-hfc_female-medium.onnx","voice_config_path":"C:\\voices\\en_US-hfc_female-medium\\en_US-hfc_female-medium.onnx.json","speed":1.0}
```

### Request Schema — `phonemize`

| Field | Type | Required | Contract |
|-------|------|----------|----------|
| `op` | string | yes | Must be `"phonemize"`. |
| `id` | string | yes | Client-chosen correlation id. 1 to 128 bytes. |
| `text` | string | yes | Text to phonemize. May be empty, whitespace-only, or punctuation-only; returns `{phonemes:"", words:[]}` in those cases. |
| `language` | string | yes | BCP-47 language tag (see [Phonemize Contract](#phonemize-contract)). Unknown tags return `kind:"unsupported_language"`. |

### Request Schema — `ping`

| Field | Type | Required | Contract |
|-------|------|----------|----------|
| `op` | string | yes | Must be `"ping"`. |
| `id` | string | yes | Client-chosen correlation id. 1 to 128 bytes. Echoed byte-for-byte in the `pong` response. |

`ping` is a base-protocol health-check op available from protocol version `>= 0.1.7`. It is **not** listed in the `ops` array of the `ready` response and must never appear in `SUPPORTED_OPS`. A `ping` is dispatched before the synthesis worker, so it always responds promptly regardless of synthesis activity.

Wire shape (locked):

```json
{"op":"ping","id":"<correlation-id>"}
```

No extra fields are accepted (`deny_unknown_fields` applies). An empty or oversize `id` returns a normal `error` response with `kind:"bad_request"`. The process stays alive.

Additional request rules:

- `espeak_data_dir` is not part of the request contract. eSpeak is selected only at process startup.
- Voice resolution is strict. If `<voice>.onnx` or `<voice>.onnx.json` is missing, the sidecar returns an `error` response and never falls back to a different model.
- Piper models are cached by resolved voice config path for the lifetime of the process. Repeated requests for the same resolved voice reuse the loaded model/session.
- `text` accepts Unicode input and may contain escaped newlines such as `\n` inside the JSON string, as long as the request itself remains one newline-delimited JSON object on `stdin`.
- There is currently no separate pre-synthesis maximum audio-size contract. Output size is model- and text-dependent, bounded only by the accepted request shape and available process resources.

### Response Framing

The sidecar writes exactly one newline-delimited JSON object per response on `stdout`.

| `op` | Fields | Contract |
|------|--------|----------|
| `ready` | `version`, `sample_rate`, `channels`, `encoding`, `ops` | Emitted exactly once after successful startup. No binary data follows. |
| `audio` | `id`, `bytes`, `sample_rate`, `channels` | Successful synthesis header. Immediately after the newline, exactly `bytes` bytes of audio follow on `stdout`. |
| `done` | `id` | Emitted after the PCM payload for a `synthesize` request. |
| `phonemes` | `id`, `phonemes`, `words` | Response for a `phonemize` request. `phonemes` is the legacy top-level IPA string. `words` is always present (possibly empty). |
| `pong` | `id` | Health-check response. Echoes the `id` from the corresponding `ping` request. JSON only; no binary data follows. Available from `>= 0.1.7`. |
| `error` | `id`, `kind`, `message` | Error response. JSON only; no audio bytes follow. The process stays alive for later requests unless `stdin` is closed. |

Example `audio` header:

```json
{"op":"audio","id":"r1","bytes":123456,"sample_rate":22050,"channels":1}
```

`bytes` is the number of raw audio bytes in this response. `sample_rate` is model-dependent. `channels` is always `1`.

Example `phonemes` response:

```json
{"op":"phonemes","id":"p1","phonemes":"aɪ wʊd lˈaɪk ɐ kˈʌp ʌv kˈɔfi","words":[{"text":"I","phonemes":"aɪ"},{"text":"would","phonemes":"wʊd"},{"text":"like","phonemes":"lˈaɪk"},{"text":"a","phonemes":"ɐ"},{"text":"cup","phonemes":"kˈʌp"},{"text":"of","phonemes":"ʌv"},{"text":"coffee","phonemes":"kˈɔfi"}]}
```

### Health Check (`op:ping` / `op:pong`)

Available from protocol version `>= 0.1.7`. The host sends a `ping` request; the sidecar replies with a `pong` response that echoes the `id` byte-for-byte. The exchange proves the sidecar process is alive and reading `stdin` without touching the synthesis worker, model cache, ONNX runtime, or eSpeak.

**Wire shape (locked):**

Request:

```json
{"op":"ping","id":"<correlation-id>"}
```

Response:

```json
{"op":"pong","id":"<correlation-id>"}
```

**Floor version:** hosts must gate `HealthStrategy::Ping` on `ready.version >= "0.1.7"`.

**Ordering invariant:** `pong` is never emitted between an `audio` line and its corresponding `done` line. The single-threaded serial stdin loop structurally guarantees this — the next request is not read until the current synthesis finishes. See ADR `docs/adr-health-ping.md` §5.1 for the full invariant and guidance for any future concurrent-dispatch migration.

**Discovery:** `ping` is a base-protocol op and does not appear in the `ops` array of the `ready` response.

### Phonemize Contract

Introduced in `v0.1.6` per sidecar directive `2026-04-22e` (word-aligned output).

**Top-level output**:

- The top-level `phonemes` string is the raw eSpeak-NG IPA for the full input. It is produced by a single `espeak_TextToPhonemes` invocation and is preserved **byte-for-byte** vs `v0.1.5` for the same `(text, language)` pair. Hosts that consumed the legacy string (e.g. the Kokoro bridge) keep working unchanged.

**Per-word output**:

- `words` is always present. It is an ordered array of `{text, phonemes}` entries, where `words[i].text` reconstructs the input modulo whitespace.
- Tokenization policy: input is split on ASCII whitespace, then each token has leading/trailing punctuation (`,.!?;:"'()[]{}…—–`) stripped. Apostrophes *inside* a token are preserved, so `"I'd"` stays a single entry.
- Best-effort alignment: the top-level IPA is split on ASCII whitespace and zipped 1:1 with the input tokens. When counts disagree (e.g. eSpeak merges two adjacent tokens across punctuation into one IPA blob), trailing IPA tokens are concatenated into the last `words[]` entry so every input token has a textual slot and no IPA byte is dropped. This is a weak invariant — `words[].phonemes` joined by spaces is NOT asserted byte-equal to the top-level string.

**Empty input is legal**:

- Empty, whitespace-only, or punctuation-only `text` returns `{"phonemes":"","words":[]}` with no error. Hosts may use this to probe the sidecar without generating work.

**BCP-47 language map** (v0.1.6 coverage):

| Accepted tags | eSpeak-NG voice |
|---------------|-----------------|
| `en-US` | `en-us` |
| `en-GB` | `en-gb` |
| `en` | `en` |
| `pt-BR` | `pt-br` |
| `pt-PT`, `pt` | `pt` |
| `de`, `de-DE` | `de` |
| `es`, `es-ES` | `es` |
| `fr`, `fr-FR` | `fr` |
| `ca`, `ca-ES` | `ca` |
| `pl`, `pl-PL` | `pl` |
| `ru`, `ru-RU` | `ru` |

Tags are matched case-insensitively. Any tag outside this table returns a `{"op":"error","kind":"unsupported_language"}` response; the process stays alive.

**IPA inventory & stress markers**:

- The sidecar emits eSpeak-NG 1.52 IPA with no phoneme separators. See the [upstream phoneme tables](https://github.com/espeak-ng/espeak-ng/blob/1.52.0/docs/phonemes.md) for the inventory per voice.
- Primary stress (`ˈ`, U+02C8) and secondary stress (`ˌ`, U+02CC) are both emitted and preserved in both `phonemes` and `words[].phonemes`.

**Determinism**:

- For a fixed `(text, language)` pair, `phonemes` and `words[]` are byte-deterministic across invocations within the same process *and* across process restarts on the same build. Regressions are gated by the byte-identity fixture at `tests/fixtures/phoneme_baseline_v0_1_5.json`.

**Latency SLO**:

- Target ≤ 30 ms for typical single-sentence English (≤ 80 characters) on a warmed-up process. eSpeak initialization is a one-time cost deferred to the first `phonemize` request.

### Audio Format

The raw audio that follows an `audio` response is:

- Encoding: PCM16 signed little-endian
- Channels: 1 (mono)
- Sample rate: the `sample_rate` value from the JSON header
- Byte count: exactly `byte_length`

### Stream Rules

- Protocol traffic is on `stdout`.
- Logs are on `stderr`.
- The host must read a JSON line first, then, for `audio`, read exactly `byte_length` raw bytes before attempting to read the next JSON line.
- No logs or operator messages may be emitted on `stdout`.

### Operator Logs

Operator-facing logs are written only to `stderr` as newline-delimited plain-text records.

- Format: `level=<LEVEL> event=<EVENT> key=value ...`
- One event per line
- No ANSI color
- No timestamps
- No module path noise
- No binary payloads

The sidecar logs safe metadata such as `voice`, `speed`, `text_len`, resolved paths, cache hit/miss, and failure category. It does not log raw request text or PCM bytes.

Example:

```text
level=DEBUG event=request_received voice=en_US-hfc_female-medium speed=1 text_len=28
```

## Error Policy

The stable contract is the response shape, the stream used, and the leading error category. The full tail text may vary by platform or by the underlying OS/library error.

### Startup Failures

- Stream: `stderr`
- Format: `Startup error: ...`
- Effect: no `ready` message, non-zero exit

Example:

```text
Startup error: this sidecar takes no arguments; it auto-discovers the eSpeak runtime next to the binary.
```

### Malformed JSON Requests

- Stream: `stdout`
- Response type: `error`
- Message prefix: `Invalid JSON request:`

Example:

```json
{"type":"error","message":"Invalid JSON request: EOF while parsing an object at line 1 column 47"}
```

### Invalid Payload and Validation Errors

- Stream: `stdout`
- Response type: `error`
- Message prefix: `Invalid request payload:`

This includes semantic validation failures and invalid request paths such as:

- empty or whitespace-only `text`
- empty or whitespace-only `voice`
- empty or whitespace-only `model_dir`
- `text` longer than `8192`
- non-finite or out-of-range `speed`
- non-absolute `model_dir`
- missing or non-directory `model_dir`
- missing requested voice files inside `model_dir`
- unknown request fields such as `language` or `espeak_data_dir`

Example:

```json
{"type":"error","message":"Invalid request payload: Invalid model_dir 'relative-model-dir': path must be absolute"}
```

### Synthesis and Runtime Failures

- Stream: `stdout`
- Response type: `error`
- Message prefix: `Synthesis failed:`

These errors happen after the request shape is accepted but synthesis cannot complete.

Example:

```json
{"type":"error","message":"Synthesis failed: Failed to load Piper voice: ..."}
```

## Windows PowerShell Host Example

This example uses the raw stdout stream directly so the host can safely read newline-delimited JSON headers and the binary PCM payload from the same stream.

```powershell
function Read-LineBytes {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Stream] $Stream
    )

    $buffer = New-Object System.Collections.Generic.List[byte]
    while ($true) {
        $value = $Stream.ReadByte()
        if ($value -lt 0) {
            throw "Unexpected EOF while reading JSON header."
        }

        if ($value -eq 10) {
            return [System.Text.Encoding]::UTF8.GetString($buffer.ToArray())
        }

        if ($value -ne 13) {
            $buffer.Add([byte] $value)
        }
    }
}

function Read-ExactBytes {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Stream] $Stream,
        [Parameter(Mandatory = $true)]
        [int] $Count
    )

    $buffer = New-Object byte[] $Count
    $offset = 0

    while ($offset -lt $Count) {
        $read = $Stream.Read($buffer, $offset, $Count - $offset)
        if ($read -le 0) {
            throw "Unexpected EOF while reading PCM payload."
        }
        $offset += $read
    }

    return $buffer
}

$sidecarPath = (Resolve-Path .\target\release\lingopilot-tts-piper.exe).Path
$voiceDir = (Resolve-Path .\voices\en_US-hfc_female-medium).Path

$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $sidecarPath
$startInfo.UseShellExecute = $false
$startInfo.RedirectStandardInput = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true

$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
$null = $process.Start()

$stdout = $process.StandardOutput.BaseStream
$stdin = $process.StandardInput

$readyLine = Read-LineBytes -Stream $stdout
$ready = $readyLine | ConvertFrom-Json
if ($ready.type -ne "ready") {
    throw "Expected ready response, got: $readyLine"
}

$request = @{
    text = "Hello from PowerShell"
    voice = "en_US-hfc_female-medium"
    speed = 1.0
    model_dir = $voiceDir
} | ConvertTo-Json -Compress

$stdin.WriteLine($request)
$stdin.Flush()

$responseLine = Read-LineBytes -Stream $stdout
$response = $responseLine | ConvertFrom-Json
if ($response.type -ne "audio") {
    throw "Expected audio response, got: $responseLine"
}

$pcmBytes = Read-ExactBytes -Stream $stdout -Count ([int] $response.byte_length)
[System.IO.File]::WriteAllBytes(".\hello.raw", $pcmBytes)

$stdin.Close()
$process.WaitForExit()

$stderrText = $process.StandardError.ReadToEnd()
if ($stderrText) {
    Write-Host "stderr log output:"
    Write-Host $stderrText
}
```

The resulting `hello.raw` file contains PCM16 LE mono audio at the `sample_rate` reported by the `audio` header.

## Rust Host Example

```rust
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

fn main() {
    let mut child = Command::new("./lingopilot-tts-piper")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start sidecar");

    let stdin = child.stdin.as_mut().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let ready: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(ready["type"], "ready");

    let request = r#"{"text":"Good morning!","voice":"en_US-hfc_female-medium","speed":1.0,"model_dir":"/path/to/voice"}"#;
    writeln!(stdin, "{request}").unwrap();
    stdin.flush().unwrap();

    line.clear();
    reader.read_line(&mut line).unwrap();
    let header: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(header["type"], "audio");

    let byte_length = header["byte_length"].as_u64().unwrap() as usize;
    let mut audio = vec![0u8; byte_length];
    reader.read_exact(&mut audio).unwrap();
}
```

## Testing

Run `cargo test` to execute the automated protocol and validation suite for this repository. The current suite covers:

- startup `ready` emission
- startup failure without `ready`
- malformed JSON handling
- rejection of legacy request fields
- invalid payload validation
- exact response JSON shape and locked error-prefix parity
- Unicode and escaped-newline request text handling
- deterministic missing-voice errors
- multi-request same-process behavior

To make release-readiness validation reproducible, download the canonical real voice fixture and the compatible ONNX Runtime dylib with the repository-owned script:

```powershell
.\scripts\Download-RealVoiceFixture.ps1
```

By default, the script downloads the canonical `en_US-hfc_female-medium` fixture from the official Hugging Face Piper voice URLs into `%LOCALAPPDATA%\LingoPilot\PiperVoices\en_US-hfc_female-medium`, downloads `onnxruntime.dll` `1.20.0` into `%LOCALAPPDATA%\LingoPilot\OnnxRuntime\1.20.0`, and prints the exact environment variable values to use for validation.

Opt-in real voice validation is available for release-readiness checks outside Git-tracked assets.

Required environment variables:

- `PIPER_TTS_REAL_VOICE_DIR`: absolute path to a directory containing one real Piper voice pair
- `PIPER_TTS_REAL_VOICE_ID`: exact filename stem for that voice
- `ORT_DYLIB_PATH`: absolute path to a compatible `onnxruntime.dll` (`1.20.x` for `ort 2.0.0-rc.9`)

Targeted test command:

```powershell
$env:PIPER_TTS_REAL_VOICE_DIR = "C:\voices\en_US-hfc_female-medium"
$env:PIPER_TTS_REAL_VOICE_ID = "en_US-hfc_female-medium"
$env:ORT_DYLIB_PATH = "$env:LOCALAPPDATA\LingoPilot\OnnxRuntime\1.20.0\onnxruntime.dll"
cargo test --locked real_voice_fixture_allows_two_successive_audio_responses_when_configured -- --exact --nocapture
```

Windows operator validation command:

```powershell
$env:PIPER_TTS_REAL_VOICE_DIR = "C:\voices\en_US-hfc_female-medium"
$env:PIPER_TTS_REAL_VOICE_ID = "en_US-hfc_female-medium"
$env:ORT_DYLIB_PATH = "$env:LOCALAPPDATA\LingoPilot\OnnxRuntime\1.20.0\onnxruntime.dll"
.\scripts\Test-RealVoiceFixture.ps1
```

Optional Windows special-path validation command:

```powershell
$env:PIPER_TTS_REAL_VOICE_DIR = "C:\voices\en_US-hfc_female-medium"
$env:PIPER_TTS_REAL_VOICE_ID = "en_US-hfc_female-medium"
$env:ORT_DYLIB_PATH = "$env:LOCALAPPDATA\LingoPilot\OnnxRuntime\1.20.0\onnxruntime.dll"
cargo test --locked real_voice_fixture_supports_model_dir_with_space_and_non_ascii_when_configured -- --exact --nocapture
```

If those environment variables are absent, the normal `cargo test --locked` run stays green and skips the real-voice success validation.

Repository readiness gate commands:

```powershell
.\scripts\Verify-Readiness.ps1
.\scripts\Verify-Readiness.ps1 -RequireRealVoice
.\scripts\Verify-Readiness.ps1 -Packaged
.\scripts\Verify-Readiness.ps1 -Packaged -RequireRealVoice
```

`-RequireRealVoice` fails unless `PIPER_TTS_REAL_VOICE_DIR` and `PIPER_TTS_REAL_VOICE_ID` are set. It uses `ORT_DYLIB_PATH` when provided, otherwise falls back to the canonical runtime path created by `.\scripts\Download-RealVoiceFixture.ps1`.

GitHub Actions also defines a platform matrix in `.github/workflows/ci.yml`:

- `windows-latest`: `cargo check --locked`, `cargo test --locked`, `cargo build --release --locked`
- `ubuntu-latest`: `cargo check --locked`, `cargo test --locked`, `cargo build --release --locked`
- `macos-latest`: `cargo check --locked`, `cargo test --locked`, `cargo build --release --locked`

Those CI runs validate that Linux and macOS remain compile-ready targets, but they do not by themselves promote Linux or macOS to official release artifact targets.

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
.\build_windows.ps1
.\build_windows.ps1 -Release
.\build_windows.ps1 -Release -Locked
```

The script auto-detects Visual Studio, LLVM, and Ninja and sets the required environment variables for a Windows build.

### Build (Linux / macOS)

```bash
cargo build --release --locked
```

Ensure `cmake`, `ninja`, and `libclang` are installed via your package manager.

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `PIPER_TTS_LOG` | Log level (`debug`, `info`, `warn`). Logs go to `stderr` in `level=<LEVEL> event=<EVENT> ...` format and never to `stdout`. |
| `PIPER_TTS_REAL_VOICE_DIR` | Development-only absolute path to a local real-voice fixture directory for release-readiness validation. Not required for normal builds or tests. |
| `PIPER_TTS_REAL_VOICE_ID` | Development-only voice ID for the local real-voice fixture. Must match the `<voice_id>.onnx` and `<voice_id>.onnx.json` files in `PIPER_TTS_REAL_VOICE_DIR`. |
| `ORT_DYLIB_PATH` | Path to `onnxruntime.dll` / `libonnxruntime.so` if not next to the binary. |

## Canonical Release Validation Flow

Use this exact sequence for the first public Windows release:

```powershell
.\scripts\Download-RealVoiceFixture.ps1
$env:PIPER_TTS_REAL_VOICE_DIR = "$env:LOCALAPPDATA\LingoPilot\PiperVoices\en_US-hfc_female-medium"
$env:PIPER_TTS_REAL_VOICE_ID = "en_US-hfc_female-medium"
$env:ORT_DYLIB_PATH = "$env:LOCALAPPDATA\LingoPilot\OnnxRuntime\1.20.0\onnxruntime.dll"

.\scripts\Verify-Readiness.ps1 -RequireRealVoice
.\build_windows.ps1 -Release -Locked
.\scripts\Package-WindowsRelease.ps1 -Version v0.1.3
.\scripts\Verify-Readiness.ps1 -Packaged -RequireRealVoice
.\scripts\Publish-ReleaseTag.ps1 -Version v0.1.3
.\scripts\Verify-PublishedRelease.ps1 -Version v0.1.3
```

Interpretation:

- `Verify-Readiness.ps1 -RequireRealVoice` proves the local tree passes deterministic and real-voice validation.
- `Verify-Readiness.ps1 -Packaged -RequireRealVoice` keeps the same real-voice gate while also verifying the most recent packaged Windows archive.
- `Publish-ReleaseTag.ps1` pushes the branch and the `v0.1.3` tag.
- `Verify-PublishedRelease.ps1` completes the downstream validation by checking the published GitHub asset and checksum from the release URL.

## Vendored `espeak-rs-sys`

This repository intentionally keeps `vendor/espeak-rs-sys` on `main`.

- Upstream baseline: `espeak-rs-sys 0.1.9`
- Current local patch areas:
  - remove the explicit Windows debug `msvcrtd` link
  - publish compiled `espeak-runtime` assets into `target/<profile>/espeak-runtime`
  - force Windows CMake reconfiguration
  - invalidate builds when relevant eSpeak environment variables change

The detailed governance, traceability, rebase procedure, and keep/remove criteria are documented in [docs/vendor-espeak-rs-sys.md](docs/vendor-espeak-rs-sys.md).

## Piper Voice Models

Download voice models from [Piper voices on HuggingFace](https://huggingface.co/rhasspy/piper-voices).

Each voice requires two files in the same directory:

- `<voice_id>.onnx` — the neural network model
- `<voice_id>.onnx.json` — the config file

When sending a request, `voice` must exactly match that filename stem. For example, `voice = "en_US-hfc_female-medium"` requires both `en_US-hfc_female-medium.onnx` and `en_US-hfc_female-medium.onnx.json` in `model_dir`.

Browse available voices: https://rhasspy.github.io/piper-samples/

## License

This project is licensed under the **GNU General Public License v3.0** — see [LICENSE](LICENSE).

This is because [eSpeak-NG](https://github.com/espeak-ng/espeak-ng) is GPL v3. [Piper](https://github.com/rhasspy/piper) itself is MIT-licensed.

### Third-Party Licenses

| Component | License |
|-----------|---------|
| [Piper TTS](https://github.com/rhasspy/piper) | MIT |
| [piper-rs](https://github.com/thewh1teagle/piper-rs) | MIT |
| [eSpeak-NG](https://github.com/espeak-ng/espeak-ng) | GPL v3 |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) | MIT |
| [ort](https://github.com/pykeio/ort) | MIT / Apache 2.0 |

The packaged Windows archive also includes `THIRD_PARTY_LICENSES.txt` with the repository's third-party license disclosure summary.
