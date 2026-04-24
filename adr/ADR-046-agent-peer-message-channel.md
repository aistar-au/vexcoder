# ADR-046: Agent Peer Message Channel

**Status:** Accepted  
**Chain:** ADR-034, ADR-025, ADR-028, ADR-038, ADR-045

## Context

Concurrent sub-agents had no structured path to exchange corrections or status updates with each other or with the orchestrator without routing through the full task-state write path.

## Decision

- Per-parent-task append-only JSONL channel at `.vex/state/{parent_task_id}.channel.jsonl`.
- Any session task belonging to the parent may post to and read from the channel.
- `recipient` field: `"*"` (broadcast) or `"{agent_id}"` (point-to-point).
- Limits: max message 4 KB, max 256 lines per channel, ~1 MiB hard safety valve, read batch cap 64.
- Message format: JSON object with `id`, `sent_at` (pagination cursor), `sender_id`, `kind`, `content`, `parent_task_id`.
- Two-layer locking: in-process [`std::sync::Mutex`](https://doc.rust-lang.org/std/sync/struct.Mutex.html) + [`std::sync::OnceLock`](https://doc.rust-lang.org/std/sync/struct.OnceLock.html) for process-local serialization, plus cross-process `flock` via [`fs2`](https://docs.rs/fs2).
- No model-aware interrupt; agents poll on their read cadence.
- Reserve `PeerMessagePosted` `RuntimeEvent` variant for ADR-045 compatibility.

## References

- Rust std [`std::sync::Mutex`](https://doc.rust-lang.org/std/sync/struct.Mutex.html)
- Rust std [`std::sync::OnceLock`](https://doc.rust-lang.org/std/sync/struct.OnceLock.html)
- [`fs2`](https://docs.rs/fs2) — cross-process file locking
- [`serde_json`](https://docs.rs/serde_json) — JSONL serialization
