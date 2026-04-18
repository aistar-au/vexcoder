# REF-01 RuntimeEnvelope API SSE Normalization

## Goal

Track the staged removal of compatibility-only internal streaming paths so the
runtime, server SSE path, CLI update surfaces, and tool loop all consume one
canonical `RuntimeEnvelope` stream.

Primary reference: `runtime-envelope-api-sse-normalization-plan.md`

## Scope Summary

- Canonical contract: `src/runtime/json_handoff.rs` and
  `schemas/runtime_envelope_v1.json`
- Server SSE transport: `src/server/sse.rs` and `src/server/handlers/mod.rs`
- API SSE boundary: `src/runtime/backend.rs`, `src/api/eventsource.rs`,
  `src/api/stream.rs`, `src/api/mock_client.rs`, `src/api/client/**`, and
  `crates/vexcoder-api-types/src/lib.rs`
- Runtime event parser and tool loop: `src/state/conversation/**`,
  `src/runtime/context.rs`, and `src/runtime/update.rs`
- Consumer surfaces: `src/app.rs`, `src/app/model_update.rs`,
  `src/batch_mode.rs`, and `src/local_api.rs`
- Tests, fixtures, config, and ADR follow-up notes tied to the stream contract

## Checklist

- [ ] Initial code-bearing batch: introduce a canonical RuntimeEnvelope SSE
  parser seam and runtime-mode round-trip coverage before the wider API
  boundary cutover.
- [ ] Confirm the canonical tool lifecycle contract in
  `src/runtime/json_handoff.rs` and the schema file.
- [ ] Change `src/runtime/backend.rs` so downstream code receives
  `RuntimeEnvelope` rather than `StreamEvent`.
- [ ] Replace compatibility SSE parsing in `src/api/eventsource.rs` and
  `src/api/stream.rs` with direct envelope parsing.
- [ ] Remove `src/api/stream/mappers.rs` and any related compatibility types in
  `crates/vexcoder-api-types/src/lib.rs`.
- [ ] Restrict `src/api/client/**` to request-shape concerns and immediate
  response normalization.
- [ ] Remove legacy mode negotiation and block-delta conversion from
  `src/server/sse.rs` and `src/server/handlers/mod.rs`.
- [ ] Convert `src/state/conversation/send_message.rs` to consume canonical
  tool events directly.
- [ ] Retire tagged or XML fallback parsing once no backend depends on it.
- [ ] Rework `src/runtime/context.rs`, `src/runtime/update.rs`, and
  `src/app/model_update.rs` into canonical projections.
- [ ] Replace compatibility fixtures in conversation, runtime, API, server, and
  renderer tests with envelope fixtures.
- [ ] Remove compatibility-only config and documentation after the code path is
  retired.

## Acceptance Gates

- [ ] The internal stream contract downstream of the API boundary is
  `RuntimeEnvelope` only.
- [ ] The server SSE path publishes envelope JSON without legacy mode
  negotiation.
- [ ] The runtime event parser and tool loop use explicit `ToolCall*` events.
- [ ] Local API, batch mode, renderer projections, and task-document updates
  all derive from the same canonical event stream.
- [ ] Validation succeeds with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run -j 2`, and
  `bash scripts/check_forbidden_names.sh`.

## Non-goals

- No new compatibility shim for downstream internal consumers.
- No second internal stream dialect alongside `RuntimeEnvelope`.
- No request-shape redesign beyond what is required to keep provider adapters
  functional during the transition.