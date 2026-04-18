# REF-01 RuntimeEnvelope API SSE Normalization

## Status

Implemented on `work/vexcoder-runtime-envelope-api-sse-normalization-plan`.

Merged in PR #402. The follow-up lane for the remaining whole-system
cleanup is `work/vexcoder-runtime-envelope-client-api-direct-consumption`.

- The backend event-stream seam now carries `RuntimeEnvelope`.
- The provider-edge SSE parser normalizes compatibility payloads into
  accepted envelopes immediately at the API boundary.
- The server SSE path now forwards accepted envelope JSON without legacy mode
  negotiation.
- The conversation tool loop now consumes accepted `RuntimeEvent` values,
  including runtime-owned tool-call IDs, metadata, and usage updates.
- The branch closes the server-layer discrepancy, but it does not yet finish
  the whole-system cleanup because the client/API ingress side still carries a
  local provider-edge compatibility parser in `src/api/stream.rs`, and the
  downstream CLI/TUI consumer audit is still open.
- Validation is green with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and
  `bash scripts/check_forbidden_names.sh`.

## Goal

Track the staged removal of compatibility-only internal streaming paths so the
runtime, server SSE path, CLI update surfaces, and tool loop all consume one
accepted `RuntimeEnvelope` stream.

Primary reference: `runtime-envelope-api-sse-normalization-plan.md`

## Scope Summary

- Accepted contract: `src/runtime/json_handoff.rs` and
  `schemas/runtime_envelope_v1.json`
- Server SSE transport: `src/server/sse.rs` and `src/server/handlers/mod.rs`
- API SSE boundary: `src/runtime/backend.rs`, `src/api/eventsource.rs`,
  `src/api/stream.rs`, `src/api/mock_client.rs`, `src/api/client/**`, and
  `crates/vexcoder-api-types/src/lib.rs`
- Runtime event parser and tool loop: `src/state/conversation/**`,
  `src/runtime/context.rs`, and `src/runtime/update.rs`
- Consumer surfaces: `src/app.rs`, `src/app/model_update.rs`,
  `src/batch_mode.rs`, `src/local_api.rs`, `src/bin/vex/**`,
  `src/tui_frontend.rs`, and the ratatui/crossterm-backed TUI stack that must
  remain a downstream API consumer rather than a second internal stream core
- Tests, fixtures, config, and ADR follow-up notes tied to the stream contract

## Checklist

- [x] Confirm the accepted tool lifecycle contract in
  `src/runtime/json_handoff.rs` and extend it where required for metadata and
  usage propagation.
- [x] Change `src/runtime/backend.rs` so downstream code receives
  `RuntimeEnvelope` rather than `StreamEvent`.
- [x] Normalize compatibility SSE payloads into envelopes at the API boundary
  in `src/api/eventsource.rs`, `src/api/mock_client.rs`, and `src/api/stream.rs`.
- [x] Remove `src/api/stream/mappers.rs` and preserve only provider-edge types
  still required before normalization.
- [x] Restrict `src/api/client/**` to request-shape concerns and immediate
  response normalization.
- [x] Remove legacy mode negotiation and block-delta conversion from
  `src/server/sse.rs` and `src/server/handlers/mod.rs`.
- [x] Convert `src/state/conversation/send_message.rs` to consume accepted
  tool events directly.
- [x] Replace residual client/API-side `StreamEvent` and `ContentBlock`
  parsing as an internal consumer path with direct `RuntimeEnvelope`
  consumption wherever the normalized API contract should be observed, while
  keeping any unavoidable provider-edge grammar confined to the immediate
  ingress adapter.
- [ ] Split `src/runtime/json_handoff.rs` into focused companion modules once
  the accepted contract changes settle, so the file no longer concentrates
  normalization, source classification, and envelope emission in one unit.
- [ ] Split `src/api/stream.rs` into focused provider-ingress and
  normalization helpers so the compatibility ingress path remains readable
  while the normalized API seam stays explicit.
