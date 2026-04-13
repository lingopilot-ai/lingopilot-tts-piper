# BACKLOG.md

## 1. Current State

Current project state:

- The binary compiles with `cargo check`.
- The basic protocol starts with `ready` and returns `error` for invalid JSON.
- A minimum automated test suite exists and runs through `cargo test`.
- `language` exists in the request but does not govern synthesis.
- Voice selection uses an implicit fallback to any `.onnx.json` found in the directory.
- Piper models are cached per resolved voice for the lifetime of the process.
- `espeak-rs-sys` is pinned through `vendor/espeak-rs-sys`.
- GitHub Actions workflow definitions now exist for Windows, Linux, and macOS validation.
- Tag-driven Windows release packaging is defined through repository-owned scripts and a release workflow.
- Vendor governance is documented in `docs/vendor-espeak-rs-sys.md`.

## 2. Current Decision on the Local Vendor

### Decision

Keep `vendor/espeak-rs-sys` for now.

### Reason

The local vendor is not redundant. It contains relevant functional changes compared to upstream `espeak-rs-sys 0.1.9`:

- removes the explicit `msvcrtd` link in Windows debug builds
- publishes compiled eSpeak runtime assets to `target/<profile>/espeak-runtime`
- forces CMake reconfiguration on Windows to avoid inconsistent cache reuse across profiles/CRT modes
- adds `rerun-if-env-changed` handling for relevant build variables

Without validating an equivalent alternative, removing the vendor can break:

- Windows debug builds
- runtime availability of `espeak-ng-data` after build
- build reproducibility on Windows

### Exit Rule

The vendor may be removed only when all of the following are true:

- there is a non-vendored path with a working Windows debug build
- `espeak-runtime` is still published correctly into the output directory
- `cargo check` and release build pass without undocumented manual steps
- the reason for removal is documented in `README.md` and `AGENTS.md`

Current state: `KEEP`

## 3. Prioritization Policy

- `P0`: blocks reliability, minimum security, or the sidecar contract
- `P1`: improves performance, operations, and third-party usability
- `P2`: improves portability, release engineering, and dependency governance

An item moves to `DONE` only when:

- acceptance criteria are `PASS`
- the test plan was executed
- documentation was updated if behavior changed

## 4. P0

### P0-01. Fix the Request Contract

Objective:

- remove request fields that have no real effect or implement them properly

Minimum scope:

- decide between:
  - removing `language` from the protocol
  - or making `language` influence synthesis deterministically

Acceptance criteria:

- `PASS` if every request field has a documented functional effect
- `PASS` if `README.md`, `src/protocol.rs`, and implementation are aligned
- `FAIL` if a required field has no real effect

Test plan:

- valid request using `language`
- request without `language`, if the field stops being required
- request with invalid `language`
- compare documentation with the actual payload accepted by the binary

### P0-02. Make Voice Resolution Strict

Objective:

- prevent implicit fallback to a model different from the one requested

Minimum scope:

- remove fallback to “first `.onnx.json` found”
- return a deterministic error when the requested voice does not exist

Acceptance criteria:

- `PASS` if a missing `voice` returns `error`
- `PASS` if a directory with multiple models does not trigger implicit selection
- `FAIL` if the effective voice can differ from the requested voice without an explicit error

Test plan:

- directory with one valid voice
- directory with two voices and a request for one of them
- directory with two voices and a request for a missing voice
- directory without `.onnx.json`

### P0-03. Fix the eSpeak Lifecycle

Objective:

- remove incorrect dependence on the first `espeak_data_dir` used in the process

Minimum scope:

- define whether `espeak_data_dir` is:
  - a process-level parameter
  - or a per-request parameter that is actually supported

Acceptance criteria:

- `PASS` if `espeak_data_dir` behavior is single, explicit, and verifiable
- `PASS` if later requests do not silently inherit invalid state
- `FAIL` if the first call still determines hidden behavior for subsequent requests

Test plan:

- process started with a valid path
- process started with an invalid path
- two requests in the same instance with different paths
- process restart after initialization failure

### P0-04. Add Minimum Input Validation

Objective:

- establish the minimum operational security baseline

Minimum scope:

- validate `speed` range
- validate non-empty `text`
- validate maximum `text` length
- validate existence and type of input paths

Acceptance criteria:

- `PASS` if out-of-contract input returns `error`
- `PASS` if no invalid input causes panic or crash
- `FAIL` if invalid input can continue into synthesis or break the process

Test plan:

- `speed < min`
- `speed > max`
- `speed = NaN` or non-numeric value
- empty `text`
- `text` above limit
- missing `model_dir`
- missing `espeak_data_dir`

### P0-05. Create a Minimum Automated Test Suite

Current state: `DONE`

Objective:

- stop depending on manual testing only

Minimum scope:

- unit tests for protocol/validation
- at least one process integration test

Acceptance criteria:

- `PASS` if automated execution exists through `cargo test`
- `PASS` if there is at least:
  - a startup test
  - an invalid JSON test
  - a voice/model error test
- `FAIL` if the repository still has no project-owned tests

Test plan:

- `cargo test`
- subprocess test that reads `ready`
- subprocess test that sends invalid JSON
- subprocess test that sends an impossible request

Evidence:

- `cargo test` executes unit tests plus subprocess integration tests
- startup test: `valid_startup_flag_emits_exactly_one_ready`
- invalid JSON test: `malformed_json_returns_error_and_process_stays_alive`
- voice/model error test: `missing_voice_returns_error_before_synthesis_and_process_stays_alive`

## 5. P1

### P1-01. Cache Model and Session by Voice

Objective:

- make the sidecar actually useful as a resident process

Minimum scope:

- cache by `(model_dir, voice)` or equivalent key
- explicit reuse policy

Acceptance criteria:

- `PASS` if the second synthesis using the same voice does not reload the model from scratch
- `PASS` if cache behavior is documented
- `FAIL` if every request still recreates the model/session unnecessarily

Test plan:

- two sequential requests using the same voice
- two requests using different voices
- relative latency validation between first and second request
- memory stability validation across multiple requests

### P1-02. Define Operational Limits for the Sidecar

Objective:

- make operation predictable for third parties

Minimum scope:

- maximum text length
- maximum response size in bytes, if applicable
- clear error policy

Acceptance criteria:

- `PASS` if limits are documented and validated
- `FAIL` if operators cannot tell when a request will be accepted or rejected

Test plan:

- payload at limit
- payload above limit
- payload with Unicode characters
- multiline text serialized as a single-line JSON request

### P1-03. Document the Protocol as a Stable Contract

Objective:

- make the project reusable by other people

Minimum scope:

- document the real request/response format
- document expected errors
- document Windows behavior

Acceptance criteria:

- `PASS` if the README describes only behavior the binary actually provides
- `PASS` if at least one host example works
- `FAIL` if there is any known divergence between README and implementation

Test plan:

- run documented examples
- validate that documented fields exist in code
- validate that documented responses appear at runtime

### P1-04. Standardize Minimum Observability

Objective:

- preserve the binary stream and improve supportability

Minimum scope:

- logs only on `stderr`
- useful and stable error messages
- no binary leakage into logs

Acceptance criteria:

- `PASS` if `stdout` contains only protocol + PCM
- `PASS` if `stderr` contains logs and never binary payload
- `FAIL` if any log output pollutes `stdout`

Test plan:

- start with `warn` logging
- start with `debug` logging
- synthesize while capturing `stdout` and `stderr` separately

## 6. P2

### P2-01. Prepare a Real Platform Matrix

Current state: `IMPLEMENTED`

Objective:

- support Windows now and reduce future Linux/macOS risk

Minimum scope:

- CI on `windows-latest`
- CI on `ubuntu-latest`
- CI on `macos-latest`

Acceptance criteria:

- `PASS` if all three platforms run `cargo check`
- `PASS` if at least one protocol test runs on all three platforms
- `FAIL` if cross-platform support remains only a claim

Test plan:

- CI pipeline for `cargo check`
- startup test on each OS
- invalid JSON test on each OS

Evidence:

- `.github/workflows/ci.yml` runs `cargo check --locked` and `cargo test --locked` on `windows-latest`, `ubuntu-latest`, and `macos-latest`
- local Windows baseline executed successfully through `cargo test --locked`
- Linux/macOS execution still depends on the workflow running in GitHub Actions

### P2-02. Add Release Packaging and Distribution

Current state: `IMPLEMENTED`

Objective:

- produce downloadable release binaries for the downstream project

Minimum scope:

- build release binaries for supported platforms
- publish binaries as GitHub Release assets
- publish checksum files
- define the downstream download contract

Recommended distribution method:

- GitHub Releases assets as the default delivery path

Acceptance criteria:

- `PASS` if a release workflow or documented release process produces versioned release artifacts
- `PASS` if release artifacts are attached to a GitHub Release
- `PASS` if checksums are published with the binaries
- `PASS` if the downstream project can resolve the correct asset by version and platform
- `FAIL` if release binaries exist only locally
- `FAIL` if the downstream project has no stable way to download them

Test plan:

- build a release binary locally
- verify artifact naming for each platform
- verify checksum generation
- upload a test release asset in a non-production release or draft release
- download the published asset from its GitHub Release URL
- validate that the downloaded binary runs

Evidence:

- `.github/workflows/release.yml` publishes Windows release assets for `v<crate-version>` tags
- `scripts/Assert-ReleaseTagMatchesVersion.ps1` rejects mismatched release tags
- `scripts/Package-WindowsRelease.ps1` creates `lingopilot-tts-piper-v<version>-windows-x86_64.zip` plus `lingopilot-tts-piper-v<version>-sha256.txt`
- `scripts/Test-WindowsReleaseArchive.ps1` smoke-tests the packaged archive by starting the extracted binary against the packaged `espeak-runtime`
- `README.md` now defines the downstream download URL and asset naming contract
- local Windows validation executed successfully:
  - `.\scripts\Assert-ReleaseTagMatchesVersion.ps1 -Tag v0.1.0`
  - `.\build_windows.ps1 -Release -Locked`
  - `.\scripts\Package-WindowsRelease.ps1 -Version v0.1.0`
  - `.\scripts\Test-WindowsReleaseArchive.ps1 -ZipPath .\dist\lingopilot-tts-piper-v0.1.0-windows-x86_64.zip`
- GitHub Release upload remains pending until the first tagged workflow run

### P2-03. Formalize Dependency Policy

Objective:

- avoid random upgrades and silent regressions

Minimum scope:

- record current versions
- record upgrade conditions
- record compatibility expectations across `piper-rs`, `ort`, and the local vendor

Acceptance criteria:

- `PASS` if an explicit upgrade and rollback policy exists
- `FAIL` if upgrades still happen without a validation matrix

Test plan:

- build validation with current versions
- compatibility validation in an upgrade branch when applicable
- pre-merge dependency checklist

### P2-04. Reduce Risk Around the Local Vendor

Current state: `IMPLEMENTED`

Objective:

- keep the vendor under control instead of keeping it by inertia

Minimum scope:

- document local diff vs upstream
- define a rebase/update procedure for the vendor
- decide whether the patch should be proposed upstream

Acceptance criteria:

- `PASS` if the reason for the vendor is documented precisely
- `PASS` if there is an objective keep/remove condition
- `FAIL` if the vendor remains unmanaged

Test plan:

- reviewed diff between local and upstream
- Windows debug build with vendor
- controlled no-vendor experiment in a separate branch

Evidence:

- `docs/vendor-espeak-rs-sys.md` records the upstream baseline, patch inventory, keep/remove conditions, rebase procedure, and branch-only no-vendor experiment policy
- `README.md` links to the vendor governance document and summarizes the patch rationale
- the reviewed local delta is still concentrated in `vendor/espeak-rs-sys/build.rs`
- the local Windows test suite and release build path still pass with the vendor in place
- the vendor remains `KEEP` on `main` until the no-vendor branch experiment proves Windows debug and runtime publishing still work without undocumented steps

## 7. Dependency and Ecosystem Items

### D-01. Evaluate `ort` Upgrade

Current state:

- repository uses `ort = 2.0.0-rc.9`

Observation:

- the `ort` release line has moved beyond `rc.9`

Rule:

- do not upgrade on `main` without a dedicated compatibility branch for `piper-rs`

Acceptance criteria:

- `PASS` if an upgrade happens with functional and build validation
- `FAIL` if an upgrade happens only because a newer version exists

Test plan:

- `cargo check`
- real synthesis test
- Windows validation
- Linux/macOS validation when the platform matrix exists

### D-02. Monitor Upstream `piper-rs`

Current state:

- this repository uses `piper-rs = 0.1.9`
- upstream `main` remains on the `0.1.9` line

Rule:

- there is no urgency to upgrade for version number alone
- focus on real compatibility and sidecar needs

Acceptance criteria:

- `PASS` if any update addresses a real project need
- `FAIL` if churn happens without objective gain

Test plan:

- verify upstream `Cargo.toml`
- verify build and synthesis impact

## 8. Recommended Execution Order

Recommended order:

1. `P0-01` Fix the request contract
2. `P0-02` Make voice resolution strict
3. `P0-03` Fix the eSpeak lifecycle
4. `P0-04` Add minimum input validation
5. `P0-05` Create the minimum automated test suite
6. `P1-01` Cache model/session
7. `P1-03` Document the real protocol
8. `P1-04` Standardize observability
9. `P2-01` Prepare the platform matrix
10. `P2-02` Add release packaging and distribution
11. `P2-04` Reduce local vendor risk

## 9. Release State

Current release state:

- Windows: `PARTIAL`
- Linux: `CI CONFIGURED`
- macOS: `CI CONFIGURED`
- Local `espeak-rs-sys` vendor: `KEEP`
- Minimum security baseline: `NOT READY`
- Third-party usability: `PARTIAL`
- Open-source release readiness: `NOT READY`
- Release distribution: `AUTOMATED_PENDING_TAG_RUN`

Condition to move the project to `READY`:

- all `P0` items are `DONE`
- the minimum automated test suite is working
- README matches real behavior
- Windows validation is complete
- release artifacts can be published and downloaded by the downstream project
