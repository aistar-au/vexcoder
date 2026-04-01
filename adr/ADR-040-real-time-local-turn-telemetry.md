# ADR-040: Real-Time Local Turn Telemetry

- **Status:** Proposed
- **Date:** 2026-03-31
- **Deciders:** Core maintainer
- **Depends on:** ADR-030, ADR-031, ADR-038, ADR-039
- **Supersedes:** None
- **Superseded by:** None

## Context

Local inference servers can spend substantial time in prompt evaluation before
the first text token arrives. Both streaming protocols supported by the
runtime — chat-compatible (`/v1/chat/completions`) and messages/v1
(`/messages/v1`) — can carry prompt-eval telemetry, but through different
mechanisms.

On endpoints that support the chat-completions protocol, the prompt phase can
emit useful stream metadata such as:

1. `prompt_progress` counters while prompt tokens are still being ingested.
2. `timings` snapshots that distinguish prompt-eval time from generation time.

On endpoints that support the messages/v1 protocol, the same telemetry is
carried natively inside event metadata without requiring opt-in request flags.

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

Treat local prompt-eval telemetry as first-class turn state across both
streaming protocols.

### Scope boundaries

1. This ADR applies to local streaming requests on both protocols and their
   operator-facing transcript/status surfaces.
2. This ADR does **not** change canonical persisted task-state machine fields.
3. This ADR does **not** add provider-specific telemetry to remote API-server
   payloads by default.
4. This ADR does **not** replace the ADR-039 canonical waiting phrase.

### Request contract — chat-compatible protocol

5. Local chat-compatible streaming requests must ask for prompt progress and
   per-token timing metadata when the backend supports it.
6. The request contract for that local path includes `return_progress = true`
   and `timings_per_token = true` as extra body parameters.
7. Remote API-server payloads remain unchanged unless a separate ADR expands
   provider-specific telemetry on those paths.

### Request contract — messages/v1 protocol

8. Messages/v1 streaming requests do not require opt-in flags for telemetry.
   Prompt progress and timing data arrive as metadata on standard stream
   events when the backend supports them.
9. The system prompt is passed as a top-level `system` field, not embedded in
   messages.
10. Tool choice uses structured format (`{"type": "auto"}`), and stop
    conditions use the `stop_sequences` key.

### Stream conversion contract — chat-compatible protocol

11. Metadata-only chat-compatible chunks are valid turn progress and must not
    be dropped merely because `content` is `null`.
12. The chat-compatible chunk struct carries `prompt_progress` and `timings`
    as top-level fields. The stream parser converts these into
    `StreamChunkMetadata` and emits them on `MessageStart` or `MessageDelta`
    events.
13. A `[DONE]` sentinel terminates the chat-compatible stream.

### Stream conversion contract — messages/v1 protocol

14. Messages/v1 events deserialize directly into the `StreamEvent` enum
    without an intermediate conversion step.
15. Telemetry arrives inside `StreamChunkMetadata` nested within
    `MessageStart.message.metadata` or `MessageDelta.delta.metadata`.
16. Usage data on messages/v1 appears as a top-level peer of `delta` in
    `MessageDelta` events, not nested inside chunk metadata.
17. No `[DONE]` sentinel is used; the stream ends with a `MessageStop` event.

### Shared telemetry types

18. Both protocols share the same telemetry structs after stream parsing:
    - `StreamPromptProgress`: `total`, `cache`, `processed`, `time_ms`.
    - `StreamTimings`: `cache_n`, `prompt_n`, `prompt_ms`,
      `prompt_per_token_ms`, `prompt_per_second`, `predicted_n`,
      `predicted_ms`, `predicted_per_token_ms`, `predicted_per_second`.
    - `ApiUsage`: `input_tokens` / `prompt_tokens`, `output_tokens` /
      `completion_tokens`, `cache_creation_input_tokens`,
      `cache_read_input_tokens`, plus `prompt_tokens_details` and
      `completion_tokens_details` sub-objects.
19. Text, tool blocks, and timing/progress metadata remain separate concerns
    in the update pipeline.

### Stream parser dispatch

20. The stream parser attempts messages/v1 deserialization first. On failure
    it falls back to chat-compatible parsing. This ordering means
    messages/v1 events are never misinterpreted as chat-compatible chunks.

### Operator-surface contract

21. The waiting lane keeps the ADR-039 canonical phrase
    `Mapping adjacent sectors...` and appends telemetry rather than replacing
    the phrase.
22. The direct ANSI CLI/app surface does not reserve a dedicated timeline
    strip; one top-aligned scrolling transcript pane owns the full upper body
    and renders waiting status, tool activity, approvals, and assistant output
    as paragraphs in that shared stream.
23. The persistent bottom surface is limited to the multiline composer and
    separate status bar; telemetry remains inline in transcript paragraphs,
    while the status bar may fold compact telemetry and git summaries into a
    single truncated line instead of claiming a dedicated fixed pane.
24. While a turn is waiting for first text, the operator surface may append
    active counters such as `read:2048/2641` in the transcript status lane.
25. After a turn completes, the transcript may append a compact timing summary
    such as `ttft`, `read`, `generate`, and `total`.
26. These additions remain subordinate status telemetry, not primary response
    prose.
27. The surface contract is protocol-agnostic; both protocols produce the same
    `StreamEvent` variants and telemetry types after parsing.

## Consequences

### Positive

- Local turns become observable during prompt evaluation rather than looking
  frozen.
- Operators can distinguish prompt-eval latency from generation latency.
- The UI preserves ADR-039 voice while gaining concrete runtime telemetry.
- Both protocols converge to the same internal event model, so downstream
  runtime and surface code is protocol-agnostic.

### Negative

- The local chat-compatible path now knowingly depends on backend-specific
  progress fields and opt-in request flags.
- The messages/v1 path carries telemetry natively but depends on per-backend
  support for populating `prompt_progress` and `timings` metadata.
- Tests must cover metadata-only chunks because they are now semantically
  meaningful.
- Telemetry availability still depends on backend support; unsupported servers
  fall back to elapsed-only waiting state on either protocol.

## Implementation notes

Candidate implementation areas:

- `src/api/client.rs` — request payload construction for both protocols;
  `apply_local_chat_compat_stream_flags()` inserts telemetry opt-in flags.
- `src/api/stream.rs` — `StreamParser` with messages/v1-first fallback chain;
  `ChatCompatChunk` intermediate struct for chat-compatible conversion.
- `src/types/api_types.rs` — shared `StreamEvent` enum, `StreamChunkMetadata`,
  `StreamPromptProgress`, `StreamTimings`, `ApiUsage`.
- `src/state/conversation/` — turn state consumption of telemetry events.
- `src/runtime/context.rs` — runtime dispatch and protocol selection.
- `src/app/` — operator-surface rendering of telemetry counters.

## References

- [ADR-030](ADR-030-runtime-task-state-and-orchestrator-control-flow.md)
- [ADR-031](ADR-031-operator-surface-ui-overhaul.md)
- [ADR-038](ADR-038-memory-first-architecture-with-minimal-disk-io.md)
- [ADR-039](ADR-039-neutral-cli-voice-and-spatial-status-language.md)
