# Task PN-01: Durable Working-Set Record

**Target files:**
- `src/runtime/task_state/` — persist and load the working-set record
- `src/runtime/task_document/` — project the record from pulse evidence
- `src/app/pulse.rs` — hydrate on resume instead of clearing the live window
- `src/app/commands/session.rs` — `/resume` and `/compact` write and restore the record
- `src/runtime/project_instructions.rs` — hierarchical load with budget fallback
- `src/session_notes.rs`, `src/auto_memory.rs` — typed reviewable candidates
- `src/runtime/task_state/peer_channel.rs` — cursors, supersession, evidence links
- `src/app/subtask_orchestrator/mod.rs` — join merge against the record
- `src/state/conversation/history.rs` — local compaction fallback from the record

**ADR:** ADR-051

**Depends on:** ADR-045 Batch 1 (sole-writer task document), ADR-046 (peer
channel), ADR-049 shared-prefix contract.

---

## Phase 0 — notes refresh stopgap — CLOSED, NOT MERGED

The Phase 0 stopgap previously drafted on PR #443 (shared `ApiClient` notes
storage, per-pulse reload, local server-info preload) was closed without
merging. The notes-refresh problem is solved by Phase 4 typed candidates
instead. Do not revive the stopgap as a second continuity protocol.

## Phase 1 — record schema and persistence
…(unchanged from the ADR-051 decision)…

## Phase 2 — hydrate on resume and compact
## Phase 3 — hierarchical instruction loading
## Phase 4 — reviewable memory candidates
## Phase 5 — peer join merge
