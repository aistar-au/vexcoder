# ADR-051 Agent Implementation Guide: Context Continuity

This document details the active work for **ADR-051: Durable Working-Set Record and Context Continuity**. It is the guide for retained, superseded, and added APIs, and the crate documentation that drives this change.

## 1. Active Work Overview

The repository is executing a 5-phase implementation plan for fragmented context construction. Each phase is an isolated, testable batch. Do not fold later phases into an earlier batch.

**Primary Checklist:** `TASKS/PN-01-working-set-record.md`
**Architectural Decision:** `adr/ADR-051-durable-working-set-record-and-context-continuity.md`

## 2. Reference Documentation

When working on any phase of ADR-051, agents must consult:
1. **The ADR itself** for the exact Rust struct definitions (`WorkingSetRecord`, `MemoryCandidate`, `JoinIndex`, `StateEnvelope`).
2. **The Crate Documentation:**
   - `tiktoken` 4.1.2 (`docs.rs/tiktoken/4.1.2`): `get_encoding` / `encoding_for_model` return `Option<&'static CoreBpe>`; `CoreBpe::count` is the zero-allocation BPE count.
   - `schemars` 1.2.2 (`docs.rs/schemars/1.2.2`): `#[derive(JsonSchema)]` and `schema_for!` (JSON Schema 2020-12) for `WorkingSetRecord`, `MemoryCandidateStore`, and `JoinIndex`.
3. **The Active Roadmap:** `TASKS/ACTIVE-ROADMAP.md` (Tier 14) for phase dependencies.

Do not reintroduce `loro`, `PeerMergeDoc`, or `{id}.channel.crdt`. Production join is a single-process orchestrator; typed JSON is the join document.

## 3. Net Changes Matrix (Retained / Superseded / Added APIs)

This matrix names the APIs each batch retains, supersedes, and adds.

| Component | Retained API | Superseded API | Added API |
| :--- | :--- | :--- | :--- |
| **Working-set restore on `/resume`** | `task_state_bridge.rs` TUI snapshot projection (`TaskDocumentCondenser::restore_from_snapshot`). | `reset_conversation_window` in `pulse.rs` calling `ConversationManager::clear_messages` (`api_messages.clear()`) with no continuity source. | `WorkingSetRecord::{try_load,try_load_from_search_dirs_from,as_prompt_block}` copied onto `ApiClient::set_supplementary_system_prompt` via `ConversationManager::seed_from_working_set`. |
| **Compaction** | Append-only `ApiMessage` log and bounded excerpts. `ContextCompactionRecord`. | `handle_compact_command` calling `reset_conversation_window` after `completed_turns.clear()`. | `TaskDocumentCondenser::write_working_set` before pulse clear; next request uses `WorkingSetRecord::as_prompt_block`. `objective` is write-once via `retain_durable_objective`. `retain_referenced_decisions` keeps join evidence. |
| **Instructions** | `project_instructions.rs` candidate-name list. | First-file-only loader that returned `OverBudget` and failed closed. | Root-to-leaf `load_hierarchical_instructions` using `std::fs` with `InstructionSet` manifest recording for skipped files. |
| **Memory / Notes** | The `notes_path` configuration and disk storage. | Flat file all-or-nothing copy into the prompt that hit a silent budget cliff. | Typed `MemoryCandidate` struct with `source`, `topic`, and `status` (pending/accepted). `inject_accepted` copies only `Accepted`. |
| **Agent join** | `peer_channel.rs` JSONL `append_message` / `read_messages` and ADR-046 HTTP routes. | `apply_join_outcome` concatenating every session-task `handoff_summary`. Empty `JoinSummary.supersedes` on the production path. `PeerMergeDoc` / `loro` / `{id}.channel.crdt`. | `JoinIndex` at `{id}.join.json`. `poll_fan_out_join` writes `JoinSummary.supersedes` from `SessionTask.supersedes` plus same-agent earlier completions. `facade_poll_join` calls `apply_join_outcome`. `live_entries` after message-id supersession. `record_join_evidence` writes `RecordedDecision.source_reference`. `StateEnvelope` + GET `/v1/tasks/{id}/working-set`. |
| **Schema Validation** | `serde_json` persistence. | Ad-hoc, untyped JSON serialization for task state extensions. | `schemars` `#[derive(JsonSchema)]` with a CI schema-diff guard for the working-set, memory-candidate, and join-index schemas. |