- [x] Audit the CLI and ratatui/crossterm consumer stack so it projects the
  normalized API contract rather than rebuilding stream semantics behind it.
- [ ] Remove tagged or XML fallback parsing once no backend depends on it.
- [x] Confirm `src/runtime/context.rs`, `src/runtime/update.rs`, and
  `src/app/model_update.rs` already operate as accepted projections; keep only
  naming cleanup where compatibility-era wording remained.
- [x] Replace compatibility fixtures in conversation, runtime, API, and server
  tests with envelope-oriented assertions where this lane changed behavior.
- [ ] Remove compatibility-only config and documentation after the code path is
  fully replaced.

## Acceptance Gates

- [x] The internal stream contract downstream of the API boundary is
  `RuntimeEnvelope` only.
- [x] The server SSE path publishes accepted envelope JSON without legacy mode
  negotiation.
- [x] The runtime event parser and tool loop use explicit `ToolCall*` events.
- [x] Local API, batch mode, CLI/TUI consumer projections, renderer
  projections, and task-document updates all derive from the same accepted
  event stream.
- [x] Validation succeeds with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and
  `bash scripts/check_forbidden_names.sh`.

## Follow-up

- Batch A: repository and wording cleanup.
  Normalize non-workflow verification commands to `cargo nextest run`, refine
  the `src/server/sse.rs` replay wording so it stays future-facing rather than
  returning to "until resumable replay is implemented", and keep the tracked
  notes aligned with the present branch boundary.
- Batch B: `src/runtime/json_handoff.rs` structural extraction.
  Move event-source classification, normalization helpers, and envelope
  emission support into companion modules under `src/runtime/json_handoff/`
  so the accepted contract remains readable as it continues to grow.
- Batch C: `src/api/stream.rs` structural extraction.
  Move provider-edge parsing, compatibility ingress handling, and normalized
  envelope emission helpers into focused `src/api/stream/` modules.
- Batch D: client and API direct-envelope completion.
  Complete the client/API-side migration from residual `StreamEvent` and
  `ContentBlock` parsing to direct `RuntimeEnvelope` consumption wherever the
  layer is acting as an internal API consumer.
- Batch E: resumable replay support.
  Introduce event IDs and replay semantics when the server transport is ready
  to support resumable envelope delivery rather than only keepalive framing.
- Decide whether tagged and XML fallback parsing can now be removed outright,
  or whether a narrower migration lane is still required for local endpoint
  compatibility.
- Audit `src/runtime/context.rs`, `src/runtime/update.rs`,
  `src/app/model_update.rs`, `src/app.rs`, `src/batch_mode.rs`,
  `src/bin/vex/**`, and `src/tui_frontend.rs` for any remaining
  compatibility-shaped projections that this branch did not need to touch.
- Distinguish removed internal duplication from residual ingress-only
  compatibility: `TurnsSseMode`, mapper dispatch, `PendingToolBlock`,
  `ActiveToolBlock`, and `src/api/stream/mappers.rs` are gone, while
  `api_client.explicit_protocol` and the parser-local provider stream dialect
  in `src/api/stream.rs` remain only at the provider/config edge before
  immediate normalization.
- Complete the whole-system cleanup by migrating client/API-side
  `StreamEvent` and `ContentBlock` parsing to direct `RuntimeEnvelope`
  consumption wherever those layers are acting as API consumers rather than as
  provider-edge adapters.
- Follow-up lane: isolate the remaining client/API-side migration on a
  dedicated branch so the merged server cleanup remains stable while the
  ingress, API-type, and consumer-audit work proceeds in narrower batches.
- Remove or rewrite compatibility-only documentation and ADR follow-up text
  once the remaining consumer cleanup is complete.

## Non-goals

- No new compatibility shim for downstream internal consumers.
- No second internal stream dialect alongside `RuntimeEnvelope`.
- No request-shape redesign beyond what is required to keep provider adapters
  functional during the transition.