# RuntimeEnvelope API SSE Normalization Plan

## Goal

Move the CLI and ratatui/crossterm-backed TUI stack onto one internal
machine-readable stream contract: `RuntimeEnvelope`. The remaining
compatibility stream paths stay only at the outbound provider edge, if they
remain at all. Internal consumers should read a single canonical event stream,
and the server SSE layer should operate as a transport wrapper around envelope
JSON rather than as a second event schema.

## Reference Basis

- ADR-025 defines the runtime JSON handoff contract and already treats
  `RuntimeEnvelope` and `RuntimeEvent` as the canonical internal stream.
- ADR-028 reserves transport logic for `src/server/**` and argues against
  secondary internal schemas leaking across boundaries.
- ADR-045 and ADR-046 keep expanding `RuntimeEvent` coverage, which makes the
  remaining block-delta and chat-chunk paths harder to justify as durable
  internal contracts.
- The current code already reflects the intended direction: `src/runtime/json_handoff.rs`
  emits canonical envelopes, while `src/local_api.rs` already treats the
  envelope stream as the reference local path.

## Current Repository Facts

- `src/runtime/json_handoff.rs` already defines the canonical contract and
  now emits explicit tool lifecycle events together with canonical metadata and
  usage updates.
- `src/server/sse.rs` now forwards canonical envelope JSON exclusively and no
  longer negotiates legacy block-delta or choices-delta modes.
- `src/runtime/backend.rs` now types `EventStream` as `Result<RuntimeEnvelope>`.
- `src/api/eventsource.rs`, `src/api/mock_client.rs`, and `src/api/stream.rs`
  now normalize provider-edge compatibility payloads into `RuntimeEnvelope`
  values immediately at the API boundary.
- `src/state/conversation/send_message.rs` now consumes canonical
  `RuntimeEvent` values directly, including canonical tool-call IDs.
- `src/runtime/task_document/condenser.rs` now absorbs canonical metadata
  events for prompt-progress and timing propagation.
- The server-layer cleanup is in place, but the whole-system cleanup is still
  incomplete because `src/api/stream.rs` and provider-facing API types still
  parse compatibility `StreamEvent` and `ContentBlock` shapes before
  normalization.
- `src/runtime/json_handoff.rs` has now grown past one thousand lines and
  `src/api/stream.rs` carries a similarly dense ingress-plus-normalization
  surface, so a follow-up structural extraction batch is warranted even where
  behavior stays unchanged.
- `src/app/model_update.rs`, `src/runtime/context.rs`, `src/app.rs`,
  `src/bin/vex/**`, `src/tui_frontend.rs`, and `src/batch_mode.rs` should
  still be audited for any remaining compatibility-shaped projections that this
  branch did not need to touch.
- The deleted internal transport machinery does not appear in live source any
  longer: `TurnsSseMode`, `PendingToolBlock`, `ActiveToolBlock`, mapper
  dispatch, and `src/api/stream/mappers.rs` are gone. Provider-edge
  `ProtocolVariant::{BlockDelta, ChoicesDelta}` and `StreamEvent` remain as
  ingress-only compatibility types before normalization.

## Implemented In This Branch

- Extended the canonical contract with `RuntimeEvent::ServerMetadata` and
  `RuntimeEvent::UsageUpdated`.
- Widened `TokenUsageEnvelope` and `TurnTokens::is_zero()` so cache token
  accounting survives normalization.
- Removed `src/api/stream/mappers.rs` and rewrote the server SSE layer as a
  thin envelope passthrough.
- Switched client negotiation to plain `text/event-stream`.
- Reworked conversation, server, API, runtime-handoff, and guard tests to
  assert canonical envelope behavior and canonical `tx_*` tool identifiers.
- Validated the branch with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and
  `bash scripts/check_forbidden_names.sh`.

## Follow-up Batches

### Batch A. Repository And Wording Cleanup

- Keep non-workflow verification commands on `cargo nextest run` rather than
  parallel-count overrides.
- Refine wording around future resumable replay support in `src/server/sse.rs`
  and the tracked notes.
- Keep the branch notes aligned with the current scope boundary and consumer
  inventory.

### Batch B. `src/runtime/json_handoff.rs` Structural Extraction

- Split normalization helpers, source classification, and envelope emission
  support into companion modules under `src/runtime/json_handoff/`.
- Preserve `RuntimeEnvelope` and `RuntimeEvent` as the visible contract while
  reducing the single-file maintenance burden.

### Batch C. `src/api/stream.rs` Structural Extraction

- Split provider-edge parsing, compatibility ingress handling, and normalized
  envelope emission helpers into focused `src/api/stream/` modules.
- Keep the provider-edge adapter boundary explicit so direct envelope
  consumption remains easy to review.

### Batch D. Whole-System Consumer Completion

- Replace residual client/API-side `StreamEvent` and compatibility
  `ContentBlock` parsing wherever those layers are acting as internal API
  consumers.
- Audit the CLI and ratatui/crossterm stack so it remains a consumer of the
  normalized API rather than a parallel stream-building path.

### Batch E. Resumable Replay Support

- Introduce event IDs and replay semantics when the transport is ready to
  support resumable envelope delivery.

## Workstreams

### 1. Contract And Schema

- Keep `src/runtime/json_handoff.rs` as the only internal machine-readable
  stream contract.
- Treat `ToolCallStarted`, `ToolCallArgumentsDelta`,
  `ToolCallStatusUpdated`, `ToolCallCompleted`, and `ToolCallFailed` as the
  authoritative tool API.
