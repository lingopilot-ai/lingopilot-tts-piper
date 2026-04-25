# ADR: Health-check `op:ping` / `op:pong`

> **Status:** Accepted
> **Date:** 2026-04-25
> **Scope:** `lingopilot-tts-piper`
> **Peer ADR:** [`lingopilot-tts-kokoro/docs/adr-health-ping.md`](../../lingopilot-tts-kokoro/docs/adr-health-ping.md)
> **Related:** [`src/protocol.rs`](../src/protocol.rs), [`src/main.rs`](../src/main.rs), [`README.md`](../README.md), [`AGENTS.md`](../AGENTS.md)

---

## 1. Context

Hosts that supervise sidecar processes need a liveness probe. Today this repo exposes only the one-shot `ready` line at startup; there is no way to ask "are you still reading stdin?" mid-session. The host kit (`lingopilot`) wants a uniform `HealthStrategy::Ping` across both TTS sidecars and the Whisper sidecar, but the three protocols use different framing and different request shapes. This ADR fixes the TTS side (`piper` here, `kokoro` in the peer ADR) to a single, additive contract so the host's health strategy is a parameter of (framing, payload) rather than a per-sidecar code path.

## 2. Decision

Add a base-protocol health-check op pair — **`op:ping`** request, **`op:pong`** response — under the existing `op`-tagged NDJSON contract. Additive (no breaking change), mandatory at protocol version ≥ `0.1.7`, **not** advertised in `SUPPORTED_OPS`.

The peer sidecar (`kokoro`) adopts the identical wire format on the same date; the only divergence permitted between the two is the version number that gates availability.

## 3. Wire format (locked)

**Request (host → sidecar):**

```json
{"op":"ping","id":"<correlation-id>"}
```

- `id`: required, non-empty, ≤ 128 bytes (same rules as other ops).
- No other fields. `#[serde(deny_unknown_fields)]` applies.

**Response (sidecar → host) — success:**

```json
{"op":"pong","id":"<correlation-id>"}
```

- `id` echoes the request `id` byte-for-byte.
- No other fields. Reserved for additive extension under a separate ADR.

**Response — invalid `id`:** existing `error` path with `kind="bad_request"`. No new error kind.

## 4. Implementation plan

### 4.1 Files to change

| File | Change |
|---|---|
| [`src/protocol.rs`](../src/protocol.rs) | Add `SidecarRequest::Ping(PingRequest { id })` variant tagged `"ping"` with `deny_unknown_fields`; add `SidecarResponse::Pong { id: &'a str }` tagged `"pong"`. `validate()` checks only `id` (reuse `validate_id`). |
| [`src/main.rs`](../src/main.rs) | In the stdin loop, match `SidecarRequest::Ping` **before** `handle_request` and reply directly via `send_response(&SidecarResponse::Pong { id })`. Do not touch `synthesis_cache`. Emit `tracing::trace!` (not `debug` / `info`) for the request. |
| [`Cargo.toml`](../Cargo.toml) | Bump `version` from `0.1.6` to `0.1.7`. |
| [`src/protocol.rs`](../src/protocol.rs) (`SUPPORTED_OPS`) | **Unchanged.** Ping is base-protocol, not a negotiated capability. |
| [`README.md`](../README.md) | Add a "Health check" subsection under request/response framing documenting the locked shape and the `0.1.7` floor. Also add `ping` row to the request-schema table. |
| [`AGENTS.md`](../AGENTS.md) | Add one bullet under the protocol section: ping is mandatory from `0.1.7` onward, must not appear in `SUPPORTED_OPS`, must bypass the synthesis worker. |
| [`VALIDATED_BASELINE.md`](../VALIDATED_BASELINE.md) | New row recording that `op:ping`/`op:pong` is part of the validated wire surface from `0.1.7`. |

> **Note on peer asymmetry:** the peer (`kokoro`) does not maintain a `VALIDATED_BASELINE.md` and records wire-surface acceptance via the test suite plus the ADR §8 checklist. This is a deliberate documentation asymmetry, not a parity regression — the wire format itself remains byte-for-byte identical.

### 4.2 Tests (acceptance)

Add to [`src/protocol.rs`](../src/protocol.rs) `tests` module:

- `ping_request_deserializes` — round-trip of `{"op":"ping","id":"h1"}`.
- `ping_request_rejects_extra_fields` — payload with extra field fails (`deny_unknown_fields` invariant).
- `ping_request_rejects_empty_id` — `validate()` returns `Err`.
- `ping_request_rejects_oversize_id` — `id` of 129 bytes fails.
- `pong_response_serializes` — `{"op":"pong","id":"h1"}` exact-string match.

