# REF-01 RuntimeEnvelope API SSE Normalization

## Status

Implemented on `work/vexcoder-runtime-envelope-api-sse-normalization-plan`.

- The backend event-stream seam now carries `RuntimeEnvelope`.
- The provider-edge SSE parser normalizes compatibility payloads into
  canonical envelopes immediately at the API boundary.
- The server SSE path now forwards canonical envelope JSON without legacy mode
  negotiation.
- The conversation tool loop now consumes canonical `RuntimeEvent` values,
  including canonical tool-call IDs, metadata, and usage updates.
- Validation is green with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run -j 2`, and
  `bash scripts/check_forbidden_names.sh`.

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

- [x] Confirm the canonical tool lifecycle contract in
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
- [x] Convert `src/state/conversation/send_message.rs` to consume canonical
  tool events directly.
- [ ] Retire tagged or XML fallback parsing once no backend depends on it.
- [ ] Rework `src/runtime/context.rs`, `src/runtime/update.rs`, and
  `src/app/model_update.rs` into canonical projections.
- [x] Replace compatibility fixtures in conversation, runtime, API, and server
  tests with envelope-oriented assertions where this lane changed behavior.
- [ ] Remove compatibility-only config and documentation after the code path is
  retired.

## Acceptance Gates

- [x] The internal stream contract downstream of the API boundary is
  `RuntimeEnvelope` only.
- [x] The server SSE path publishes envelope JSON without legacy mode
  negotiation.
- [x] The runtime event parser and tool loop use explicit `ToolCall*` events.
- [ ] Local API, batch mode, renderer projections, and task-document updates
  all derive from the same canonical event stream.
- [x] Validation succeeds with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run -j 2`, and
  `bash scripts/check_forbidden_names.sh`.

## Follow-up

- Decide whether tagged and XML fallback parsing can now be removed outright,
  or whether a narrower migration lane is still required for local endpoint
  compatibility.
- Audit `src/runtime/context.rs`, `src/runtime/update.rs`,
  `src/app/model_update.rs`, `src/app.rs`, and `src/batch_mode.rs` for any
  remaining compatibility-shaped projections that this branch did not need to
  touch.
- Remove or rewrite compatibility-only documentation and ADR follow-up text
  once the remaining consumer cleanup is complete.

## Non-goals

- No new compatibility shim for downstream internal consumers.
- No second internal stream dialect alongside `RuntimeEnvelope`.
- No request-shape redesign beyond what is required to keep provider adapters
  functional during the transition.