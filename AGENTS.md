# AGENTS.md

## 1. Project Identity

- Project name: `lingopilot-tts-piper`
- Project type: local TTS sidecar process
- Primary role: read one JSON request per line from `stdin`, write one JSON response to `stdout`, and, on success, write raw PCM bytes immediately after the JSON header
- Current binary: `lingopilot-tts-piper`
- Current language/runtime: Rust 2021

## 2. Language Policy

All project-facing written content must be in English.

This applies to:

- repository documentation
- backlog items
- agent instructions
- issue templates
- pull request descriptions created for this repository
- code comments added by agents

This does not require rewriting third-party vendored code.

## 3. Product Scope

This project is acceptable only if all statements below are true:

- It runs as a long-lived sidecar process.
- It does not require network access at request time.
- It does not write arbitrary files at request time.
- It keeps protocol traffic on `stdout`.
- It keeps logs on `stderr`.
- It works on Windows now.
- It does not introduce platform-specific behavior in the request protocol.
- It can be packaged as a release artifact for downstream project consumption.

This project is out of scope if any change does one of the following:

- Turns the binary into a general application server.
- Adds HTTP, gRPC, or sockets as the primary interface.
- Makes cloud connectivity mandatory.
- Couples the protocol to LingoPilot-only private assumptions without documenting them.

## 4. Canonical Files

Use these files as the source of truth:

- [`src/main.rs`](./src/main.rs): sidecar lifecycle, protocol loop, stdout/stderr behavior
- [`src/protocol.rs`](./src/protocol.rs): request/response schema
- [`src/synthesis.rs`](./src/synthesis.rs): model loading, eSpeak init, synthesis path
- [`Cargo.toml`](./Cargo.toml): dependency policy and vendor patch policy
- [`README.md`](./README.md): public usage contract
- [`build_windows.ps1`](./build_windows.ps1): Windows build workflow

If implementation and README disagree, the discrepancy must be treated as a defect, not as acceptable ambiguity.

## 5. Protocol Invariants

Any change is acceptable only if all invariants remain true:

- On startup, the process emits exactly one `ready` JSON object on `stdout`.
- Each request is exactly one JSON object terminated by `\n`.
- Each successful response emits exactly one `audio` JSON object terminated by `\n`.
- After an `audio` response, exactly `byte_length` bytes of PCM16 LE mono audio follow on `stdout`.
- An `error` response is JSON only. No audio bytes may follow an `error`.
- Logs may not be emitted on `stdout`.
- A malformed request must not crash the process.
- Closing `stdin` must terminate the process cleanly.
- From `0.1.7` onward, `op:ping` is a mandatory base-protocol op. It must NOT appear in `SUPPORTED_OPS` (it is not a negotiated capability). Ping dispatch must run before `handle_request` so the synthesis worker, model cache, ONNX runtime, and eSpeak are never touched during a health check.
- `pong` MUST NOT be emitted while a synthesis has emitted `audio` but not yet emitted `done` for that same request (ADR `docs/adr-health-ping.md` §5.1 ordering invariant). The current single-threaded serial stdin loop guarantees this structurally. Any future migration to concurrent dispatch MUST preserve this invariant explicitly — it is load-bearing for PCM framing on the host side.

## 6. Current Reality Gaps

These are known gaps in the current codebase. Agents must not normalize them:

- Real synthesis validation still depends on an external real voice fixture that is intentionally not stored in Git-tracked files.
- Real synthesis on Windows depends on a compatible `onnxruntime.dll` (`1.20.x` for `ort 2.0.0-rc.9`) when that DLL is not already adjacent to the binary.
- Linux and macOS remain CI-validated compile/protocol targets, but Windows x86_64 is still the only official GitHub Release artifact target.

## 7. Binary Acceptance Criteria

Every feature change must be judged with `PASS` or `FAIL`.

### 7.1 Functional Gate

PASS only if all are true:

- `cargo check` succeeds.
- The binary starts and emits `{"type":"ready",...}`.
- A malformed JSON line returns `{"type":"error",...}` and the process remains alive.
- A valid synthesis request returns `{"type":"audio",...}` and exactly `byte_length` bytes follow.
- A second valid request in the same process also succeeds.

FAIL if any statement above is false.

### 7.2 Protocol Gate

PASS only if all are true:

- No logs are written to `stdout`.
- JSON headers are newline-delimited.
- Audio bytes are emitted only after `audio` responses.
- `byte_length` matches the actual number of emitted audio bytes.

FAIL if any statement above is false.

### 7.3 Security Baseline Gate

PASS only if all are true:

- Invalid JSON does not crash the process.
- Invalid `model_dir` returns an error response.
- Invalid `espeak_data_dir` returns an error response or a deterministic failure mode documented in the repository.
- Input validation rejects values outside documented bounds.
- No request path causes shell execution.
- No request path performs network access.

FAIL if any statement above is false.

### 7.4 Cross-Platform Gate

PASS only if all are true:

- The protocol format is identical on Windows, Linux, and macOS.
- Build logic does not require editing source files per platform.
- Platform-specific build behavior is isolated to build scripts or dependency configuration.

FAIL if any statement above is false.

## 8. Required Validation by Change Type

### 8.1 Protocol Changes

Required before merge:

- Update `src/protocol.rs`
- Update `README.md`
- Add or update protocol tests
- Verify backward-compatibility policy is explicit

Merge is blocked if any item is missing.

### 8.2 Synthesis Path Changes

Required before merge:

- Validate one successful synthesis request with a real voice model
- Validate one error path with a missing model/config
- Validate multiple requests in the same process
- Verify `byte_length` equals emitted bytes