Add to [`src/main.rs`](../src/main.rs) `tests` module:

- `parse_request_accepts_ping` — `parse_request(r#"{"op":"ping","id":"h1"}"#)` returns `Ok(SidecarRequest::Ping(_))`.

Add an integration test in `tests/` (new file `ping_contract.rs`):

- Spawn the binary, send `{"op":"ping","id":"h1"}` after `ready`, assert next stdout line is exactly `{"op":"pong","id":"h1"}`.
- Send a ping **between** two synthesize requests (after the first request's `done`, before the second's `synthesize` — never between `audio` and `done` of the same synthesis) and assert the pong lands as a clean NDJSON line without disturbing PCM framing of either synthesis.

### 4.3 Observability

- `tracing::trace!(event = "ping", id = …)` on receipt.
- No `info` / `warn` log for the happy path; pings are frequent.
- Existing `request_rejected` log already covers the invalid-id case.

### 4.4 Out of scope (do not bundle)

- Migrating the discriminator name (`op` → `kind`). Tracked separately if ever needed.
- Adding metrics / uptime / queue depth to the pong payload.
- Cancellation, batching, or any other op.
- Changes to the audio framing contract.

## 5. Why this shape

| Decision | Reason |
|---|---|
| New op under existing `op` tag | Additive; no host parser changes for non-ping flows. |
| `ping` not in `SUPPORTED_OPS` | Health is base contract, not negotiated capability. Hosts gate on `ready.version ≥ 0.1.7`. |
| `id` required, echoed | Preserves the per-message correlation invariant the rest of the protocol already depends on. |
| Bypass synthesis worker | Health must answer "process alive and reading stdin", not "worker idle". A ping queued behind a long synthesis defeats its purpose. |
| Empty pong payload | Liveness is binary. Metrics belong on a separate op or stderr/tracing. |
| Identical contract on peer (`kokoro`) | Lets the host kit treat TTS as one family with one health payload, parameterized only by which binary is running. |

### 5.1 Ordering invariant (ping vs. PCM)

The stdout stream is **not** pure NDJSON during a synthesis: between the `audio` JSON line and the `done` JSON line, [`src/main.rs:240-262`](../src/main.rs#L240-L262) writes raw little-endian PCM16 bytes. Inserting a `pong` line into that window would corrupt the audio framing on the host side.

**Invariant:** `pong` MUST NOT be emitted while a synthesis has emitted `audio` but not yet emitted `done`. Today this is structurally guaranteed because the stdin dispatch loop in [`src/main.rs:133-160`](../src/main.rs#L133-L160) is single-threaded and serial — the next request is not read until the current synthesis finishes. Any future migration to concurrent dispatch (worker pool, async stdin reader, parallel synthesis) MUST preserve this ordering invariant explicitly, e.g. by gating pong emission on a "synthesis-in-flight" flag or by serializing all stdout writes through a single writer that holds back pongs until PCM completes. This invariant is load-bearing for the wire contract and any change to it is a parity-breaking event under [`docs/adr-parity-alignment.md`](adr-parity-alignment.md).

## 6. Rollout

1. Land the change in this repo as `0.1.7`.
2. Land the mirror change in `lingopilot-tts-kokoro` under the same wire contract (peer ADR tracks its own version bump).
3. Host kit (`lingopilot`) consumes `HealthStrategy::Ping` once **both** sidecars are at the floor version. Until then the host falls back to whatever liveness check it already has (typically: process-alive only).
4. No deprecation window required — this is purely additive.

## 7. Acceptance for closing this ADR

- [ ] [`src/protocol.rs`](../src/protocol.rs) carries `Ping` request and `Pong` response variants with the test coverage from §4.2.
- [ ] [`src/main.rs`](../src/main.rs) dispatches ping before the synthesis path.
- [ ] [`Cargo.toml`](../Cargo.toml) at `0.1.7`.
- [ ] [`README.md`](../README.md), [`AGENTS.md`](../AGENTS.md), [`VALIDATED_BASELINE.md`](../VALIDATED_BASELINE.md) updated.
- [ ] Integration test in `tests/ping_contract.rs` green on Windows.
- [ ] Peer ADR in `lingopilot-tts-kokoro` reaches the same acceptance state; both repos cut their version bump in coordination.
