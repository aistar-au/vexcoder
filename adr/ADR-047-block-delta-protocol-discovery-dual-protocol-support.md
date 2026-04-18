# ADR-047: Pivot messages/v1 to Block-Delta Default + Protocol Discovery + Dual-Protocol Support via Normalised Internal API

- **Status:** Accepted
- **Date:** 2026-04-16
- **Deciders:** Core maintainer
- **Related ADRs:** ADR-015 (local endpoint text-protocol default), ADR-003 (dual-protocol API auto-detection), ADR-029 (stream-parser completeness), ADR-043 (structured-output parser gates), ADR-046 (agent peer message channel)
- **Related tasks:** PM-05 (crate boundaries and tool calls), TASKS/PM-01-conversation-compaction, ADR-034 (multi-agent parallel task execution)

## Context

The current `messages/v1` endpoint emits full runtime envelopes. To enable small-portion SSE parsing (only the relevant delta for a given `tx_` ID), we pivot the default to Block-Delta format while preserving dual-protocol support via one canonical accumulator and two thin mappers. This amendment removes legacy backwards compatibility requirements and introduces client-side protocol discovery for a host-and-port-only configuration surface.

## Implementation Status Note

- Phase 0 foundations are merged.
- Phase 1 mapper serialisation now lives in `src/api/stream/mappers.rs`.
- `/v1/turns` now negotiates runtime-envelope, Block-Delta, or Choices-Delta SSE via `Accept`, and HTTP-level tests assert `tx_` IDs over the wire for both mapper formats.
- Live client-side discovery now uses `src/api/client/protocol_discovery.rs` for local `api_client.base_url` and local `model_url` sessions, with `explicit_protocol` remaining as the only bypass and now reusing canonical `ModelProtocol` names.
- `LocalApiTaskShared::new()` now wires a bounded peer-event channel into the accumulator, and partial tool-argument truncation emits explicit peer diagnostics instead of failing silently.

## Why Backwards Compatibility Is Not Required

1. **Controlled deployment surface.** Vexcoder is a local-first tool deployed via package managers or source builds. Users update through defined channels. There is no uncontrolled third-party API surface requiring indefinite legacy support.

2. **Early adopter expectations.** The user base consists of developers and agent builders who expect iterative evolution. Breaking changes are communicated via `CHANGELOG.md` and migration guides. The `tx_` ID scheme is a documented, intentional evolution.

3. **Memory-first architecture benefits.** ADR-038 and ADR-045 benefit from a clean protocol boundary. Legacy `call_` IDs and full-envelope parsing create unnecessary complexity in the streaming pipeline.

4. **Dual-protocol normaliser already exists.** The `CanonicalToolDeltaAccumulator` + thin mappers pattern means the server can emit either format from the same internal state. There is no runtime cost to supporting both; the question is client configuration complexity.

5. **Discovery solves the real problem.** Users do not want to remember endpoint suffixes. Protocol discovery moves the complexity to a one-time probe at connection time, caching the result for the session.

## Core Architectural Decisions

### 1. Normalised Internal API Pattern

`DeltaAccumulator` lives in `src/runtime/delta_accumulator.rs`. The API layer (`src/api/stream/`) receives a read-only `Arc<Mutex<>>` snapshot via `snapshot()`. No trait handoff; direct struct access with interior mutability. Integration with `RuntimeEnvelopeNormalizer` in `json_handoff.rs` is synchronous within the runtime event loop.

### 2. Single Source of Truth for IDs

`generate_tool_call_id()` is exported as a public free function from `src/runtime/json_handoff.rs` using `AtomicU32` + 4-hex entropy. The accumulator receives pre-generated `tx_` IDs only and never creates its own.

### 3. Generic Protocol Terminology

- **Block-Delta Format** (default for `messages/v1`): explicit `content_block_start` / `content_block_delta` (`input_json_delta` partial JSON) / `content_block_stop`.
- **Choices-Delta Format** (for `chat/completions` compatibility): index-based `choices[].delta.tool_calls[]` with partial argument strings.

### 4. Integration with Existing Peer Channel Infrastructure

The `DeltaAccumulator` exposes an optional `mpsc::Sender<PeerDeltaEvent>` for in-process cross-agent delta propagation. Each `tx_` ID is scoped to a `task_id`, enabling parallel tool execution across agents without ID collision. A facade-level bridge from `PeerDeltaEvent` to `PeerMessage` for file-backed cross-process propagation is deferred per ADR-028 and the dependency-direction gate.

### 5. Robust Memory Management for Multi-Agent Scenarios

- Bounded per-ID delta queues (`VecDeque`, capacity 32) with oldest-entry eviction on overflow
- TTL-based cleanup: `cleanup_finished(older_than)` and `cleanup_task(task_id)` for per-task removal
- Memory watermark monitoring with graceful degradation: `start_tool` returns `AccumulationError::MemoryPressure` when the map is still above watermark after eviction
- Watermark configurable via `ApiClientConfig::delta_accumulator_memory_watermark_mb` (default 256 MiB)

### 6. Client-Side Protocol Discovery

