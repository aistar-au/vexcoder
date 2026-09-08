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

**Status:** Batch 1 (this PR). Do not fold later phases into this change.

### Net change

| Surface | Stays | Removed | Inserted |
| :--- | :--- | :--- | :--- |
| Working-set file | `TaskState` `{id}.json` persist, `write_json_safe`, durable-access policy | Ad-hoc untyped extras on the task-state JSON | `WorkingSetRecord` at `.vex/state/{task_id}.working-set.json` |
| Schema | `serde_json` for task state | Hand-written continuity JSON | `schemars` `#[derive(JsonSchema)]` + checked-in `schemas/working_set.schema.json` |
| Token budget | Call sites in `session_notes` and `project_instructions` | `len / 4` and `(len + 3) / 4` heuristics | `tiktoken` `CoreBpe::count` via `src/runtime/token_count.rs` |
| Task-state scan | `{id}.json` membership | Accidental scan of `*.working-set.json` as a task id | Skip sidecar filenames in `visit_state_files_in_dir` |
| Instruction loader | Single-directory first-match `load_project_instructions` | Nothing in this batch (walk is Phase 3) | Token count only |
| Memory | Flat notes file injection | Byte-quarter budget on notes | Token count only (typed candidates are Phase 4) |

### Files

- Inserted: `src/runtime/token_count.rs`, `src/runtime/task_state/working_set.rs`
- Updated: `src/session_notes.rs`, `src/runtime/project_instructions.rs`, `src/runtime/task_state/persist.rs`, `src/runtime.rs`, `src/runtime/task_state/mod.rs`, `Cargo.toml`
- Generated: `schemas/working_set.schema.json` from `schema_for!(WorkingSetRecord)`

### Acceptance tests

- `working_set_record_round_trips_through_persist`
- `working_set_schema_matches_checked_in_file`
- `state_files_skip_working_set_sidecar`
- `token_count_is_not_byte_quarter_heuristic`

Crate APIs used (docs.rs only): `tiktoken::get_encoding`, `tiktoken::encoding_for_model`, `CoreBpe::count`; `schemars::JsonSchema`, `schemars::schema_for!`.


## Phase 2 — hydrate on resume and compact
## Phase 3 — hierarchical instruction loading
## Phase 4 — reviewable memory candidates
## Phase 5 — peer join merge
