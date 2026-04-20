# RuntimeEnvelope API SSE Normalization Plan

## Goal

Move the CLI and ratatui/crossterm-backed TUI stack onto one internal
machine-readable stream contract: `RuntimeEnvelope`. The remaining
compatibility stream paths stay only at the outbound provider edge, if they
remain at all. Internal consumers should read a single accepted event stream,
and the server SSE layer should operate as a transport wrapper around envelope
JSON rather than as a second event schema.

## Reference Basis

- ADR-025 defines the runtime JSON handoff contract and already treats
  `RuntimeEnvelope` and `RuntimeEvent` as the accepted internal stream.
- ADR-028 reserves transport logic for `src/server/**` and argues against
  secondary internal schemas leaking across boundaries.
- ADR-045 and ADR-046 keep expanding `RuntimeEvent` coverage, which makes the
  remaining block-delta and chat-chunk paths harder to justify as durable
  internal contracts.
- The current code already reflects the intended direction: `src/runtime/json_handoff.rs`
  emits accepted envelopes, while `src/local_api.rs` already treats the
  envelope stream as the reference local path.

## Current Repository Facts

- `src/runtime/json_handoff.rs` already defines the accepted contract and
  now emits explicit tool lifecycle events together with accepted metadata and
  usage updates.
- `src/server/sse.rs` now forwards accepted envelope JSON exclusively and no
  longer negotiates legacy block-delta or choices-delta modes.
- `src/runtime/backend.rs` now types `EventStream` as `Result<RuntimeEnvelope>`.
- `src/api/eventsource.rs`, `src/api/mock_client.rs`, and `src/api/stream.rs`
  now normalize provider-edge compatibility payloads into `RuntimeEnvelope`
  values immediately at the API boundary.
- `src/state/conversation/send_message.rs` now consumes accepted
  `RuntimeEvent` values directly, including runtime-owned tool-call IDs.
- `src/runtime/task_document/condenser.rs` now absorbs accepted metadata
  events for prompt-progress and timing propagation.
- The server-layer cleanup is in place, the whole-system consumer audit is
  closed, and the remaining provider-edge compatibility parser is now confined
  to focused `src/api/stream/` modules rather than a monolithic
  `src/api/stream.rs`.
- `src/runtime/json_handoff.rs` has now grown past one thousand lines, while
  the `src/api/stream.rs` extraction is in place under
  `src/api/stream/{framing,chat_compat,provider}.rs`, so the next structural
  follow-up is the runtime handoff split even where behavior stays unchanged.
- A post-PR #404 consumer-boundary hardening pass now routes structured
  tool-call arguments through `src/state/conversation/send_message.rs`,
  `src/runtime/context.rs`, and `src/runtime/update.rs`, so
  `src/app/model_update.rs` no longer reparses raw JSON tool-call deltas in
  the ratatui app layer.
- `src/local_api.rs`, `src/bin/vex/**`, `src/tui_frontend.rs`, and
  `src/batch_mode.rs` remain downstream projections of the accepted contract;
  raw block deltas are preserved only where envelope projection or renderer-
  oriented block handling still requires them.
- The deleted internal transport machinery does not appear in live source any
  longer: `TurnsSseMode`, `PendingToolBlock`, `ActiveToolBlock`, mapper
  dispatch, and `src/api/stream/mappers.rs` are gone. Provider-edge
  `api_client.explicit_protocol` and the parser-local stream dialect in
  `src/api/stream.rs` remain as ingress-only compatibility surfaces before
  normalization.

## Implemented In This Branch

- Extended the accepted contract with `RuntimeEvent::ServerMetadata` and
  `RuntimeEvent::UsageUpdated`.
- Widened `TokenUsageEnvelope` and `TurnTokens::is_zero()` so cache token
  accounting survives normalization.
- Removed `src/api/stream/mappers.rs` and rewrote the server SSE layer as a
  thin envelope passthrough.
- Switched client negotiation to plain `text/event-stream`.
- Reworked conversation, server, API, runtime-handoff, and guard tests to
  assert accepted envelope behavior and runtime-owned `tx_*` tool identifiers.
- Extracted `src/api/stream.rs` into focused `framing`, `chat_compat`, and
  `provider` modules while preserving the stable root module and immediate
  provider-edge normalization boundary.
- Validated the branch with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and
  `bash scripts/check_forbidden_names.sh`.

## Follow-up Batches

### Batch A. Repository And Wording Cleanup

Completed in PR #403. Verification guidance now stays on `cargo nextest run`,
the server-SSE replay wording remains future-facing, and the tracked notes now
match the present scope boundary and consumer inventory.

### Batch B. `src/runtime/json_handoff.rs` Structural Extraction

- Split normalization helpers, source classification, and envelope emission
  support into companion modules under `src/runtime/json_handoff/`.
- Preserve `RuntimeEnvelope` and `RuntimeEvent` as the visible contract while
  reducing the single-file maintenance burden.

### Batch C. `src/api/stream.rs` Structural Extraction

