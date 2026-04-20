# REF-01 RuntimeEnvelope API SSE Normalization

## Status

Implemented on `work/vexcoder-runtime-envelope-api-sse-normalization-plan`.

Merged in PR #402. The whole-system consumer cleanup follow-up landed on
`work/vexcoder-runtime-envelope-client-api-direct-consumption` in PR #403,
and the active structural-extraction follow-up is
`work/vexcoder-api-stream-structural-extraction` in PR #404. The current
consumer-boundary hardening follow-up is
`work/vexcoder-cli-tui-envelope-consumer-audit`.

- The backend event-stream seam now carries `RuntimeEnvelope`.
- The provider-edge SSE parser normalizes compatibility payloads into
  accepted envelopes immediately at the API boundary.
- Same-machine local endpoints that accept `stream = true` but do not emit an
  initial SSE event now fall back once to `stream = false`, and the full JSON
  response is normalized through the same API boundary rather than opening a
  second downstream parse path.
- The server SSE path now forwards accepted envelope JSON without legacy mode
  negotiation.
- The conversation tool loop now consumes accepted `RuntimeEvent` values,
  including runtime-owned tool-call IDs, metadata, and usage updates.
- The merged follow-up lane closes the whole-system consumer cleanup from
  PR #402. Tagged/XML fallback parsing is now gone, legacy thinking-tag
  rewrites no longer survive behind the API boundary, and the governing
  ADR/task/docs set now states that provider ingress is the only
  normalization seam.
- Validation is green with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and
  `bash scripts/check_forbidden_names.sh`.

## Goal

Track the staged removal of compatibility-only internal streaming paths so the
runtime, server SSE path, CLI update surfaces, and tool loop all consume one
accepted `RuntimeEnvelope` stream.

This lane exists to prevent the same architectural regression from recurring:
once provider ingress and downstream runtime code both repair legacy dialects,
the repository is again maintaining two semantic assembly paths for one
accepted contract.

Primary reference: `adr/ADR-047-amendment-2026-04-20.md`

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
- [x] Split `src/api/stream.rs` into focused provider-ingress and
  normalization helpers so the compatibility ingress path remains readable
  while the normalized API seam stays explicit.
- [x] Audit the CLI and ratatui/crossterm consumer stack so it projects the
  normalized API contract rather than rebuilding stream semantics behind it.
- [x] Replace the remaining ratatui-side tool-call argument reassembly in
  `src/state/conversation/send_message.rs`, `src/runtime/context.rs`,
  `src/runtime/update.rs`, and `src/app/model_update.rs` with runtime-owned
  typed updates, while preserving raw block deltas for renderer and envelope
  projection duties.
- [x] Remove tagged or XML fallback parsing and the remaining legacy
  thinking-tag compatibility rewrite once canonical structured blocks are the
  only accepted API shape.
- [x] Replace compatibility fixtures in conversation, runtime, API, and server
  tests with envelope-oriented assertions where this lane changed behavior.
- [x] Surface provider block-decode failures as recoverable API-boundary
  errors rather than silently normalizing them downstream.
- [x] Remove or rewrite compatibility-only config and documentation after the
  code path is fully replaced.

## Acceptance Gates

- [x] The internal stream contract downstream of the API boundary is
  `RuntimeEnvelope` only.
- [x] The server SSE path publishes accepted envelope JSON without legacy mode
  negotiation.
- [x] The runtime event parser and tool loop use explicit `ToolCall*` events.
- [x] Local API, batch mode, CLI/TUI consumer projections, renderer
  projections, and task-document updates all derive from the same accepted
  event stream.
- [x] Non-canonical provider block tags are rejected or surfaced as
  recoverable API-boundary errors rather than rewritten into accepted runtime
  content.
- [x] Validation succeeds with `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and
  `bash scripts/check_forbidden_names.sh`.

## Follow-up

- Batch A: repository and wording cleanup.
  Completed in PR #403: non-workflow verification commands now standardize on
  `cargo nextest run`, the `src/server/sse.rs` replay wording stays
  future-facing, and the tracked notes now match the present branch boundary.
- Batch B: `src/runtime/json_handoff.rs` structural extraction.
  Move event-source classification, normalization helpers, and envelope
  emission support into companion modules under `src/runtime/json_handoff/`
  so the accepted contract remains readable as it continues to grow.
- Batch C: `src/api/stream.rs` structural extraction.
  Implemented in PR #404: provider-edge parsing, compatibility ingress
  handling, and normalized envelope emission now live in focused
  `src/api/stream/` modules while `src/api/stream.rs` remains the stable
  public root.
- Batch D: client and API direct-envelope completion.
  Completed in PR #403: residual `StreamEvent` and `ContentBlock` parsing no
  longer remains as an internal consumer path; the remaining provider grammar
  is confined to the immediate ingress adapter.
- Batch E: resumable replay support.
  Introduce event IDs and replay semantics when the server transport is ready
  to support resumable envelope delivery rather than only keepalive framing.
- Batch F: CLI/TUI consumer-boundary hardening.
  The current follow-up branch removes the residual ratatui-side
  `streaming_tool_input_buffers` path, emits typed tool-call argument updates
  from the runtime/conversation layer, and retains raw block deltas only where
  envelope projection or block-oriented rendering still requires them.
- Completed in PR #408: tagged/XML fallback parsing and legacy thinking-tag
  rewrites were removed because preserving them beside API-level
  normalization recreated a second semantic repair path for the same runtime
  contract.
- Distinguish removed internal duplication from residual ingress-only
  compatibility: `TurnsSseMode`, mapper dispatch, `PendingToolBlock`,
  `ActiveToolBlock`, `src/api/stream/mappers.rs`,
  `src/state/conversation/tool_call_parser.rs`, tagged/XML fallback parsing,
  and legacy thinking-tag rewrites are gone, while
  `api_client.explicit_protocol` and the parser-local provider stream dialect
  in `src/api/stream/{framing,chat_compat,provider}.rs` remain only at the
  provider/config edge before immediate normalization.
- Keep future ADR/task/docs updates aligned with the API-boundary rule: once a
  canonical schema exists, downstream runtime code must not reintroduce a
  compatibility rewrite for the same semantic job.

## Non-goals

- No new compatibility shim for downstream internal consumers.
- No second internal stream dialect alongside `RuntimeEnvelope`.
- No request-shape redesign beyond what is required to keep provider adapters
  functional during the upgrade.
