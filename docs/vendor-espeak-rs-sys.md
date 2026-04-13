# Vendored `espeak-rs-sys` Governance

This repository intentionally keeps a local vendor at `vendor/espeak-rs-sys`.

The vendor exists to preserve the sidecar's Windows build and runtime contract while keeping the upstream baseline traceable and reviewable.

## Upstream Baseline

- Upstream crate: `espeak-rs-sys`
- Upstream version: `0.1.9`
- Upstream source of truth: crates.io package contents for `espeak-rs-sys 0.1.9`
- Local traceability anchors:
  - `vendor/espeak-rs-sys/Cargo.toml.orig`
  - `vendor/espeak-rs-sys/Cargo.toml`
  - `vendor/espeak-rs-sys/build.rs`

Current review result:

- The functional local delta is concentrated in `vendor/espeak-rs-sys/build.rs`.
- The local package metadata still maps cleanly to the upstream `0.1.9` crate.

## Patch Inventory

The current local patch set contains four intentional behaviors beyond upstream `espeak-rs-sys 0.1.9`.

| Patch area | Local behavior | Why it exists |
|-----------|----------------|---------------|
| Windows debug CRT handling | Removes the explicit debug link to `msvcrtd` in Windows debug builds. | Avoids mixed-CRT behavior with the modern UCRT and prevents debug runtime failures when other native libraries share the process. |
| Runtime asset publishing | Copies compiled `espeak-ng-data` into `target/<profile>/espeak-runtime`. | The sidecar requires compiled eSpeak runtime assets at startup, and release packaging depends on a deterministic runtime directory. |
| Windows CMake cache behavior | Forces CMake reconfiguration on Windows builds. | Prevents stale cache reuse across profile and CRT-mode changes. |
| Build-env invalidation | Adds `rerun-if-env-changed` handling for the relevant eSpeak build variables. | Keeps Cargo rebuild behavior deterministic when environment-selected build inputs change. |

## Keep Or Remove Decision

Current decision: `KEEP`

The vendor may be removed only when all required evidence exists.

| Condition | Required evidence |
|-----------|-------------------|
| Windows debug works without the vendor | A separate no-vendor branch shows a successful Windows debug build and startup validation. |
| Runtime assets are still published | The non-vendored path still produces `target/<profile>/espeak-runtime/espeak-ng-data`. |
| Standard builds remain deterministic | `cargo check`, test execution, and Windows release packaging succeed without undocumented manual steps. |
| The removal is documented | `README.md`, `AGENTS.md`, and this document explain why the vendor was removed and what replaced it. |

If any condition is not met, the vendor stays in place on `main`.

## Review Procedure For Vendor Changes

Any change to `vendor/espeak-rs-sys` must be reviewed with this procedure:

1. Compare the local vendor against the upstream `espeak-rs-sys 0.1.9` source.
2. Confirm whether the functional delta still matches the four patch areas listed above.
3. Re-run Windows debug validation with the vendor in place.
4. Re-run the repository build/test baseline.
5. Update this document if the patch inventory, rationale, or evidence changed.

Do not merge undocumented vendor drift.

## Rebase Or Update Procedure

When rebasing or replacing the vendor:

1. Stage the work on a dedicated branch.
2. Import the candidate upstream crate source and compare it against the current vendor.
3. Re-apply only the required patch areas, with the smallest possible diff.
4. Re-run:
   - Windows debug build validation
   - `cargo check`
   - `cargo test`
   - Windows release packaging smoke test
5. Update this document, `README.md`, and `Cargo.toml` comments if the rationale or baseline changed.

## No-Vendor Experiment Policy

The no-vendor experiment is required before any removal decision, but it must happen outside `main`.

Required branch-only experiment:

1. Remove the `[patch.crates-io]` override on a dedicated branch.
2. Run a Windows debug build.
3. Verify whether `target/debug/espeak-runtime/espeak-ng-data` is still published.
4. Run `cargo check`, `cargo test`, and the packaged startup smoke test.
5. Record the result in this document or in a linked issue before deciding to keep or remove the vendor.

## Upstream Proposal Status

Current scope: repo governance only.

This repository has not yet opened an upstream issue or pull request for the local patch set. Revisit that decision only after the no-vendor experiment clarifies which patch areas are still required.
