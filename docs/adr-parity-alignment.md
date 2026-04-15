# ADR: Parity Alignment with `lingopilot-tts-kokoro`

> **Status:** Proposed
> **Date:** 2026-04-14 (revised 2026-04-15)
> **Scope:** `lingopilot-tts-piper`
> **Peer ADR:** `lingopilot-tts-kokoro/docs/adr-parity-alignment.md`
> **Related:** [`AGENTS.md`](../AGENTS.md), [`BACKLOG.md`](../BACKLOG.md), [`VALIDATED_BASELINE.md`](../VALIDATED_BASELINE.md), [`docs/vendor-espeak-rs-sys.md`](vendor-espeak-rs-sys.md)

---

## 1. Context

`lingopilot-tts-piper` and `lingopilot-tts-kokoro` already share the same host-facing sidecar contract: NDJSON on `stdin`/`stdout`, PCM bytes immediately after the `audio` response, `ready` handshake at startup, `stderr`-only observability logs, and locked error prefixes (`Invalid JSON request:`, `Invalid request payload:`, `Synthesis failed:`). Everything else that differs between the two sidecars is either model-imposed (voice bundle layout, sample rate, `speed` range) or license-strategy-imposed (`espeak-ng` linkage).

This ADR enumerates the remaining parity work for this repository. Release readiness is tracked in [`BACKLOG.md §3`](../BACKLOG.md) ("Release State"), which currently reports `Open-source release readiness: NOT READY` with the remaining blockers being `P2-01` (publish first GitHub Release) and `P2-02` (execute real synthesis success validation). The parity actions below are additive: they do not supersede those backlog items, and closing them does not by itself flip the release state.

For the purposes of this ADR, host-facing protocol parity comes first. Governance, documentation, and release-tooling symmetry are still in scope, but they are secondary: they support the same sidecar-family operating model and must not redefine whether request/response parity has been achieved.

## 2. Already True Today (Verified 2026-04-15)

The following is already implemented in this repository and is **not** an action item. Regressing any of these regresses parity:

- `PIPER_TTS_LOG` is the primary log env var in [`src/main.rs:101`](../src/main.rs#L101), with `RUST_LOG` as the only fallback. The repository intentionally chose an immediate break over a legacy alias — `LINGOPILOT_TTS_LOG` does not appear anywhere in the source tree.
- Wire-level parity with the peer (field names, serde `type` tag, error prefixes, `#[serde(deny_unknown_fields)]`) is preserved in [`src/protocol.rs`](../src/protocol.rs).
- `VALIDATED_BASELINE.md` at the repository root documents the baseline that was historically tracked as `P0..P1` in `BACKLOG.md`; the backlog itself was trimmed to open work only.
- Per [`BACKLOG.md §3`](../BACKLOG.md): `Minimum security baseline: PASS` for validated startup/error-path coverage, `Windows protocol/startup/error-path validation: PASS`, `Windows packaged local artifact validation: PASS`.
- Release-adjacent scripts in `scripts/`: `Assert-ReleaseTagMatchesVersion.ps1`, `Package-WindowsRelease.ps1`, `Set-WindowsBuildEnv.ps1`, `Test-RealVoiceFixture.ps1`, `Test-WindowsReleaseArchive.ps1`, `Verify-PublishedRelease.ps1`.

## 3. Current State Anchors (Verified 2026-04-15)

The actions in §5 and §6 are written against the following observed gaps:

- [`src/protocol.rs`](../src/protocol.rs) validates `text` length and `speed` range; it does **not** reject whitespace-only `voice` or `model_dir`.
- [`src/main.rs:123`](../src/main.rs#L123) calls `std::env::set_var(ESPEAK_DATA_ENV, …)` to pass the eSpeak data path into `piper-rs`. The environment mutation has never been reviewed for alternatives.
- `scripts/` has no `Verify-Readiness.ps1` aggregator and no vendor-policy assertion script.
- `docs/` contains only this file and `vendor-espeak-rs-sys.md`; there is no `environment-variable-neutrality.md` and no `adr-production-readiness.md`.
- `README.md` does not mention `lingopilot-tts-kokoro` and has no divergence section.
- `THIRD_PARTY_LICENSES.txt` does not exist at the repository root.
- No existing test in `tests/` covers a `model_dir` path containing both whitespace and non-ASCII characters.

If any of the above changes before execution, the corresponding item must be re-checked before being marked done.

## 4. Decision

Split the parity work into two buckets. The primary bucket is host-facing contract parity: anything that keeps request/response framing, validation strictness, stable error categories, and sidecar-family operability aligned with the peer. The secondary bucket is governance and tooling symmetry: documentation, readiness scripting, and release ergonomics that reduce asymmetry with the peer without redefining the protocol contract itself. Execute §5 as work that plausibly hardens the binary or closes a license-disclosure gap. Execute §6 as documentation and tooling parity. Neither bucket promotes the repository to `READY`; that transition is governed by [`BACKLOG.md §3`](../BACKLOG.md) and depends on `P2-01` and `P2-02`.

## 5. Readiness-Relevant Actions

- [ ] **P-A01 — Reject empty `voice` and `model_dir` in `TtsRequest::validate()`.** Mirror the peer sidecar's `src/protocol.rs` behavior: whitespace-only values for either field produce `Invalid request payload: voice must not be empty or whitespace` or `… model_dir must not be empty or whitespace`. Strengthens the minimum-input-validation posture that `BACKLOG.md §3` already reports as `PASS`.
  - Done when: two new tests in `src/protocol.rs` cover both fields and `tests/request_contract.rs` asserts the error-message prefix.

- [ ] **P-A02 — Decide the fate of `std::env::set_var(ESPEAK_DATA_ENV, …)`.** The current call at [`src/main.rs:123`](../src/main.rs#L123) mutates the process environment. Investigate whether `piper-rs` accepts the eSpeak data path via a non-environment API. If yes, migrate and remove the mutation. If no, record the constraint in `docs/vendor-espeak-rs-sys.md` as a reviewed divergence from the peer pattern.
  - Done when: the investigation is resolved in code or in writing, with no silent third outcome.

- [ ] **P-A03 — Windows test for `model_dir` with space and non-ASCII characters.** Add an integration test that places `model_dir` under `C:\Users\<name with space and ação>\piper-voices\...` and asserts a successful synthesis path. Matches peer action K-A06 and closes a known `to_string_lossy()`-class regression risk.

- [ ] **P-A04 — Create `THIRD_PARTY_LICENSES.txt` and include it in the Windows zip.** File does not exist today. The binary is GPL-3.0-only because it statically links `espeak-rs-sys` (which wraps the GPL-3.0 `espeak-ng`); the disclosure must state this plainly and must also cover MIT/Apache-2.0 for `ort` and MIT for `piper-rs`. Placed here rather than in §6 because license disclosure is a compliance obligation, not a documentation nicety.
  - Done when: the file exists, `Package-WindowsRelease.ps1` copies it into the zip, and an assertion in `tests/` or `scripts/` confirms its presence in packaged output.

## 6. Governance And Documentation Parity

- [ ] **P-G01 — Create `docs/environment-variable-neutrality.md`.** Document the repository-owned environment variables currently in use (`PIPER_TTS_LOG`, `PIPER_ESPEAKNG_DATA_DIRECTORY`, `PIPER_TTS_REAL_VOICE_DIR`, `PIPER_TTS_REAL_VOICE_ID`) and the repository's naming policy. State explicitly that no `LINGOPILOT_*` alias exists and why the immediate-break was chosen over a transition alias.

- [ ] **P-G02 — Add a "Differences from `lingopilot-tts-kokoro`" section to `README.md`.** Single table with: `speed` range (`0.5–5.5` vs `0.5–2.0`), sample rate (voice-dependent, typically `22050`, vs fixed `24000`), `model_dir` layout (per-voice `.onnx` + `.onnx.json` vs shared bundle + `voices*.bin`), eSpeak linkage (static vs `libloading`), binary license (GPL-3.0-only vs Apache-2.0), eSpeak data env var (`PIPER_ESPEAKNG_DATA_DIRECTORY` vs startup-only).

- [ ] **P-G03 — Create `docs/adr-production-readiness.md`.** Use the peer's ADR structure (Interface Parity / Functional Completeness / Release & Integration gates) to restate the existing `BACKLOG.md` open items (`P2-01`, `P2-02`) and `VALIDATED_BASELINE.md` evidence as agent-checkable gates. This action does not change scope; it changes form.

- [ ] **P-G04 — Create `scripts/Verify-Readiness.ps1`.** Aggregated script with the peer's contract: exits `0` iff every evaluable gate passes. Minimum checks: `cargo check --locked`, `cargo test --locked`, and a `-Packaged` mode that runs `Test-WindowsReleaseArchive.ps1` against the newest `dist\*.zip`. It must not include a "forbidden crate" assertion — see §8. The existing `Test-RealVoiceFixture.ps1` and `Verify-PublishedRelease.ps1` remain narrow, task-specific scripts; this aggregator stands alongside them.

- [ ] **P-G05 — Cross-sidecar contract test.** In `tests/request_contract.rs`, add assertions that request field names, the serde `type` tag, and the three error-message prefixes match the peer byte-for-byte. Future drift is then caught in CI.

- [ ] **P-G06 — Cross-link the new docs from `BACKLOG.md`.** After P-G01 and P-G03 land, add a "Related Documents" block in `BACKLOG.md` pointing to the new files. Prevents the new documents from being orphaned.

## 7. Acceptance Criteria

This ADR is closed when **all** of the following are true:

1. Items P-A01..P-A04 and P-G01..P-G06 are checked.
2. `scripts\Verify-Readiness.ps1 -Packaged` exists and returns `0` on a clean Windows runner.
3. `README.md` contains the divergence section.
4. `docs/environment-variable-neutrality.md` and `docs/adr-production-readiness.md` exist and are linked from `BACKLOG.md`.

Closing this ADR does not flip `BACKLOG.md §3` to `READY`. Release-state transitions remain owned by `BACKLOG.md` and require execution of `P2-01` and `P2-02`.

## 8. Non-Goals

- **Importing the peer's `Assert-ForbiddenCargoLockCrates.ps1`.** The peer forbids `espeak-rs-sys`, `espeak-rs`, and `piper-rs` because it intends to ship an Apache-2.0 binary. This repository ships a GPL-3.0 binary that intentionally links `espeak-rs-sys`, and `piper-rs` is itself MIT, not GPL. A forbidden-crate guard with the peer's list is therefore inapplicable here, and a guard with a different list has no clear target. Vendor-patch integrity for `espeak-rs-sys` is already governed by [`AGENTS.md §10`](../AGENTS.md) and [`docs/vendor-espeak-rs-sys.md`](vendor-espeak-rs-sys.md); no additional script is added by this ADR.
- **Reintroducing a `LINGOPILOT_TTS_LOG` alias.** The repository has already completed an immediate break; resurrecting a transition alias would be regression, not parity work.
- Changing the wire protocol or functional ranges (`speed`, `sample_rate`). These are intentional divergences.
- Migrating to `libloading` for `espeak-ng`. Licensing strategy is distinct by design.
- Removing `vendor/espeak-rs-sys`.
- Declaring the repository `READY`.
- Cutting a first GitHub Release.

## 9. Consequences

**Positive.** The two sidecars gain symmetric request validation, documentation, verification tooling, and observability conventions. Reviewers and agents can operate both repositories with one mental model.

**Negative.** An aggregator (`Verify-Readiness.ps1`) is added alongside the existing narrow scripts, increasing the surface area of the `scripts/` directory. Mitigated by keeping the aggregator purely orchestrational — it calls the existing scripts rather than duplicating their logic.

**Neutral.** No wire-protocol change. Existing hosts require no recompilation.

## 10. Revision History

| Date       | Change |
|------------|--------|
| 2026-04-14 | Initial proposal. |
| 2026-04-15 | Rewrite to match current repository state: `PIPER_TTS_LOG` already implemented without legacy alias; `BACKLOG.md` restructured into Open Backlog / Dependency Watch / Release State; `VALIDATED_BASELINE.md` introduced; `Test-RealVoiceFixture.ps1` and `Verify-PublishedRelease.ps1` added to `scripts/`. Dropped the obsolete "introduce `PIPER_TTS_LOG` with legacy fallback" action. Governance items renumbered P-G01..P-G06. |