- Users configure only `base_url` (e.g., `http://127.0.0.1:8000`)
- `discover_protocol()` runs a one-time probe at connection time:
  1. Attempt `GET /v1/messages` with `Accept: application/vnd.block-delta+sse`
  2. If `200 OK` + `text/event-stream` content-type → `ModelProtocol::MessagesV1`
  3. Else attempt `GET /v1/chat/completions` with `Accept: application/vnd.choices-delta+sse`
  4. If `200 OK` + `text/event-stream` → `ModelProtocol::ChatCompat`
  5. Else return `DiscoveryError::AllProbesFailed` with probe diagnostics
- Discovery result cached for session duration
- Optional override: `explicit_protocol = "messages-v1"` or `"chat-compat"` bypasses discovery
- Probe timeout: 500 ms per attempt, 1.5 s total max

## Resolution of Debug Points

| Issue | Resolution |
|-------|-----------|
| Accumulator boundary | `DeltaAccumulator` in `src/runtime/delta_accumulator.rs`; API layer receives `Arc<Mutex<>>` snapshot |
| `tx_` ID source of truth | `generate_tool_call_id()` as `pub fn` in `json_handoff.rs`; accumulator receives pre-generated IDs |
| Thread safety | `std::sync::Mutex<HashMap<>>>`; `DashMap` rejected (single module, no cross-thread contention) |
| SSE frame ordering | `VecDeque<String>` FIFO per tool; mappers drain in insertion order |
| Config propagation | `ApiClientConfig` in `src/config.rs`; handlers read via `State<Config>` |
| Error propagation | `AccumulationError::MemoryPressure`; SSE emitter maps to `{"type":"error",...}` per spec |
| Memory lifecycle | `cleanup_finished(ttl)`, `cleanup_task(task_id)`, bounded queues, watermark enforcement |
| ADR-003 alignment | Accept-header probe is primary; explicit override available for debugging |

## Implementation Phases

### Phase 0 — Foundation (merged in PR #388)

- New `src/runtime/delta_accumulator.rs` with full lifecycle, peer events, memory management
- `RuntimeEnvelopeNormalizer` wired to accumulator; `tx_` ID format; schema updated
- `generate_tool_call_id(&AtomicU32, u16)` exported as public free function
- `AccumulationError::MemoryPressure` added; `LocalApiTaskShared::new` constructor
- `ApiClientConfig` struct in `src/config.rs` with `base_url`, `explicit_protocol`, `delta_accumulator_memory_watermark_mb`
- `src/api/client/protocol_discovery.rs` with `discover_protocol()`, `ModelProtocol`, `DiscoveryResult`, `DiscoveryError`
- Legacy `legacy_messages_protocol_value` / `legacy_chat_protocol_value` migration functions removed

### Phase 1 — Protocol Mappers

- New `src/api/stream/mappers.rs` with internal `BlockDeltaMapper` and `ChoicesDeltaMapper` implementations behind a crate-private `ProtocolMapper` trait
- Mappers are thin stateless serialisers over normalised tool-call state

### Phase 2 — Server Handler Simplification

- `/v1/turns` now selects a response mode from `Accept` and can emit legacy runtime envelopes, Block-Delta SSE, or Choices-Delta SSE from the same normalised runtime state.
- Server-side request negotiation remains intentionally lightweight: runtime-envelope stays the default/fallback for older consumers, while ADR-047 clients can opt into mapper-native wire formats without a separate handler surface.

### Phase 3 — Testing

Expanded coverage now includes HTTP-level `tx_` SSE assertions for both mapper formats plus live discovery coverage for `api_client.base_url`, local `model_url` sessions, and explicit-protocol request routing via canonical protocol names.

### Phase 4 — Schema + Docs

- `schemas/runtime_envelope_v1.json` locked to `tx_` pattern only (completed in Phase 0)
- `docs/src/configuration.md` updated with `ApiClientConfig` table and TOML example

## Consequences

- Simplified user configuration: only `host:port` required
- Faster iteration: server can evolve protocols without breaking clients
- Cleaner codebase: removed legacy ID patterns and migration shims
- Robust discovery: cached result, timeout protection, explicit override
- Memory safety: bounded queues, watermark monitoring, graceful degradation
- Peer channel integration: live delta propagation across parallel agent sessions (phase 1)

## Migration Path

1. Update `schemas/runtime_envelope_v1.json` to `tx_` pattern — **done (Phase 0)**
2. Remove legacy `call_` ID generation — **done (Phase 0)**
3. Add `protocol_discovery.rs` — **done (Phase 0)**
4. Update `ApiClient::connect` (or equivalent live connection path) to run discovery on first connect — **done for local `api_client.base_url` and local `model_url` sessions**
5. Add protocol mapper tests — **done; HTTP-level mapper assertions and client discovery tests are merged**
6. Update docs + CHANGELOG — **in progress**

This ADR supersedes all previous draft versions and captures the intended end state. The repository now contains the phase 0 foundations, mapper serialisation, live `/v1/turns` negotiated emission, and the unified local discovery cutover; remaining work is primarily documentation follow-through and broader parity coverage.