Implemented in PR #404. Provider-edge parsing, compatibility ingress
handling, and normalized envelope emission now live in focused
`src/api/stream/` modules while `src/api/stream.rs` remains the stable root
and direct envelope consumption stays easy to review.

### Batch D. Whole-System Consumer Completion

Completed in PR #403. Residual client/API-side `StreamEvent` and compatibility
`ContentBlock` parsing no longer acts as an internal consumer path, and the
CLI plus ratatui/crossterm stack remain downstream of the normalized API.

### Batch E. Resumable Replay Support

- Introduce event IDs and replay semantics when the transport is ready to
  support resumable envelope delivery.

### Batch F. CLI/TUI Consumer-Boundary Hardening

- Remove the residual ratatui-side tool-call JSON buffer from
  `src/app/model_update.rs`.
- Emit a typed runtime/UI update for tool-call argument changes so the
  terminal consumer observes structured state rather than reparsing transport
  fragments.
- Preserve raw block deltas only for envelope projection and block-oriented
  renderer duties.
- Keep tool-call accumulation at the ingress boundary. Current public streaming
  protocols still emit partial tool-argument state instead of finalized call
  objects, including responses-style
  `response.function_call_arguments.delta`, messages-style
  `input_json_delta.partial_json`, and chat-compatible incremental
  tool-argument fragments. CLI and TUI consumers must stay
  downstream of that coalescence step.

## Follow-up Lane

- Branch: `work/vexcoder-api-stream-structural-extraction`
- Purpose: carry the Batch C structural extraction after PR #403 completed the
  whole-system client/API consumer cleanup from PR #402.
- Status: consumer migration and downstream audit are complete; this lane
  narrows the remaining work to the `src/api/stream.rs` extraction and later
  replay plus runtime-handoff follow-up.
- Primary source anchors for that lane:
  - `src/api/stream.rs` is now a slim root over
    `src/api/stream/{framing,chat_compat,provider}.rs`, with the provider wire
    dialect confined to the immediate ingress adapter rather than shared
    through `vexcoder-api-types`.
  - `crates/vexcoder-api-types/src/lib.rs` still defines
    `ContentBlockCompat` at the provider-facing type layer for outbound and
    history-facing content handling.
  - `src/api/client/mod.rs` and `src/api/client/protocol_discovery.rs` still
    carry provider-boundary request-shape selection and `ContentBlock`-shaped
    request-history handling, not a second internal stream consumer.
  - `src/bin/vex/**`, `src/tui_frontend.rs`, `src/app.rs`,
    `src/app/model_update.rs`, `src/runtime/context.rs`, and
    `src/batch_mode.rs` were audited in PR #403 and remain downstream API
    consumers of the normalized contract.

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
- Keep only accepted envelope framing plus keepalive handling.
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
  normalized API contract directly, or by confining any remaining provider
  grammar to the immediate ingress adapter.

### 4. Runtime Event Parser And Tool Loop

- Replace `StreamEvent` matching in `src/state/conversation/send_message.rs`
  with direct `RuntimeEnvelope` and `RuntimeEvent` handling.
- Remove `tool_input_buffers` and the block-stop JSON reparse path.
- Remove tool lifecycle reconstruction from `ContentBlockStart`,
  `ContentBlockDelta`, and `ContentBlockStop`.
- Drive tool execution, round progression, and context enrichment from
  accepted tool events.
- Remove tagged or XML fallback parsing once no backend requires tagged tool
  output.
- Reduce `src/runtime/context.rs` and `src/runtime/update.rs` to accepted
  projections for renderer and CLI updates.

### 5. Consumer Surfaces

- Update `src/app/model_update.rs` to project transcript and tool state from
  accepted events instead of compatibility deltas.
- Remove compatibility-only state in `src/app.rs`.
- Confirm `src/batch_mode.rs` derives its output from accepted events only.
- Confirm `src/bin/vex/**` and `src/tui_frontend.rs` remain downstream API
  consumers rather than alternative stream-building layers.
- Keep `src/local_api.rs` as the reference envelope path and align the direct
  runtime path with it.

### 6. Tests And Fixtures

- Replace compatibility SSE fixtures in conversation, runtime-context, API,
  and renderer tests with envelope fixtures.
- Remove chat-style and tagged fallback scenarios once the corresponding code
  paths are no longer part of the active compatibility surface.
- Expand local API envelope contract tests because that path becomes the shared
  reference behavior.

### 7. Config And Defaults

- Remove internal streaming-mode configuration that exists only for
  block-delta and choices-delta compatibility.
- Remove tagged-fallback defaults once accepted structured tool handling is
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
- The server publishes accepted envelope JSON over SSE without legacy mode
  negotiation.
- The runtime event parser and deterministic tool loop consume explicit
  `ToolCall*` events directly.
- Compatibility parser code and compatibility stream mappers are removed from
  the internal contract path.
- The renderer, batch mode, local API, and task-document surfaces derive their
  updates from the same accepted event stream.

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
  the accepted envelope contract.