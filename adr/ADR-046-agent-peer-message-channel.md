# ADR-046: Agent Peer Message Channel

- **Status:** Accepted
- **Date:** 2026-04-13
- **Deciders:** Core maintainer
- **Depends on:** ADR-034, ADR-025, ADR-028, ADR-038, ADR-045
- **Deprecates:** None
- **Deprecated by:** None

---

## Context

ADR-034 introduced `SessionTask` with a `handoff_summary` field for
communicating the result of a completed sub-task to the parent or the next
agent in sequence. That model is adequate for coarse-grained handoffs but
provides no mechanism for in-flight peer correction: a running agent cannot
signal to a concurrently running peer that its approach is heading in the wrong
direction without waiting for the full handoff cycle.

The gap manifests in two practical scenarios:

1. **Parallel review.** A `rust-fixer` agent is applying a large refactor.
   A `docs-reviewer` agent running in parallel notices the refactor violates
   an invariant documented in `ARCHITECTURE.md`. Under the handoff model the
   reviewer can only write its concern to its own `handoff_summary`, which the
   parent orchestrator reads only after both agents complete. By then the
   refactor is committed and the correction cost is higher.

2. **Sequential critique.** A `proposer` agent writes a plan and marks itself
   complete. A `critic` agent is delegated the plan. Under the handoff model
   the critic can post its critique to its own `handoff_summary` for the parent
   to read, but the proposer has already exited. If a second revision is needed
   the orchestrator must delegate a new task, losing the conversational thread.

Neither scenario requires synchronous blocking: in both cases the correction is
useful whenever the target agent next checks for messages, not immediately. An
asynchronous, append-only, file-backed message channel per parent task is
sufficient.

### Constraints from existing ADRs

- **ADR-034 §1** — The orchestrator is the only authority for session-task
  lifecycle transitions. The channel must not let agents directly change each
  other's `lifecycle_state`; it is a communication layer, not a control layer.
- **ADR-028** — All session-task operations go through the application facade.
  No runtime layer code may call message-channel functions directly.
- **ADR-038** — All new disk reads must respect bounded allocation. Channel
  reads must be capped and support cursor-based pagination.
- **ADR-045** — Every piece of state that affects runtime semantics must
  eventually have a `RuntimeEvent` variant. A `PeerMessagePosted` stub is
  introduced here and must be completed when ADR-045 event-log work lands.

---

## Decision

Introduce a **peer message channel** as a per-parent-task append-only JSONL
file at `.vex/state/{parent_task_id}.channel.jsonl`. Any session task belonging
to the parent may post to and read from the channel. The orchestrator exposes
the channel through two new facade functions and two new HTTP routes.

### Scope

This ADR:

- Defines `PeerMessage` and `PeerMessageKind` types in
  `src/runtime/task_state/peer_channel.rs`.
- Defines two facade functions in `src/app/task_facade.rs`:
  `facade_post_peer_message` and `facade_read_peer_messages`.
- Defines two HTTP routes: `POST /v1/tasks/{id}/messages` and
  `GET /v1/tasks/{id}/messages`.
- Reserves `PeerMessagePosted(PeerMessage)` as a future `RuntimeEvent` variant
  for ADR-045 compatibility.
- Does **not** add any field to `SessionTask` or `TaskState`; message state is
  stored in a sidecar JSONL file, keeping the main state file clean.

This ADR does not:

- Let agents transition each other's lifecycle state.
- Add a synchronous blocking "wait for reply" mechanism.
- Add in-process channels, `mpsc`, or `tokio` primitives — the channel is
  file-backed only for durability and replay compatibility.
- Process message content; the runtime passes messages to the receiver verbatim.
  Interpretation is the receiving agent's responsibility.

---

## Addressing model

Each message carries a `recipient` field:

| Value | Meaning |
|-------|---------|
| `"*"` | Broadcast — all agents in the parent task read this message |
| `"{agent_id}"` | Point-to-point — only the named agent processes this message |

