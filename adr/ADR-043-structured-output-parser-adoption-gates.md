# ADR-043: Structured Output Parser Adoption Gates

- **Status:** Proposed
- **Date:** 2026-04-02
- **Deciders:** Core maintainer
- **Depends on:** ADR-029, ADR-030, ADR-031, ADR-041
- **Deprecates:** None
- **Deprecated by:** None

## Context

The active interactive parser path already has three production seams:

1. `src/api/stream.rs` and its companion ingress modules parse provider and
  chat-compat streaming frames.
2. `RuntimeEnvelopeNormalizer` in `src/runtime/json_handoff.rs` normalizes
  provider payloads into accepted runtime events and surfaces recoverable
  protocol errors when provider blocks are non-canonical.
3. Downstream transcript/task-state consumers already operate on the accepted
  runtime event stream; there is no separate live conversation-loop
  tool-call parser or inline-markup repair lane to preserve as a fallback.

Fullscreen transcript-first parity work does not require a general parser
replacement. The higher-priority gaps are live state visibility, footer
budgeting, overlay-based detail surfaces, and active or fallback fullscreen
convergence.

At the same time, local-model output can still arrive with malformed JSON,
partial provider deltas, or other non-canonical blocks that may justify a
broader structured parser lane in future work. That lane must remain at
provider ingress and must not displace the accepted live parser path or
reintroduce downstream compatibility repair until it proves runtime value
against the real transcript and tool-call flow.

## Decision

Track any future structured parser work behind explicit adoption gates.

The primary live parser path remains the shared ingress parser plus the runtime
normalisation boundary that emits accepted runtime events into task state.

No future structured parser implementation may become the default parser path,
and no future parser experiment may reintroduce downstream compatibility
repair, until all three adoption gates below are satisfied.

### Gate 1 — Live runtime wiring gate

At least one production runtime path must route structured-parser decisions
through the active conversation and normalisation pipeline and into accepted
runtime updates without creating parser-local UI truth.

### Gate 2 — Parity coverage gate

The test suite must prove behavioral parity against the current parser path for
structured tool calls, provider decode failures, malformed partial-JSON
handling, transcript rendering, and tool-result propagation. This gate
requires named
coverage in the active runtime and fullscreen pipeline test surfaces, not only
isolated parser unit tests.

### Gate 3 — Defect-reduction gate

Before any default cutover, captured malformed local-model fixtures or
equivalent regression tests must show a measurable reduction in transcript
loss, provider decode ambiguity, or malformed-structure recovery failures
relative to the current parser path.

## Consequences

- Fullscreen transcript-first work stays on the active task-state and renderer
  path instead of waiting on a parser replacement.
- A future structured parser lane remains available for targeted ingress-side
  malformed-output recovery, but it must earn adoption through runtime
  evidence.
- The repository can document parser follow-up work without implying that a
  broader parser is already the active runtime path.
- The framework provides optional structured-output support code, but it does
  not by itself authorize parser cutover or removal of the existing live
  parser path. Shadow validation, targeted recovery experiments, or opt-in
  local-model paths are permitted; default cutover requires all three gates.

## Validation targets

- `src/state/conversation/send_message.rs`
- `src/runtime/json_handoff.rs`
- `src/api/stream/provider.rs`
- `src/runtime/context.rs`
- `src/runtime/context/tests.rs`
- `src/api/stream/tests.rs`
- `src/app/tests/task_layout.rs`
- `src/ui/render/tests.rs`
