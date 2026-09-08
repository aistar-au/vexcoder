# Task PN-01: Durable Working-Set Record

**Target files:**
- `src/runtime/task_state/` — persist and load the working-set record
- `src/runtime/task_document/` — project the record from pulse evidence
- `src/app/pulse.rs` — load `WorkingSetRecord` into the next request on `/resume` instead of clearing the live `ApiMessage` window
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

**Status:** Merged in PR #444. Do not fold later phases into this change.

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


## Phase 2 — seed the next request from `WorkingSetRecord` on `/resume` and `/compact`

**Status:** Batch 3 (this PR). Do not fold `loro` / peer join into this change.

### Net change

| Surface | Retained API | Superseded API | Added API |
| :--- | :--- | :--- | :--- |
| `/resume` TUI restore | `TaskDocumentCondenser::restore_from_snapshot` and `task_state_bridge.rs` on-screen projection | `TuiMode::apply_resumed_task` calling `reset_conversation_window` → `RuntimeContext::clear_conversation` → `ConversationManager::clear_messages` (`api_messages.clear()`) with no continuity source | `WorkingSetRecord::load_from_search_dirs_from` then `ConversationManager::seed_from_working_set` copies `WorkingSetRecord::as_prompt_block` onto `ApiClient::set_supplementary_system_prompt` |
| `/compact` | `ContextCompactionRecord` append, `completed_turns.clear()`, `persist_task_document`, task id and grants | `handle_compact_command` calling `reset_conversation_window` after clearing pulses, so the next request has an empty `ApiMessage` window | `TaskDocumentCondenser::write_working_set` before `completed_turns.clear()`; then `seed_from_working_set` so the next request carries the sidecar |
| Conversation window | `reset_conversation_window` on `/new` and `/fork` | Using that helper as the `/resume` and `/compact` path | `TuiMode::reset_session_surface` for TUI chrome; `/resume` and `/compact` no longer call `reset_conversation_window` |
| Next request assembly | `RuntimeContext::start_turn_with_system_prompt` / `set_runtime_prompt` | Passing `None` and overwriting a previously set supplementary prompt | `set_runtime_prompt` merges `ConversationManager::working_set_prompt_block` with any extra coding prompt |
| Condenser write | `TaskDocumentCondenser::persistable_snapshot` for `{id}.json` | No writer for `{id}.working-set.json` at compact/turn-complete | `project_working_set` / `write_working_set` (sole writer) on compact (before pulse clear) and on `commit_completed_turn` |

### Files

- Updated: `src/app/pulse.rs`, `src/app/commands/session.rs`, `src/app/runtime_build.rs`, `src/runtime/context.rs`, `src/state/conversation/state.rs`, `src/runtime/task_document/task_state_bridge.rs`, `src/runtime/task_state/working_set.rs`, `src/app/tests/session/compact.rs`, `src/app/tests/session/mod.rs`, `docs/src/commands.md`
- Unchanged in this batch: `src/state/conversation/history.rs` (local byte heuristic is a later rewrite), `loro`, `MemoryCandidate` attachment to the record

### Acceptance tests

- `resume_injects_working_set_into_next_request`
- `compact_writes_working_set_before_clearing_pulses`

Crate APIs used (docs.rs only): `ApiClient::set_supplementary_system_prompt`; `WorkingSetRecord::{save,load,as_prompt_block}`; `TaskDocumentCondenser::{project_working_set,write_working_set}`; `ConversationManager::{clear_messages,seed_from_working_set}`.


## Phase 3 — hierarchical instruction loading

**Status:** Merged in PR #445. Do not fold `WorkingSetRecord` restore on `/resume` or `loro` into this change.

### Net change

| Surface | Stays | Removed | Inserted |
| :--- | :--- | :--- | :--- |
| Candidate names | Three-name list `.vex/AGENTS.md`, `AGENTS.md`, `.vex/PROJECT.md` and same-directory first match | Single-directory loader that returned `OverBudget` and stopped the walk | Root-to-leaf walk, one file per directory, closer files layered after farther ones |
| Over-budget file | Per-file token count via `token_count` | Fail-closed `LoadResult::OverBudget` that dropped the whole instruction layer | Skip that file, record it in the manifest, continue to the next directory |
| Call sites | `build_facade_client`, `resolve_batch_project_instructions` | Match on `LoadResult::{Loaded, OverBudget, NotFound}` | `load_instructions_for_workspace` + `InstructionSet` content/manifest |
| `/context` | Session, git, token summary | Inferring skipped files from prompt size; a second disk walk at `/context` time | Session-start `InstructionSet` manifest copied from `build_facade_client` (loaded vs skipped with token counts) |

### Files

- Updated: `src/runtime/project_instructions.rs`, `src/app/facade.rs`, `src/batch_mode.rs`, `src/app.rs`, `src/app/ctor.rs`, `src/app/runtime_build.rs`, `src/app/commands/run.rs`
- Types inserted: `InstructionSource`, `InstructionSet`, `load_hierarchical_instructions`, `load_instructions_for_workspace`

### Acceptance tests

- `instruction_walk_falls_back_when_higher_file_is_over_budget`
- `instruction_walk_layers_root_before_leaf`
- Existing same-directory priority / fallback tests kept against `InstructionSet`

## Phase 4 — reviewable memory candidates

**Status:** Merged in PR #445. Do not attach candidates to `WorkingSetRecord` in this change (condenser/resume is Phase 2 / Batch 3).

### Net change

| Surface | Stays | Removed | Inserted |
| :--- | :--- | :--- | :--- |
| Notes path | `notes_path` / XDG `memory.md` resolution | Whole-file all-or-nothing injection | Typed `MemoryCandidate` store (`source`, `topic`, `body`, `status`, `source_reference`) |
| Provenance | Markdown `[auto]` tag as a projection | Treating auto-extracted and operator notes as equal prompt text | `CandidateSource::{User, Feedback, Project, Reference}` + `CandidateStatus::{Pending, Accepted}` |
| Injection | Token budget call site in `resolve_notes_for_injection` | Silent skip of the entire notes file | Only `Accepted` inject; over budget drops lowest-priority accepted first |
| Operator commands | `/memory`, `/memory add`, `/memory clear`, `/memory auto *` | Auto extract writing `Accepted` prompt text | `/memory accept <n\|topic>`; auto extract writes `Feedback` + `Pending`; add writes `User` + `Accepted` |
| Persistence | Markdown notes file for existing readers | Markdown as the only durable unit | JSON sidecar `memory.candidates.json` via `write_json_safe` (no durable-access assert; path is user-config) |
| Schema | `schemars` already in-tree from Phase 1 | Ad-hoc notes JSON | `schemas/memory_candidates.schema.json` from `schema_for!(MemoryCandidateStore)` |

### Files

- Inserted: `src/runtime/memory_candidates.rs`, `schemas/memory_candidates.schema.json`
- Updated: `src/session_notes.rs`, `src/auto_memory.rs` (extract helpers stay; TUI writes through the store), `src/app/commands/memory.rs`, `src/app/slash_commands.rs`, `src/app/commands/mod.rs`, `src/app/pulse.rs`, `src/app/tests/memory.rs`, `src/runtime.rs`, `Cargo.toml`

### Acceptance tests

- `pending_memory_candidates_are_not_injected`
- `pending_auto_notes_are_not_injected_from_markdown`
- `memory_candidates_schema_matches_checked_in_file`
- `memory_accept_promotes_pending_candidate`

Crate APIs used (docs.rs only): `schemars::JsonSchema`, `schemars::schema_for!`.

## Phase 5 — peer join merge