The channel file contains all messages regardless of recipient. Agents filter
by recipient on the read side. This keeps the file structure simple and
audit-friendly.

---

## Size and depth limits

| Limit | Value | Rationale |
|-------|-------|-----------|
| `MAX_PEER_MESSAGE_BYTES` | 4 096 | Enough for a paragraph of correction text; prevents channel bloat |
| `MAX_CHANNEL_DEPTH` | 256 | One channel file never grows past ~1 MiB even at max message size |
| `MAX_CHANNEL_FILE_BYTES` | 1 048 576 | Hard file-size safety valve independent of line count |
| `MAX_CHANNEL_READ_BATCH` | 64 | Bounds read allocation per request; callers page with `after_ms` |

When `MAX_CHANNEL_DEPTH` or `MAX_CHANNEL_FILE_BYTES` is reached,
`facade_post_peer_message` returns `PeerChannelError::ChannelFull`. The caller
must wait for the orchestrator to archive the channel before posting again.

---

## File format

Each line in `.vex/state/{parent_task_id}.channel.jsonl` is one JSON object:

```json
{"id":"550e8400-e29b-41d4-a716-446655440000","sent_at":1744538412345,"sender_id":"parent-rust-fixer-550e...","sender_agent_id":"rust-fixer","recipient":"*","kind":"Observation","content":"auth.rs:42 — removing the nonce check here will break the replay guard in ADR-023","parent_task_id":"parent-abc"}
```

Fields:

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID string | Unique message identifier |
| `sent_at` | u64 (epoch ms) | Used as the pagination cursor |
| `sender_id` | SessionTaskId | Full session task ID of the sender |
| `sender_agent_id` | String | Agent name derived from the persisted session task |
| `recipient` | String | `"*"` or an agent_id |
| `kind` | PeerMessageKind | `Observation`, `Correction`, `Question`, `Acknowledgement` |
| `content` | String | Message body (max `MAX_PEER_MESSAGE_BYTES` bytes) |
| `parent_task_id` | String | Parent task the channel belongs to |

---

## Concurrency model

Writes use a two-layer locking pattern matching `task_facade.rs`:

1. **In-process `Mutex`** — serialises concurrent `append_message` calls from
   threads in the same process.
2. **Cross-process `flock`** via `fs2::FileExt` — serialises writes from
   concurrent agent processes sharing a parent task.

Both readers and writers acquire their lock on the same
`.vex/state/{parent_task_id}.channel.lock` file. Readers use a shared lock;
writers use an exclusive lock. This ensures readers never observe a
partially-written JSONL line.

`O_APPEND` alone is insufficient for correctness on macOS and Windows where
POSIX atomicity guarantees are weaker than on Linux.

---

## ADR-045 compatibility

When ADR-045 lands and introduces `RuntimeEventLog` as the authoritative
persisted record, the following `RuntimeEvent` variant must be added and a
`TaskDocumentCondenser::apply_event` arm provided:

```rust
// Reserved — do not implement until ADR-045 event-log work is Accepted.
// PeerMessagePosted(PeerMessage),
```

Until then, the JSONL sidecar file is the sole persistence mechanism. The
`PeerMessage` type is already `Serialize + Deserialize` and can be embedded in
a `RuntimeEnvelope` without modification.

---

## Consequences

### Positive

- Agents running in parallel can share observations and corrections without
  waiting for a full handoff cycle.
- The channel is durable (file-backed) and human-readable (JSONL), consistent
  with the existing task-state format.
- No in-process synchronisation primitives are introduced; the channel works
  across process restarts and is safe for multi-process agent deployments.
- Message history is auditable: the complete channel is retained in the sidecar
  file and can be replayed when ADR-045 event-log work lands.
- The facade boundary (ADR-028) is respected throughout: no runtime code calls
  channel functions directly.

### Negative

- **Polling.** Agents must actively read the channel — there is no push
  notification. The recommended pattern is to read at the start of each
  reasoning step.
