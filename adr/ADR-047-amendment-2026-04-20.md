# ADR-047 Amendment: RuntimeEnvelope API Normalization and Consumer Boundary (2026-04-20)

**Date:** 2026-04-20
**Status:** Amended
**Amends:** ADR-047, ADR-047-amendment-2026-04-16
**Related:** ADR-025, ADR-028, ADR-045, ADR-046

## Context

The repository has been carrying `runtime-envelope-api-sse-normalization-plan.md`
at the root as a working note for the runtime-envelope cutover. That material
now records accepted architecture rather than a temporary branch plan.

The merged tree already reflects the intended boundary:

- `src/runtime/json_handoff.rs` emits the accepted internal stream contract.
- `src/server/sse.rs` forwards accepted envelope JSON rather than translating
  between internal stream schemas.
- `src/runtime/backend.rs` and downstream consumers read `RuntimeEnvelope`
  directly.
- Provider-facing compatibility parsing is confined to
  `src/api/stream/{framing,chat_compat,provider}.rs` and the immediate ingress
  adapter.
- The CLI, ratatui task surface, batch mode, local API, and task-document
  condenser consume accepted runtime events rather than carrying a second
  internal stream dialect.

Leaving those facts in a root-level plan note makes the repository harder to
read: the user guide should stay focused on building and running the binary,
while accepted design history belongs beside the rest of the ADR corpus.

## Decision

### 1. Record the normalized envelope boundary as part of ADR-047

The accepted architecture is:

- downstream of the API boundary, `RuntimeEnvelope` / `RuntimeEvent` are the
  only machine-readable internal stream contract
- server SSE is a transport wrapper over accepted envelope JSON, not a second
  internal event schema
- provider compatibility grammars remain confined to immediate ingress parsing
  and request-shape code at the provider edge
- transcript rendering, tool-call state, batch output, and task-document
  updates are downstream projections of accepted runtime events rather than
  independent stream-building layers

### 2. The merged implementation status is part of the design record

PR #402 established the envelope-only server/runtime path, PR #403 closed the
whole-system consumer cleanup, and PR #404 completed the `src/api/stream.rs`
structural extraction. The repository now treats the following as merged
boundary facts:

- explicit tool lifecycle, metadata, and usage events are emitted on the
  accepted envelope path
- backend event streams and downstream consumers read envelopes directly
- legacy server-side block-delta and choices-delta conversion is removed
- client/API normalization happens immediately at ingress
- local API, batch mode, CLI/TUI, and task-document consumers observe the same
  accepted event surface

### 3. Remaining work is structural follow-through, not boundary ambiguity

Remaining follow-up is limited to:

- splitting `src/runtime/json_handoff.rs` into companion modules as the
  accepted contract grows
- documenting and maintaining the same-machine local fallback from stalled
  SSE startup to one `stream = false` retry through the accepted normalizer
- adding resumable replay semantics when the transport layer is ready for
  stable event IDs and replay tokens

These follow-ups do not reopen the internal stream contract or restore a
second schema downstream of API normalization.

## Consequences

- The repository no longer keeps a root-level plan note for an accepted
  architecture boundary.
- Future documentation can refer to ADR-047 and this amendment instead of a
  standalone planning document.
- The build-first user guide and the architecture history remain separate,
  which makes the onboarding path easier to scan without losing the design
  record.
