# VALIDATED_BASELINE.md

This document tracks the repository baseline that is already implemented and locally validated.

Validated locally in this repository:

- `cargo check --locked` passes.
- `cargo test --locked` passes.
- The request contract no longer accepts legacy `language` or `espeak_data_dir` fields.
- Voice resolution is strict and does not fall back to unrelated `.onnx.json` files.
- eSpeak runtime selection is process-scoped through `--espeak-data-dir` at startup.
- Minimum input validation exists for `text`, `speed`, and `model_dir`.
- Operational limits are explicit in `README.md`:
  - `text` is capped at `8192` Unicode scalar values
  - Unicode request text is accepted
  - escaped newlines are accepted inside the JSON string payload
  - there is no separate pre-synthesis maximum audio-size contract today
- Automated coverage includes Unicode and escaped-newline request handling.
- Piper models are cached by resolved voice config path for the lifetime of the process.
- Protocol/log separation is enforced: protocol traffic stays on `stdout`, logs stay on `stderr`.
- CI runs `cargo check --locked` and `cargo test --locked` on Windows, Linux, and macOS.
- Windows release packaging, checksum generation, and packaged startup smoke testing are defined in repository-owned scripts and workflows.
- Repo-owned scripts now exist for:
  - canonical real-voice fixture download plus compatible `ORT_DYLIB_PATH` provisioning
  - local real-voice validation using `PIPER_TTS_REAL_VOICE_DIR` and `PIPER_TTS_REAL_VOICE_ID`
  - published GitHub Release asset download plus checksum verification
- Local real-voice validation passed with exact `byte_length` checks using the canonical `en_US-hfc_female-medium` fixture and a compatible `onnxruntime.dll`.
- The published `v0.1.2` GitHub Release asset and checksum were downloaded and verified through the documented downstream path.
- Vendored `espeak-rs-sys` governance is documented in `docs/vendor-espeak-rs-sys.md`.
