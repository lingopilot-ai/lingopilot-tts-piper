# BACKLOG.md

This backlog tracks only outstanding work. The current implemented baseline now lives in `VALIDATED_BASELINE.md`.

## Related Documents

- `VALIDATED_BASELINE.md`
- `docs/adr-parity-alignment.md`
- `docs/adr-production-readiness.md`
- `docs/adr-health-ping.md`
- `docs/environment-variable-neutrality.md`
- `docs/vendor-espeak-rs-sys.md`

## 1. Open Backlog

No open items.

## 2. Dependency Watch Items

### D-01. Evaluate `ort` Upgrade Only Through a Compatibility Branch

Current state:

- repository uses `ort = 2.0.0-rc.9`

Rule:

- do not upgrade on `main` without dedicated compatibility validation for `piper-rs`

Acceptance criteria:

- `PASS` if any upgrade is justified by a real need and validated with build plus synthesis checks
- `FAIL` if a version bump happens only because a newer release exists

### D-02. Monitor Upstream `piper-rs`

Current state:

- repository uses `piper-rs = 0.1.9`

Rule:

- update only for a real sidecar need with compatibility validation

Acceptance criteria:

- `PASS` if any update addresses a concrete project need
- `FAIL` if churn happens without measurable gain

## 3. Release Notes (additive, most recent first)

### v0.1.7 — `op:ping` / `op:pong` health-check contract (H-01)

- Adds `op:ping` request and `op:pong` response as a mandatory base-protocol health-check pair. Wire shapes locked: request `{"op":"ping","id":"<id>"}`, response `{"op":"pong","id":"<id>"}`. No extra fields on either side (`deny_unknown_fields` enforced on the request).
- `ping` is dispatched before the synthesis worker — it never touches the model cache, ONNX runtime, or eSpeak, so it responds promptly regardless of synthesis activity.
- `ping` is intentionally absent from the `ops` array in the `ready` response and must not appear in `SUPPORTED_OPS`. Hosts gate `HealthStrategy::Ping` on `ready.version >= "0.1.7"`.
- Ordering invariant (ADR `docs/adr-health-ping.md` §5.1): `pong` is never emitted between an `audio` line and its `done` line. Structurally guaranteed by the serial stdin loop; must be preserved explicitly by any future concurrent-dispatch migration.
- Integration test in `tests/ping_contract.rs` validates pong byte-exactness and absence of framing disruption around two consecutive synthesis requests.
- ADR `docs/adr-health-ping.md` Status promoted to Accepted.
- Peer sidecar (`lingopilot-tts-kokoro`) must adopt the identical wire contract; coordinate deployment so the host kit can enable `HealthStrategy::Ping` for both.

### v0.1.6 — Word-aligned phonemize (directive 2026-04-22e)

- `phonemize` responses now include a `words` array (`[{text, phonemes}, ...]`) alongside the legacy top-level `phonemes` string. The top-level string is preserved **byte-for-byte** vs `v0.1.5` for every en-US corpus entry in `tests/fixtures/phoneme_baseline_v0_1_5.json`.
- Per-word phonemes are produced by splitting the single-eSpeak-call IPA output on ASCII whitespace. Input tokens are split on whitespace with leading/trailing punctuation stripped (apostrophes inside a token preserved). When eSpeak merges adjacent words into one IPA token, the last `words[]` entry absorbs the trailing IPA tail so no byte is dropped.
- Empty, whitespace-only, and punctuation-only `text` values are now legal — the sidecar returns `{"phonemes":"","words":[]}` instead of a `bad_request` error.
- BCP-47 language tags (case-insensitive) are mapped to eSpeak voices. v0.1.6 coverage: `en-US`, `en-GB`, `en`, `pt-BR`, `pt-PT`/`pt`, `de(-DE)`, `es(-ES)`, `fr(-FR)`, `ca(-ES)`, `pl(-PL)`, `ru(-RU)`.
- New error `kind`: `unsupported_language` for BCP-47 tags outside the map. Sidecar stays alive after the error.
- README drift cleanup: speed-range column swap (piper `2.0`, kokoro `5.5`), `ready` line reflects the `op:ready` shape, and the request-schema table documents `voice_model_path`/`voice_config_path` instead of the stale `voice`/`model_dir` pair.

## 4. Release State

Current release state:

- Windows protocol/startup/error-path validation: `PASS`
- Windows packaged local artifact validation: `PASS`
- Operational limits documentation and automated coverage: `PASS`
- Local real-voice validation path: `PASS`
- Windows published GitHub Release validation: `PASS`
- Linux CI validation: `CONFIGURED`
- macOS CI validation: `CONFIGURED`
- Local `espeak-rs-sys` vendor: `KEEP`
- Minimum security baseline: `PASS` for validated startup/error-path coverage
- Successful synthesis release gate: `PASS`
- Open-source release readiness: `READY`

Condition to move the project to `READY`:

- already satisfied by the validated `v0.1.2` release flow and published asset verification