- **No interrupt support.** A correction that arrives while an agent is
  mid-reasoning sits unread until that agent finishes its current step and calls
  `facade_read_peer_messages`. For latency-sensitive correction scenarios this
  is the most material limitation; a `should_interrupt` field and runtime-loop
  injection would address it but require a separate ADR touching the agent loop.
- **No strict ordering guarantee.** Two concurrent writers on POSIX filesystems
  without a write lock can interleave. The two-layer locking scheme (in-process
  mutex + cross-process flock) prevents this in practice, but the timestamp
  ordering is advisory: callers that post multiple messages inside one
  millisecond can produce identical `sent_at` values. Strict ordering would
  require a monotonic counter with the flock held across the counter read and
  write.
- **Channel-full intervention.** The channel-full error forces explicit
  orchestrator intervention. This is intentional — the orchestrator must archive
  or clear the channel rather than silently dropping messages.
- **Sidecar cleanup.** The sidecar file must be archived or deleted when the
  parent task is archived, or it will accumulate indefinitely. The
  `facade_release_session_task` cleanup path should be extended to handle
  sidecar files.

---

## Validation

Unit tests (inline in `src/runtime/task_state/peer_channel.rs`):

- `round_trip_single_message`
- `after_ms_cursor_excludes_earlier_messages`
- `recipient_filter_delivers_broadcast_and_targeted`
- `channel_full_error_at_depth_cap`
- `read_returns_empty_when_no_channel_file`
- `read_batch_is_capped_at_max`
- `skips_malformed_lines_without_panicking`
- `parse_peer_message_kind_handles_all_variants`
- `file_size_safety_valve_rejects_oversized_channel`
- `oversized_line_skipped_during_read`

Facade integration tests (`src/app/task_facade/tests.rs`):

- `test_post_peer_message_rejects_unknown_sender`
- `test_post_peer_message_rejects_content_too_long`
- `test_post_peer_message_rejects_invalid_kind`
- `test_post_peer_message_delivers_to_broadcast_and_targeted_reader`
- `test_read_peer_messages_returns_empty_before_first_post`
- `test_read_peer_messages_respects_after_ms_cursor`
- `test_post_peer_message_returns_channel_full_at_depth_cap`
- `test_post_peer_message_derives_sender_agent_id_from_task`
- `test_post_peer_message_cross_task_sender_rejected`

---

## Implementation order (completed in PR #378)

1. `src/runtime/task_state/peer_channel.rs` — types and file I/O with two-layer
   locking
2. `src/runtime/task_state/mod.rs` — `pub(crate) mod peer_channel`
3. `src/app/task_facade/types.rs` — `PeerChannelError` typed error enum
4. `src/app/task_facade.rs` — `facade_post_peer_message`,
   `facade_read_peer_messages`, `load_parent_task_state`
5. `src/app/task_facade/tests.rs` — facade integration tests
6. `src/server/handlers/session.rs` — `POST`/`GET` route handlers
7. `src/server/http.rs` — route wiring
8. `src/app.rs` — re-exports

---

## References

- [ADR-034](https://github.com/aistar-au/vexapi/blob/main/adr/ADR-034-multi-agent-parallel-task-execution.md) — multi-agent parallel task execution
- [ADR-025](https://github.com/aistar-au/vexapi/blob/main/adr/completed/ADR-025-runtime-json-handoff-contract.md) — runtime JSON handoff contract
- [ADR-028](https://github.com/aistar-au/vexapi/blob/main/adr/ADR-028-application-facade-and-transport-boundaries.md) — application facade and transport boundaries
- [ADR-038](https://github.com/aistar-au/vexapi/blob/main/adr/ADR-038-memory-first-architecture-with-minimal-disk-io.md) — memory-first architecture with minimal disk I/O
- [ADR-045](https://github.com/aistar-au/vexapi/blob/main/adr/ADR-045-replay-first-task-document-and-single-writer-state.md) — replay-first task document and single-writer state
