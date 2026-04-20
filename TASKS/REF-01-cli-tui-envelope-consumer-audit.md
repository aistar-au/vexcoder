# REF-01 CLI/TUI Envelope Consumer Audit

## Status

Active on `work/vexcoder-cli-tui-envelope-consumer-audit`.

This branch closes the remaining consumer-boundary seam discovered after
PR #404: the ratatui/crossterm-backed TUI still buffered and reparsed raw
tool-call argument JSON in `src/app/model_update.rs`. That behavior kept a
second semantic assembly path behind the normalized API boundary even though
`RuntimeEnvelope` already served as the accepted internal stream contract.

The same branch now closes the residual compatibility-repair seam that had
survived behind that boundary: tagged/XML fallback parsing and the remaining
legacy thinking-tag rewrite have been removed from the runtime normalization
path so provider ingress must emit canonical structured blocks or surface a
recoverable decode error.

## Objective

Keep the CLI and ratatui/crossterm stack as downstream consumers of runtime-
owned structured updates. Tool-call argument assembly should occur in the
runtime/conversation layer, while raw block deltas remain available only where
they still serve renderer or envelope-projection duties.

## External Reference Basis

- `ratatui` application-pattern guidance defines a `Model` / `Update` /
  `View` split in which the view renders from application state rather than
  reconstructing domain semantics from presentation fragments.
- `ratatui` event-handling guidance treats centralized event capture plus
  message passing as the scalable boundary when state changes must be routed to
  multiple consumers.
- `crossterm` documents display manipulation, event capture, and TTY
  detection as transport and device-control concerns rather than domain-state
  ownership.
- Tokio `mpsc` documents typed multi-producer, single-consumer channels as the
  runtime-agnostic message boundary for communication between tasks and across
  sync/async seams.
- ECMA-48 defines display control functions in terms of effects on a
  character-imaging input/output device, which supports treating display
  control as a presentation transport layer rather than an application-domain
  protocol.

## Scope

- `src/state/conversation/send_message.rs`
- `src/state/conversation/state.rs`
- `src/runtime/context.rs`
- `src/runtime/update.rs`
- `src/app.rs`
- `src/app/model_update.rs`
- `src/app/turn.rs`
- `src/local_api.rs`
- `src/batch_mode.rs`
- focused tests covering conversation emission, runtime forwarding, and TUI
  projection

## Checklist

- [x] Add a typed `ConversationStreamUpdate::ToolCallArgumentsUpdated` event
  for structured tool-call argument propagation.
- [x] Add the matching `UiUpdate::ToolCallArgumentsUpdated` projection so the
  runtime, CLI, and TUI share the same typed boundary.
- [x] Move tool-call argument assembly into
  `src/state/conversation/send_message.rs` and update the stored
  `ContentBlock::ToolUse` input there when the accumulated JSON becomes
  parseable.
- [x] Remove the ratatui-side `streaming_tool_input_buffers` path from
  `src/app.rs`, `src/app/turn.rs`, and `src/app/model_update.rs`.
- [x] Keep raw `StreamBlockDelta` updates available for local envelope
  projection and renderer-oriented block handling.
- [x] Accept both streamed JSON-string argument deltas and fully materialized
  JSON argument values at the chat-compatible API boundary, then normalize them
  to the same typed runtime tool-call updates before the CLI/TUI consumer
  layer.
- [x] Remove tagged/XML fallback parsing and the remaining legacy thinking-tag
  compatibility rewrite so no downstream compatibility parser remains behind
  the accepted API boundary.
- [x] Update focused tests so they assert the typed downstream update path
  directly.
- [x] Run an end-to-end local-server exercise against `http://localhost:8000`
  so the branch proves the patched ingress path under a real runtime, not only
  under unit and integration tests.
- [ ] Re-run full repository validation after the documentation and roadmap
  updates settle.

## Acceptance Criteria

- [x] The ratatui/crossterm TUI no longer reparses raw tool-call JSON deltas in
  the app layer.
- [x] Structured tool-call argument updates originate in the runtime /
  conversation layer and flow through typed runtime update messages.
- [x] Raw block deltas still remain available where envelope projection or
  block-oriented rendering requires them.
- [x] No downstream runtime or UI consumer rewrites legacy provider
  tool/thinking tags into accepted runtime shapes.
- [x] Focused tests cover conversation emission, runtime forwarding, and TUI
  projection of the typed update.

## Notes

- This branch removes tagged/XML fallback parsing and the last legacy
  thinking-tag rewrite behind `RuntimeEnvelope` normalization. The hard cutover
  exists to prevent the repository from maintaining two semantic assembly
  paths for the same tool-call and transcript job.
- The same boundary now tolerates the two local chat-compatible argument
  encodings seen in practice: streamed JSON fragments and fully materialized
  JSON values. Both are converted to one typed runtime contract before the
  consumer surface receives them.
- Official local-runtime API docs also advertise both streaming and
  non-streaming response modes, so keeping that retry inside API ingress is an
  interoperability measure rather than a second consumer-side protocol path.
- Live validation against `http://127.0.0.1:8000` showed a second local-server
  variation: `stream = true` requests for both `/v1/messages` and
  `/v1/chat/completions` could complete only after the client retried with
  `stream = false`. The retry now stays inside the API boundary and reuses the
  same runtime-envelope normalizer rather than exposing a second downstream
  parse path.
- `src/local_api.rs` still relies on raw block deltas only where envelope
  projection requires them; that projection is not a second semantic parser.