## 4. Architectural Reasoning (Pros & Cons based on API Research)

The decisions in ADR-051 are directly informed by researching managed provider APIs and evaluating Rust ecosystem crates.

### Why `tiktoken` over byte-division?
*   **Research:** Managed provider APIs define context windows in exact BPE tokens. A `len/4` heuristic causes silent budget cliffs where files are skipped or kept incorrectly.
*   **Pros:** `tiktoken` 4.1.2 provides a zero-allocation `CoreBpe::count` path. Lookups return `Option<&'static CoreBpe>`.
*   **Cons:** Adds a dependency and embedded vocabulary tables (mitigated by `vocab-o200k_base` only).

### Why `schemars` over hand-written JSON schemas?
*   **Research:** If the persisted `WorkingSetRecord` drifts from the Rust type, `/resume` cannot deserialize the record and the next request starts from an empty `ApiMessage` window.
*   **Pros:** `schemars` 1.2.2 ties the Rust struct directly to a checked-in schema file, allowing CI to fail the build on drift.
*   **Cons:** Requires maintaining the schema generation step in CI.

### Why `JoinIndex` over JSONL concatenation and over a CRDT?
*   **Research:** Isolated session-task summaries need a deterministic replace rule. Concatenating `handoff_summary` strings has no conflict rule. A CRDT (`loro`) is built for independent writers exchanging missing ops; production join is one in-process document. The `loro` graph also failed `cargo deny` (MPL-2.0).
*   **Pros:** Message-id `supersedes` plus `live_entries` is inspectable typed JSON. `schemars` guards the on-disk contract. `StateEnvelope` keeps consumers off the filesystem.
*   **Cons:** Join is not multi-writer. Concurrent external writers would need a later ADR.

### Why Reject Opaque Provider-Side Compaction?
*   **Research:** Some managed APIs offer compaction endpoints that return encrypted, unreadable continuation blobs.
*   **Reasoning:** This violates the repository's local-first, inspectable design (ADR-023/038). We must be able to resume, export, and replay tasks without depending on a vendor's store staying reachable. The `WorkingSetRecord` is the local, readable equivalent.

## 5. Batching Strategy

To keep CI green and changes reviewable, ADR-051 is divided into 5 PR batches. **Do not combine these phases into a single PR.**

1.  **Batch 1 (PR #444, merged):** Schema (`schemars`), Persistence, and Token Accuracy (`tiktoken`).
2.  **Batch 2 (PR #445, merged):** Hierarchical Instructions & Memory Candidates.
3.  **Batch 3 (PR #446, merged):** Restore the next request from `WorkingSetRecord` on `/resume` and `/compact` (`reset_conversation_window` is no longer called on those paths).
4.  **Batch 4 (this PR):** Agent-join merge via `JoinIndex`. JSONL ADR-046 routes stay. `poll_fan_out_join` writes `JoinSummary.supersedes`. `facade_poll_join` calls `apply_join_outcome`. `StateEnvelope` is the internal read API. Do not restore `loro` / `PeerMergeDoc`. Do not drive join replace from `PeerMessageKind`.

## 6. Phase 5 production call graph (do not reintroduce unused join surface)

Researched from the production graph, not from a CRDT catalog:

| Production function | What it writes |
| :--- | :--- |
| `schedule_team` / `advance_sequential` / `facade_delegate_session_task` | `SessionTask.supersedes` via `stamp_join_supersedes` |
| `poll_fan_out_join` | `JoinSummary.supersedes` (spawn stamp union same-agent earlier completions). This is the only production writer of that field. |
| `facade_poll_join` | Calls `apply_join_outcome` when no session-task remains live. Callers: HTTP `join_status_handler`, `/watch` (`commands/info.rs`). |
| `apply_join_outcome` | `JoinIndex::post` / `save` at `{id}.join.json`; `record_join_evidence` on the working-set sidecar |

Do not restore: `PeerMergeDoc`, `loro`, `{id}.channel.crdt`, empty poll `supersedes`, `facade_poll_join` without `apply_join_outcome`, `PeerMessageKind` as the join rule, or a notes-file fingerprint refresh beside `WorkingSetRecord`. JSONL `append_message` / `read_messages` stay.
