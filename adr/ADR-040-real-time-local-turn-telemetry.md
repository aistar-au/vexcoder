# ADR-040: Real-Time Local Turn Telemetry

- **Status:** Proposed
- **Date:** 2026-03-31
- **Deciders:** Core maintainer
- **Depends on:** ADR-030, ADR-031, ADR-038, ADR-039
- **Supersedes:** None
- **Superseded by:** None

## Context

Local chat-compatible inference servers can spend substantial time in prompt
evaluation before the first text token arrives. On `llama.cpp`, that prompt
phase can already emit useful stream metadata such as:

1. `prompt_progress` counters while prompt tokens are still being ingested.
2. `timings` snapshots that distinguish prompt-eval time from generation time.

The operator-facing problem was not one bug but one broken feedback loop:

1. The local chat-compatible request payload did not ask for progress frames,
   so some servers never emitted the incremental prompt-eval metadata at all.
2. When metadata-only chunks were emitted, the stream conversion path treated
   them as empty content because `content` was `null`.
3. The TUI therefore had no transport-level signal to show during prompt
   evaluation beyond a client-side elapsed timer.
4. That made long local turns look indistinguishable from a stalled client,
   even when the backend was actively evaluating prompt tokens.

This is especially visible on local CPU-bound runs with large system prompts,
where the backend can be healthy and progressing while no visible text token
has been produced yet.

## Decision

Treat local prompt-eval telemetry as first-class turn state.

### Scope boundaries

1. This ADR applies to local chat-compatible streaming requests and their
   operator-facing transcript/status surfaces.
2. This ADR does **not** change canonical persisted task-state machine fields.
3. This ADR does **not** add provider-specific telemetry to remote API-server
   payloads by default.
4. This ADR does **not** replace the ADR-039 canonical waiting phrase.

### Request contract

5. Local chat-compatible streaming requests must ask for prompt progress and
   per-token timing metadata when the backend supports it.
6. The request contract for that local path includes `return_progress = true`
   and `timings_per_token = true`.
7. Remote API-server payloads remain unchanged unless a separate ADR expands
   provider-specific telemetry on those paths.

### Stream conversion contract

8. Metadata-only chat-compatible chunks are valid turn progress and must not be
   dropped merely because `content` is `null`.
9. Prompt-eval progress and timing snapshots must flow through the runtime as
   structured updates, not as ad hoc transcript spam.
10. Text, tool blocks, and timing/progress metadata remain separate concerns in
   the update pipeline.

### Operator-surface contract

11. The waiting lane keeps the ADR-039 canonical phrase
    `Mapping adjacent sectors...` and appends telemetry rather than replacing
    the phrase.
12. While a turn is waiting for first text, the operator surface may append
    live counters such as `read:2048/2641` in the same status lane.
13. After a turn completes, the transcript may append a compact timing summary
    such as `ttft`, `read`, `generate`, and `total`.
14. These additions remain subordinate status telemetry, not primary response
    prose.

## Consequences

### Positive

- Local turns become observable during prompt evaluation rather than looking
  frozen.
- Operators can distinguish prompt-eval latency from generation latency.
- The UI preserves ADR-039 voice while gaining concrete runtime telemetry.

### Negative

- The local chat-compatible path now knowingly depends on backend-specific
  progress fields.
- Tests must cover metadata-only chunks because they are now semantically
  meaningful.
- Telemetry availability still depends on backend support; unsupported servers
  fall back to elapsed-only waiting state.

## Implementation notes

Candidate implementation areas:

- `src/api/client.rs`
- `src/api/stream.rs`
- `src/state/conversation/`
- `src/runtime/context.rs`
- `src/app/`

## References

- [ADR-030](ADR-030-runtime-task-state-and-orchestrator-control-flow.md)
- [ADR-031](ADR-031-operator-surface-ui-overhaul.md)
- [ADR-038](ADR-038-memory-first-architecture-with-minimal-disk-io.md)
- [ADR-039](ADR-039-neutral-cli-voice-and-spatial-status-language.md)