Merge is blocked if any item is missing.

### 8.3 Build or Dependency Changes

Required before merge:

- `cargo check`
- Windows build validation
- README/build instructions updated if the operator workflow changed
- Vendor policy reviewed if a vendored crate or patched crate changed

Merge is blocked if any item is missing.

## 9. Dependency Policy

Current dependency facts in this repository:

- `piper-rs = 0.1.9`
- `ort = 2.0.0-rc.9`
- `ort-sys = 2.0.0-rc.9`
- `espeak-rs-sys = 0.1.9`
- `espeak-rs-sys` is patched to `vendor/espeak-rs-sys`

Rules:

- Do not upgrade runtime dependencies opportunistically.
- Do not change `ort` independently of compatibility validation with `piper-rs`.
- Do not remove the `espeak-rs-sys` vendor patch unless Windows debug builds are revalidated.
- Any dependency upgrade must document:
  - current version
  - target version
  - reason for change
  - verified platforms
  - verified protocol behavior

## 10. Vendor Policy

The local vendor is acceptable only if all are true:

- The vendored crate differs from upstream for a documented reason.
- The diff is small enough to explain.
- The repository states why the vendor exists.
- The vendored code is still traceable to an upstream version.

The local vendor is not acceptable if any are true:

- The patch reason is unknown.
- The vendored code drifts without review.
- The repository cannot explain which upstream version it is based on.

Current vendor rule for this project:

- Keep `vendor/espeak-rs-sys` until Windows debug and runtime-asset publishing are validated without it.

## 11. Required Test Matrix

Minimum matrix for release readiness:

- Windows:
  - `cargo check`
  - startup `ready` test
  - malformed JSON test
  - one real synthesis test
  - multi-request same-process test
- Linux:
  - `cargo check`
  - startup `ready` test
  - malformed JSON test
  - one real synthesis test
- macOS:
  - `cargo check`
  - startup `ready` test
  - malformed JSON test
  - one real synthesis test

If a platform is not executed, that platform is not release-ready.

## 12. Distribution Policy

Default distribution target:

- publish release binaries as GitHub Release assets

Default download strategy for downstream consumers:

- use GitHub Release asset URLs
- treat GitHub asset delivery as the default CDN path unless a separate distribution channel is explicitly adopted

Required release artifact properties:

- versioned file names
- reproducible release build steps
- checksum file for published binaries
- platform-specific artifacts clearly named

Release assets are acceptable only if all are true:

- they are built from the tagged or release commit
- they are release-mode binaries
- checksums are published with the assets
- the downstream project can resolve a stable versioned download URL or release lookup flow

## 13. Release Readiness Rule

A release is `READY` only if all statements below are true:

- README usage matches actual behavior.
- No known protocol mismatch remains undocumented.
- Windows validation passed.
- Required tests for the target release scope passed.
- Dependency and vendor state is documented.
- release artifact generation is defined
- release distribution method is defined

Otherwise the release state is `NOT READY`.

## 14. Agent Working Rules

When modifying this repository:

- Write new project-facing text in English.
- Prefer deterministic behavior over convenience fallbacks.
- Prefer explicit errors over silent recovery.
- Do not change the wire protocol casually.
- Do not add hidden behavior that is absent from README.
- Do not merge undocumented vendor changes.
- Do not claim cross-platform support without running the relevant checks.
- Do not define a release process without specifying how downstream projects will download binaries.

## 15. Fast Post-Change Validation Loop

After any code change that touches the sidecar (protocol, synthesis, build, or dependencies), the agent must run both gates below before declaring the task done. Both are fast (seconds once the fixture and release binary are cached).

### 15.1 Gate A — Real Unit Tests

Command:

```
cargo test --release
```

This runs the real protocol and contract tests in [`src/protocol.rs`](./src/protocol.rs) and [`tests/request_contract.rs`](./tests/request_contract.rs). No mocks.

### 15.2 Gate B — Audible Real E2E

Command (PowerShell):

```
pwsh -File scripts/Test-AudibleE2E.ps1
```

Script: [`scripts/Test-AudibleE2E.ps1`](./scripts/Test-AudibleE2E.ps1). It uses voice `en_US-hfc_female-medium`, auto-resolves the fixture via [`scripts/Download-RealVoiceFixture.ps1`](./scripts/Download-RealVoiceFixture.ps1), spawns the real release binary, exercises the full `synthesize` path, validates the `audio` envelope (`bytes == byte_length`, mono, positive `sample_rate`), asserts the PCM is non-silent (RMS threshold), validates the `done` envelope, and plays the synthesized audio on the default output device so the operator hears the voice.

Pass options `-Play:$false` only in headless CI. Human validation of the task requires playback.

### 15.3 Failure Handling Rule

If any gate fails, the agent must:

1. Read `stderr`, the script error line, and the relevant source file to identify the root cause.
2. Fix the underlying defect in the code (not in the test, unless the test itself encodes a wrong assumption).
3. Re-run the failing gate.
4. Repeat until both gates return `PASS` in a single clean cycle.

The agent must not:

- Skip, weaken, or disable a test to make it pass.
- Declare the task done while any gate is `FAIL`.
- Silence a failure with `--no-verify`, `continue-on-error`, or environment fallbacks.

The task is `OK` only after one full clean cycle where Gate A and Gate B both pass end-to-end.

## 16. Definition of Done

A task is done only if all are true:

- Code changes compile.
- Relevant tests or manual validations were executed.
- Public docs changed if behavior changed.
- Acceptance criteria in this file evaluate to `PASS`.

If any item is false, the task is not done.
