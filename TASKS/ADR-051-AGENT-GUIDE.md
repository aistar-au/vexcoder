# ADR-051 Agent Implementation Guide: Context Continuity

This document details the active work for **ADR-051: Durable Working-Set Record and Context Continuity**. It is the guide for retained, superseded, and added APIs, and the crate documentation that drives this change.

## 1. Active Work Overview

The repository is executing a 5-phase implementation plan for fragmented context construction. Each phase is an isolated, testable batch. Do not fold later phases into an earlier batch.

**Primary Checklist:** `TASKS/PN-01-working-set-record.md`
**Architectural Decision:** `adr/ADR-051-durable-working-set-record-and-context-continuity.md`

## 2. Reference Documentation

When working on any phase of ADR-051, agents must consult:
1. **The ADR itself** for the exact Rust struct definitions (`WorkingSetRecord`, `MemoryCandidate`) and the `loro` CRDT API sketches.
2. **The Crate Documentation:**
   - `tiktoken` 4.1.2 (`docs.rs/tiktoken/4.1.2`): `get_encoding` / `encoding_for_model` return `Option<&'static CoreBpe>`; `CoreBpe::count` is the zero-allocation BPE count.
   - `schemars` 1.2.2 (`docs.rs/schemars/1.2.2`): `#[derive(JsonSchema)]` and `schema_for!` (JSON Schema 2020-12).
   - `loro` 1.16 (`docs.rs/loro`): `LoroDoc`, `ExportMode::Snapshot` / `ExportMode::updates`, `VersionVector`, `LoroMap::insert_container`.
3. **The Active Roadmap:** `TASKS/ACTIVE-ROADMAP.md` (Tier 14) for phase dependencies.

## 3. Net Changes Matrix (Retained / Superseded / Added APIs)

This matrix names the APIs each batch retains, supersedes, and adds.

| Component | Retained API | Superseded API | Added API |
| :--- | :--- | :--- | :--- |
| **Working-set restore on `/resume`** | `task_state_bridge.rs` TUI snapshot projection (`TaskDocumentCondenser::restore_from_snapshot`). | `reset_conversation_window` in `pulse.rs` calling `ConversationManager::clear_messages` (`api_messages.clear()`) with no continuity source. | `WorkingSetRecord::{try_load,try_load_from_search_dirs_from,as_prompt_block}` copied onto `ApiClient::set_supplementary_system_prompt` via `ConversationManager::seed_from_working_set`. |
| **Compaction** | Append-only `ApiMessage` log and bounded excerpts. `ContextCompactionRecord`. | `handle_compact_command` calling `reset_conversation_window` after `completed_turns.clear()`. | `TaskDocumentCondenser::write_working_set` before pulse clear; next request uses `WorkingSetRecord::as_prompt_block`. `objective` is write-once via `retain_durable_objective`. |
| **Instructions** | `project_instructions.rs` candidate-name list. | First-file-only loader that returned `OverBudget` and failed closed. | Root-to-leaf `load_hierarchical_instructions` using `std::fs` with `InstructionSet` manifest recording for skipped files. |
| **Memory / Notes** | The `notes_path` configuration and disk storage. | Flat file all-or-nothing copy into the prompt that hit a silent budget cliff. | Typed `MemoryCandidate` struct with `source`, `topic`, and `status` (pending/accepted). `inject_accepted` copies only `Accepted`. |
| **Peer Channel** | `peer_channel.rs` JSONL `append_message` / `read_messages` and ADR-046 HTTP routes. | `apply_join_outcome` concatenating every child `handoff_summary`. | `PeerMergeDoc` (`LoroDoc`, `ExportMode::updates`, `VersionVector`). `live_entries` after message-id supersession. `record_peer_join_evidence` writes `RecordedDecision.source_reference`. |
| **Schema Validation** | `serde_json` persistence. | Ad-hoc, untyped JSON serialization for task state extensions. | `schemars` `#[derive(JsonSchema)]` with a CI schema-diff guard. |

## 4. Architectural Reasoning (Pros & Cons based on API Research)

The decisions in ADR-051 are directly informed by researching managed provider APIs and evaluating Rust ecosystem crates.

### Why `tiktoken` over byte-division?
*   **Research:** Managed provider APIs define context windows in exact BPE tokens. A `len/4` heuristic causes silent budget cliffs where files are skipped or kept incorrectly.
*   **Pros:** `tiktoken` 4.1.2 provides a zero-allocation `CoreBpe::count` path. Lookups return `Option<&'static CoreBpe>`.
*   **Cons:** Adds a dependency and embedded vocabulary tables (mitigated by `vocab-o200k_base` only).

### Why `schemars` over hand-written JSON schemas?
*   **Research:** If the persisted `WorkingSetRecord` drifts from the Rust type, `/resume` cannot deserialize the record and the next request starts from an empty `ApiMessage` window.
*   **Pros:** `schemars` 1.2.2 ties the Rust struct directly to a checked-in schema file (`schemas/working_set.schema.json`), allowing CI to fail the build on drift.
*   **Cons:** Requires maintaining the schema generation step in CI.

### Why `loro` (CRDT) over JSONL append-only?
*   **Research:** Isolated writers need a deterministic merge. A JSONL log forces the orchestrator to read free-text summaries, destroying structured state.
*   **Pros:** `loro` provides causal merge semantics (`VersionVector`), allowing subagents to update the working set concurrently without conflicts.
*   **Cons:** `loro` is a complex dependency; moving from "append and read" to a CRDT surface requires rigorous testing.

### Why Reject Opaque Provider-Side Compaction?
*   **Research:** Some managed APIs offer compaction endpoints that return encrypted, unreadable continuation blobs.
*   **Reasoning:** This violates the repository's local-first, inspectable design (ADR-023/038). We must be able to resume, export, and replay tasks without depending on a vendor's store staying reachable. The `WorkingSetRecord` is the local, readable equivalent.

## 5. Batching Strategy

To keep CI green and changes reviewable, ADR-051 is divided into 5 PR batches. **Do not combine these phases into a single PR.**

1.  **Batch 1 (PR #444, merged):** Schema (`schemars`), Persistence, and Token Accuracy (`tiktoken`).
2.  **Batch 2 (PR #445, merged):** Hierarchical Instructions & Memory Candidates.
3.  **Batch 3 (PR #446, merged):** Restore the next request from `WorkingSetRecord` on `/resume` and `/compact` (`reset_conversation_window` is no longer called on those paths).
4.  **Batch 4 (this PR):** Peer-join merge via `PeerMergeDoc` (`loro`). JSONL ADR-046 routes stay.
