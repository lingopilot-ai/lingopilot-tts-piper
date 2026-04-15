# ADR: Production Readiness — `lingopilot-tts-piper`

> **Status:** Proposed
> **Date:** 2026-04-15
> **Related:** [`BACKLOG.md`](../BACKLOG.md), [`VALIDATED_BASELINE.md`](../VALIDATED_BASELINE.md), [`docs/adr-parity-alignment.md`](adr-parity-alignment.md)

## 1. Context

`lingopilot-tts-piper` already implements the sidecar-family host contract: startup `ready`, NDJSON requests and responses, PCM bytes immediately after `audio`, `stderr`-only logs, strict request parsing, and deterministic error prefixes. The remaining work for release readiness is evidence, not a protocol redesign.

`VALIDATED_BASELINE.md` records the evidence already established in this repository. `BACKLOG.md` records the open work that still blocks a truthful `READY` state.

## 2. Decision

Treat production readiness as three gates:

1. **Interface Parity Gate**: keep the sidecar-family contract aligned and documented.
2. **Functional Validation Gate**: prove successful synthesis and byte-exact output with a real voice model.
3. **Release & Distribution Gate**: prove the published GitHub Release path works end to end.

The repository remains `NOT READY` until every open blocker in those gates has execution evidence.

## 3. Interface Parity Gate

Current evidence from `VALIDATED_BASELINE.md`:

- strict request contract with `language` and `espeak_data_dir` rejection
- deterministic voice resolution without fallback to unrelated configs
- process-scoped eSpeak startup selection
- protocol/log separation on `stdout` vs `stderr`
- cross-platform CI coverage for `cargo check --locked` and `cargo test --locked`

Current expectation:

- no wire-protocol changes
- locked error prefixes stay `Invalid JSON request:`, `Invalid request payload:`, and `Synthesis failed:`
- README and implementation must stay aligned

## 4. Functional Validation Gate

Already implemented:

- real-voice validation hooks via `PIPER_TTS_REAL_VOICE_DIR` and `PIPER_TTS_REAL_VOICE_ID`
- automated test coverage for repeated same-process requests
- packaging/runtime scripts for local Windows validation

Still open from `BACKLOG.md`:

- `P2-02` real synthesis success validation with a real Piper voice
- exact `byte_length` confirmation against emitted PCM bytes with execution evidence

## 5. Release & Integration Gate

Already implemented:

- Windows packaging and checksum scripts
- packaged local archive smoke test
- published release verification script
- GitHub Actions release workflow

Still open from `BACKLOG.md`:

- `P2-01` first published GitHub Release with documented assets
- downstream verification of the published asset and checksum from the release URL

## 6. Acceptance Rule

This ADR is satisfied only when:

- `BACKLOG.md` open blockers `P2-01` and `P2-02` are executed, not merely implemented as scripts
- release-state evidence can truthfully move from `NOT READY` to `READY`
- README, backlog, and validated baseline remain mutually consistent

Until then, this repository stays `NOT READY`.
