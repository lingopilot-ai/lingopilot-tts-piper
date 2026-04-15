# Environment Variable Neutrality Map

This document records the repository-owned environment variables used by `lingopilot-tts-piper` and the naming policy around them.

## Naming Rules

- Public and semi-public repository-owned environment variables use the `PIPER_TTS_` prefix.
- `ORT_DYLIB_PATH` remains unchanged because it is the upstream `ort` override name.
- `RUST_LOG` remains the generic Rust logging fallback.
- No `LINGOPILOT_*` compatibility alias exists in this repository. The immediate-break policy was intentional to keep the public surface explicit and avoid carrying migration aliases in the sidecar family indefinitely.

## Mapping

| Variable | Classification | Purpose | Compatibility policy |
|----------|----------------|---------|----------------------|
| `PIPER_TTS_LOG` | public operational | Controls log filtering for the sidecar's `stderr` observability output. | Primary supported log env var. |
| `PIPER_ESPEAKNG_DATA_DIRECTORY` | internal process bridge | Communicates the startup-selected eSpeak runtime path into the current dependency stack before first phonemization. | Internal-only. Not part of the request protocol and not documented as a host-facing runtime switch. |
| `PIPER_TTS_REAL_VOICE_DIR` | test-only | Absolute path to a local real Piper voice fixture directory for release-readiness validation. | Supported for local validation only. |
| `PIPER_TTS_REAL_VOICE_ID` | test-only | Voice ID matching the `<voice>.onnx` and `<voice>.onnx.json` files in `PIPER_TTS_REAL_VOICE_DIR`. | Supported for local validation only. |

## Notes

- `PIPER_ESPEAKNG_DATA_DIRECTORY` is intentionally process-scoped. Hosts select the runtime through `--espeak-data-dir` at startup, and the sidecar does not accept per-request runtime overrides.
- Requests that attempt to send `espeak_data_dir` or `language` are rejected as invalid request payloads.