- Keep transcript block events as renderer projections rather than transport
  or tool-contract semantics.
- Align `schemas/runtime_envelope_v1.json` with the envelope-only SSE contract.
- Update ADR follow-up text once the code lands so the architecture notes no
  longer imply dual internal stream dialects.

### 2. Server SSE Transport

- Remove `TurnsSseMode::BlockDelta` and `TurnsSseMode::ChoicesDelta` from
  `src/server/sse.rs`.
- Remove `PendingToolBlock`, `ActiveToolBlock`, mapper dispatch, and related
  argument re-serialization helpers.
- Remove Accept-header negotiation for legacy stream modes in
  `src/server/handlers/mod.rs`.
- Keep only canonical envelope framing plus keepalive handling.
- Rewrite server SSE tests so they assert envelope passthrough and heartbeat
  behavior instead of mode conversion.

### 3. API SSE Boundary

- Change `src/runtime/backend.rs` so `EventStream` carries
  `Result<RuntimeEnvelope>`.
- Replace compatibility parsing in `src/api/eventsource.rs` with direct
  envelope deserialization from SSE payloads.
- Update `src/api/mock_client.rs` to emit envelope fixtures directly.
- Remove provider-shaped streaming parser behavior from `src/api/stream.rs`.
- Remove `src/api/stream/mappers.rs` entirely.
- Remove inbound compatibility streaming types from
  `crates/vexcoder-api-types/src/lib.rs`, while preserving any outbound
  request/history types that still belong at the provider boundary.
- Restrict `src/api/client/mod.rs` and
  `src/api/client/protocol_discovery.rs` to request-shape concerns only.
- Finish the client/API-side migration by removing `StreamEvent` and
  compatibility `ContentBlock` parsing from layers that should now consume the
  normalized API contract directly.

### 4. Runtime Event Parser And Tool Loop

- Replace `StreamEvent` matching in `src/state/conversation/send_message.rs`
  with direct `RuntimeEnvelope` and `RuntimeEvent` handling.
- Remove `tool_input_buffers` and the block-stop JSON reparse path.
- Remove tool lifecycle reconstruction from `ContentBlockStart`,
  `ContentBlockDelta`, and `ContentBlockStop`.
- Drive tool execution, round progression, and context enrichment from
  canonical tool events.
- Remove tagged or XML fallback parsing once no backend requires tagged tool
  output.
- Reduce `src/runtime/context.rs` and `src/runtime/update.rs` to canonical
  projections for renderer and CLI updates.

### 5. Consumer Surfaces

- Update `src/app/model_update.rs` to project transcript and tool state from
  canonical events instead of compatibility deltas.
- Remove compatibility-only state in `src/app.rs`.
- Confirm `src/batch_mode.rs` derives its output from canonical events only.
- Confirm `src/bin/vex/**` and `src/tui_frontend.rs` remain downstream API
  consumers rather than alternative stream-building layers.
- Keep `src/local_api.rs` as the reference envelope path and align the direct
  runtime path with it.

### 6. Tests And Fixtures

- Replace compatibility SSE fixtures in conversation, runtime-context, API,
  and renderer tests with envelope fixtures.
- Remove chat-style and tagged fallback scenarios once the corresponding code
  paths are retired.
- Expand local API envelope contract tests because that path becomes the shared
  reference behavior.

### 7. Config And Defaults

- Remove internal streaming-mode configuration that exists only for
  block-delta and choices-delta compatibility.
- Remove tagged-fallback defaults once canonical structured tool handling is
  mandatory.
- Update documentation and ADR follow-up notes after the refactor lands.

## Files In Scope

- `src/runtime/json_handoff.rs`
- `schemas/runtime_envelope_v1.json`
- `src/server/sse.rs`
- `src/server/handlers/mod.rs`
- `src/runtime/backend.rs`
- `src/api/eventsource.rs`
- `src/api/mock_client.rs`
- `src/api/stream.rs`
- `src/api/stream/mappers.rs`
- `src/api/client/mod.rs`
- `src/api/client/protocol_discovery.rs`
- `crates/vexcoder-api-types/src/lib.rs`
- `src/state/conversation/send_message.rs`
- `src/state/conversation/streaming.rs`
- `src/state/conversation/history.rs`
- `src/state/conversation/tool_call_parser.rs`
- `src/state/conversation/tools/formatting.rs`
- `src/runtime/context.rs`
- `src/runtime/update.rs`
- `src/local_api.rs`
- `src/app/model_update.rs`
- `src/app.rs`
- `src/batch_mode.rs`
- conversation, runtime, renderer, API, and server test targets tied to the
  compatibility stream path

## Acceptance Gates

- Downstream of the API boundary, only `RuntimeEnvelope` remains as the
  machine-readable stream contract.
- The server publishes canonical envelope JSON over SSE without legacy mode
  negotiation.
- The runtime event parser and deterministic tool loop consume explicit
  `ToolCall*` events directly.
- Compatibility parser code and compatibility stream mappers are retired.
- The renderer, batch mode, local API, and task-document surfaces derive their
  updates from the same canonical event stream.

## Validation

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo nextest run`
- `bash scripts/check_forbidden_names.sh`

## Notes

- Request-shape branching may remain temporarily where direct provider
  integrations still exist, but it must terminate at the API boundary.
- This lane does not preserve compatibility-only internal streaming surfaces.
  It concentrates them at the provider edge while the internal path stays on
  the canonical envelope contract.