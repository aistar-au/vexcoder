# ADR-051 Agent Implementation Guide: Context Continuity

This document details the active work for **ADR-051: Durable Working-Set Record and Context Continuity**. It serves as the authoritative guide for agents and contributors to understand the insertions, removals, net changes, and the researched API/crate reasoning driving this architectural shift.

## 1. Active Work Overview

The repository is currently executing a 5-phase implementation plan to fix fragmented context construction. The previous stopgap attempt (PR #443) was closed to ensure these changes are implemented in isolated, testable batches rather than a single monolithic PR.

**Primary Checklist:** `TASKS/PN-01-working-set-record.md`
**Architectural Decision:** `adr/ADR-051-durable-working-set-record-and-context-continuity.md`

## 2. Reference Documentation

When working on any phase of ADR-051, agents must consult:
1. **The ADR itself** for the exact Rust struct definitions (`WorkingSetRecord`, `MemoryCandidate`) and the `loro` CRDT API sketches.
2. **The Crate Documentation:**
   - `tiktoken` (docs.rs/tiktoken) for zero-allocation BPE token counting.
   - `schemars` (docs.rs/schemars) for `#[derive(JsonSchema)]` and schema drift prevention.
   - `loro` (docs.rs/loro) for `LoroDoc`, `VersionVector`, and causal merge semantics.
3. **The Active Roadmap:** `TASKS/ACTIVE-ROADMAP.md` (Tier 14) for phase dependencies.

## 3. Net Changes Matrix (Insertions, Removals, Additions)

This matrix defines exactly what code is being removed, what is staying, and what is being added.

| Component | What Stays | What is Removed (Faulty Code) | Net Change (Additions) |
| :--- | :--- | :--- | :--- |
| **Working-set restore on `/resume`** | `task_state_bridge.rs` TUI snapshot projection for UI surface. | `reset_conversation_window` in `pulse.rs` that dropped `ApiMessage` history. | Injection of serialized `WorkingSetRecord` block into the system prompt on `/resume`. |
| **Compaction** | Append-only `ApiMessage` log and bounded excerpts. | `history.rs` byte-division heuristic (`len/4`) and `user`-first-line summarizer. | Accurate BPE token counting via `tiktoken` + deterministic local fallback via the `WorkingSetRecord`. |
| **Instructions** | `project_instructions.rs` file discovery logic. | First-file-only loader that returned `OverBudget` and failed closed. | Root-to-leaf directory walk using `std::fs` with manifest recording for skipped files. |
| **Memory / Notes** | The `notes_path` configuration and disk storage. | Flat file all-or-nothing injection that hit a silent budget cliff. | Typed `MemoryCandidate` struct with `provenance`, `topic`, and `status` (pending/accepted). |
| **Peer Channel** | `peer_channel.rs` facade validation and ADR-046 routes. | Free-text summary concatenation on subagent `join`. | CRDT-based state-merge protocol via `loro` (supersession cursors, evidence links). |
| **Schema Validation** | `serde_json` persistence. | Ad-hoc, untyped JSON serialization for task state extensions. | `schemars` `#[derive(JsonSchema)]` with a CI schema-diff guard. |

## 4. Architectural Reasoning (Pros & Cons based on API Research)

The decisions in ADR-051 are directly informed by researching managed provider APIs (e.g., Responses API, managed CLI agents) and evaluating Rust ecosystem crates.

### Why `tiktoken` over byte-division?
*   **Research:** Managed provider APIs define context windows in exact BPE tokens. A `len/4` heuristic causes silent "budget cliffs" where files are skipped or kept incorrectly.
*   **Pros:** `tiktoken` provides a zero-allocation `count()` path that matches the exact vocabulary the model uses.
*   **Cons:** Adds a dependency and embedded vocabulary tables (mitigated by feature flags).

### Why `schemars` over hand-written JSON schemas?
*   **Research:** Provider APIs enforce strict JSON schemas for tool calls. If the persisted `WorkingSetRecord` drifts from the Rust type, `/resume` cannot deserialize the record and the next request starts from an empty `ApiMessage` window.
*   **Pros:** `schemars` ties the Rust struct directly to a checked-in schema file (`schemas/working_set.schema.json`), allowing CI to fail the build on drift.
*   **Cons:** Requires maintaining the schema generation step in CI.

### Why `loro` (CRDT) over JSONL append-only?
*   **Research:** Managed agent APIs handle subagent state via isolated, persistent sandbox environments that merge state deterministically. Our JSONL log forces the orchestrator to read free-text summaries, destroying structured state.
*   **Pros:** `loro` provides causal merge semantics (`VersionVector`), allowing subagents to update the working set concurrently without conflicts.
*   **Cons:** `loro` is a complex dependency; moving from "append and read" to a CRDT surface requires rigorous testing.

### Why Reject Opaque Provider-Side Compaction?
*   **Research:** Some managed APIs offer `/compact` endpoints that return encrypted, unreadable continuation blobs.
*   **Reasoning:** This violates the repository's local-first, inspectable design (ADR-023/038). We must be able to resume, export, and replay tasks without depending on a vendor's store staying reachable. The `WorkingSetRecord` is our local, readable equivalent.

## 5. Batching Strategy

To ensure CI remains green and changes are reviewable, ADR-051 is strictly divided into 5 PR batches. **Do not combine these phases into a single PR.**

1.  **Batch 1 (PR #444, merged):** Schema (`schemars`), Persistence, and Token Accuracy (`tiktoken`).
2.  **Batch 2 (this PR):** Hierarchical Instructions & Memory Candidates.
3.  **Batch 3 (PR 3):** Seed the next request from `WorkingSetRecord` on `/resume` and `/compact` (removing `reset_conversation_window`).
4.  **Batch 4 (PR 4):** Peer Channel CRDT Migration (`loro`).
