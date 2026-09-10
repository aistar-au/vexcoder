# ADR-051: Durable Working-Set Record and Context Continuity

**Status:** Accepted (Phases 1–5 on `main`; Phase 5 merged in PR #447)
**Chain:** ADR-023, ADR-024, ADR-029, ADR-033, ADR-038, ADR-045, ADR-046, ADR-049
**Implementation checklist:** `TASKS/PN-01-working-set-record.md`

## Context

Context sources were fragmented across a saved task document, an on-screen
TUI projection, a pulse-evidence snapshot, a peer JSONL log, and a flat
notes file. A saved task could restore the visible TUI surface via
`TaskDocumentCondenser::restore_from_snapshot`, but
`TuiMode::apply_resumed_task` previously called `reset_conversation_window`
after rebuilding the task document, which cleared the live `ApiMessage`
history. `/compact` did the same after clearing completed pulses. The
snapshot bridge in `task_state_bridge.rs` is presentation-oriented — pulse
input, final text, changed files, tool summaries — not a serialized model
conversation, so it cannot stand in as the resume source by itself.

Three heuristics carried more weight than they should:

- **Token budgeting** was `content.len() / 4` in both
  `session_notes::resolve_notes_load` and
  `project_instructions::estimate_tokens`. A byte count is not a token
  count for any BPE vocabulary, so the budget check itself was
  approximate exactly where it decided whether something survived.
- **Project instructions** (`load_project_instructions`) checked a fixed
  three-name list — `.vex/AGENTS.md`, `AGENTS.md`, `.vex/PROJECT.md` — in
  a single directory and stopped at the first match. If that first match
  was over budget, the loader returned `OverBudget` immediately; it never
  tried the next candidate, and it never looked at a parent or child
  directory.
- **Memory** was one file, copied into the prompt whole or skipped whole.
  There was no scope, provenance, or accepted/pending state, so a single
  stale line and a single verified fact were trusted equally, or dropped
  equally.

A notes-file fingerprint refresh (`ApiClient` notes storage plus per-pulse
reload) was evaluated as an early continuity protocol and closed without
merging. That path does not restore model working state on `/resume`, and
later phases must not grow a second continuity protocol beside
`WorkingSetRecord`. Phases 1 (schema, persist, `tiktoken`), 2 (`WorkingSetRecord`
restore on `/resume` and `/compact`), 3 (hierarchical instructions), 4
(`MemoryCandidate`), and 5 (`JoinIndex` + `StateEnvelope` at `{id}.join.json`)
are on `main` (PRs #444–#447). Do not reintroduce `loro`, `PeerMergeDoc`, or
`{id}.channel.crdt`.

## Crate and API Evidence

Three upstream crates close the gaps above with primitives this codebase
did not previously have, and one class of managed provider API is
evaluated and rejected on the same grounds ADR-023/024 already used to
prefer local, inspectable state.

- **Token counting — `tiktoken` 4.1.2.** A pure-Rust byte-pair-encoding
  tokenizer. Published signatures (`docs.rs/tiktoken/4.1.2`):
  `tiktoken::get_encoding(name: &str) -> Option<&'static CoreBpe>` and
  `tiktoken::encoding_for_model(model: &str) -> Option<&'static CoreBpe>`.
  An encoding whose vocabulary is not compiled in is absent:
  `get_encoding` returns `None`. `CoreBpe::count(&self, text: &str) -> usize`
  returns a token count on a path that does not allocate the token-id
  vector `encode` would produce; `count_with_special_tokens` extends that
  to text containing special tokens. This crate's default encoding is
  `o200k_base`, compiled in via `default-features = false` plus
  `features = ["vocab-o200k_base"]`. Source: docs.rs/tiktoken.

- **Schema validation — `schemars` 1.2.2.** `#[derive(JsonSchema)]` plus
  the `schema_for!($type:ty)` macro generate a `Schema` document from a
  Rust type (JSON Schema 2020-12). schemars reads a type's `#[serde(...)]`
  attributes so the schema matches what `serde_json` actually produces.
  This gives `WorkingSetRecord`, `MemoryCandidateStore`, and `JoinIndex`
  a checked contract instead of a hand-maintained one, without changing
  how the record is written to disk. Source: docs.rs/schemars/1.2.2.

- **Peer-join merge — `JoinIndex` / `StateEnvelope`.** Production join
  is a single-process orchestrator: `poll_fan_out_join` decides each
  child's replace set, `apply_join_outcome` posts those entries into one
  typed JSON `JoinIndex`, and `JoinIndex::save` persists
  `.vex/state/{task_id}.join.json` through `write_json_safe` /
  `assert_durable_access`. Message-id `supersedes` is the replace rule;
  `live_entries` drops any id that appears in another entry's
  `supersedes` list. `StateEnvelope` is the internal read API so
  consumers do not open `{id}.json`, `{id}.working-set.json`, or
  `{id}.join.json` directly. GET `/v1/tasks/{task_id}/working-set`
  returns the envelope (the only HTTP read of live join ids /
  `supersedes`); GET `/v1/tasks/{task_id}/join-status` returns agent
  summaries from `facade_poll_join`, not the raw index. A CRDT
  (`loro` / `PeerMergeDoc` / `{id}.channel.crdt`) is out of the join
  surface: there is no second writer, and the `loro` graph failed
  `cargo deny` (MPL-2.0). ADR-046 JSONL `PeerMessage` remains the
  inter-agent log.

- **Rejected: opaque provider-side compaction.** Some managed chat/response
  APIs pair a create call with a server-side compaction operation and a
  continuation identifier for the previous turn; the compacted result
  comes back as an encrypted item that is explicitly not meant to be read
  outside that vendor's own runtime. That is a reasonable design for a
  vendor-hosted conversation store, but it is the wrong fit here: this
  project keeps its working context in a local, `serde_json`-readable
  file precisely so a task can be resumed, exported, or replayed without
  depending on one vendor's store staying reachable. A local
  `WorkingSetRecord` is the fallback that makes resume independent of any
  such item, per ADR-049's existing local-first framing for the
  shared-prefix cache contract.

- **Existing dependency, reframed — `ratatui`.** Ratatui renders in
  immediate mode with an intermediate buffer: on every frame the
  application supplies the full view from its own state, and the library
  does not retain that view between frames. That is exactly why the
  on-screen transcript cannot be the resume source (ADR-045 already
  reaches the same conclusion for snapshot-based replay) — the view is a
  projection of state the application already owns, not a second copy of
  that state.

## Retained / Superseded / Added

| Area | Retained API | Superseded API | Added API |
| :--- | :--- | :--- | :--- |
| Working-set restore on `/resume` | `TaskDocumentCondenser::restore_from_snapshot` and the on-screen projection in `task_state_bridge.rs` | `TuiMode::apply_resumed_task` and `handle_compact_command` calling `reset_conversation_window` → `RuntimeContext::clear_conversation` → `ConversationManager::clear_messages` (`api_messages.clear()`) with no continuity source | `WorkingSetRecord::{save,load,try_load,try_load_from_search_dirs_from,as_prompt_block,retain_durable_objective}`; `ConversationManager::seed_from_working_set` copies `as_prompt_block` onto `ApiClient::set_supplementary_system_prompt`; `TaskDocumentCondenser::write_working_set` is the sole sidecar writer. `reset_conversation_window` remains on `/new` and `/fork` |
| Memory / notes | `notes_path` resolution and the markdown projection of the store | Flat file as the only durable unit, copied into the prompt whole or skipped whole | Typed `MemoryCandidate` / `MemoryCandidateStore` (`source`, `topic`, `body`, `status`, `source_reference`); `inject_accepted` copies only `CandidateStatus::Accepted`; over budget, the lowest-priority accepted candidate is dropped first |
| Token budgeting | The budget-check call sites in `session_notes` and `project_instructions` | `content.len() / 4` and `(len + 3) / 4` | `tiktoken::get_encoding` / `encoding_for_model` (`Option<&'static CoreBpe>`) and `CoreBpe::count` via `src/runtime/token_count.rs` |
| Project instructions | The three-name candidate list and its same-directory priority order | Single-directory, first-match, fail-closed `LoadResult::OverBudget` | Root-to-leaf `load_hierarchical_instructions` / `load_instructions_for_workspace`; one candidate per directory; closer files layered after farther ones; `InstructionSet` manifest records skipped files |
| Peer channel | ADR-046 JSONL sidecar (`append_message` / `read_messages`), two-layer locking, HTTP POST/GET `/v1/tasks/{id}/messages` | `SubtaskOrchestrator::apply_join_outcome` concatenating every child `handoff_summary` with `join("\n")`; empty `JoinSummary.supersedes` on `poll_fan_out_join`; `facade_poll_join` returning child tuples without `apply_join_outcome`; `PeerMergeDoc` / `loro` / `{id}.channel.crdt` | `JoinIndex` typed JSON (`schemars`) at `{id}.join.json`. `poll_fan_out_join` writes `JoinSummary.supersedes` (spawn-declared `SessionTask.supersedes` plus same-agent earlier completions). `facade_poll_join` calls `apply_join_outcome` when no session-task remains live. `handoff_summary` from `live_entries` only. `TaskDocumentCondenser::record_join_evidence` writes `RecordedDecision.source_reference` as the join message id. `StateEnvelope` + GET `/v1/tasks/{id}/working-set` |
| API caching | ADR-049's shared-prefix fingerprint and `ApiMessage.cache_hint` | Nothing; provider-specific mapping stays deferred | The working-set record is the local fallback, so `/resume` never depends on an opaque provider continuation item |

## Decision

### 1. `WorkingSetRecord` with a generated schema

See `src/runtime/task_state/working_set.rs`. Persist is `save` / `load` /
`try_load` / `try_load_from_search_dirs_from` through `write_json_safe`.
`retain_durable_objective` keeps the first non-empty `objective`.
`as_prompt_block` is the resume seed. A test regenerates
`schema_for!(WorkingSetRecord)` against `schemas/working_set.schema.json`.

### 2. Real token counts for every budget check

`tiktoken` 4.1.2: `encoding_for_model` / `get_encoding` return
`Option<&'static CoreBpe>`. `CoreBpe::count` is the zero-allocation path.
Wrapper: `src/runtime/token_count.rs`. Default encoding `o200k_base`.

### 3. Hierarchical instruction loading

Walk repository root to cwd. Same three candidate names per directory.
Root-first concatenate; skip over-budget files and record them in the
`InstructionSet` manifest. Token counts use `token_count`.

### 4. Typed, reviewable memory candidates

`MemoryCandidate` + `MemoryCandidateStore`. Only `Accepted` candidates
inject. Pending needs `/memory accept`. Over budget, drop lowest-priority
accepted first.

### 5. Agent-join merge (`JoinIndex` / `StateEnvelope`)

JSONL `PeerMessage` append/read (ADR-046) stays as the inter-agent log.
Join merge uses one typed JSON `JoinIndex` per parent task at
`.vex/state/{task_id}.join.json`. Production join is a single-process
orchestrator; a CRDT is not required.

**Production call graph.** Join has three production writers of
`SessionTask` rows, one production writer of `JoinSummary.supersedes`,
and one production caller of `apply_join_outcome`:

| Function | Role | Replace set |
| :--- | :--- | :--- |
| `SubtaskOrchestrator::schedule_team` | Inserts fan-out members, or the first sequential member | `stamp_join_supersedes(..., sequential)` |
| `SubtaskOrchestrator::advance_sequential` | Inserts the next sequential member after no prior member remains live | `stamp_join_supersedes(..., true)` — every earlier `SessionTask.id` |
| `facade_delegate_session_task` | Inserts a single-agent child | `stamp_join_supersedes(..., false)` — earlier tasks of the same `agent_id` |
| `poll_fan_out_join` | Sole production writer of `JoinSummary.supersedes` | Spawn stamp union same-agent earlier `Completed` rows that have `handoff_summary` |
| `facade_poll_join` | Sole production caller of `apply_join_outcome` | HTTP `join_status_handler`; `/watch` in `commands/info.rs` (two sites) |

`SessionTask::new` does not choose a replace set. Independent fan-out
members of different agents list none, so both summaries remain live.
`TeamScheduler` is not stored on `TaskState`; sequential versus fan-out
intent is the spawn stamp on `SessionTask.supersedes`.

**Why `PeerMessageKind` is not the join replace rule.** ADR-046 JSONL is a
live inter-agent log keyed per `session_task_id`. Join reads completed
`SessionTask.handoff_summary` on the parent task. A `Correction` row on
the JSONL log is not a child handoff and is not posted into `JoinIndex`.

**Why this crate does not depend on `loro`.** A first Batch 4 sketch
wrapped `LoroDoc` as `PeerMergeDoc` and persisted `{id}.channel.crdt`.
That path is rejected: production join never has a second writer, and the
`loro` graph failed `cargo deny` (MPL-2.0). Persist is `JoinIndex::save`
to `{id}.join.json`.

**Do not reintroduce these join-surface items.**

| Item | Why it looked load-bearing | This crate |
| :--- | :--- | :--- |
| `PeerMergeDoc` / `loro` / `{id}.channel.crdt` | Early Batch 4 CRDT sketch | Removed. Typed JSON `JoinIndex` is the join document. |
| Empty `JoinSummary.supersedes` on `poll_fan_out_join` | First Batch 4 revision always wrote `Vec::new()` | Removed. Poll is the production writer of the replace set. |
| `facade_poll_join` returning child tuples without `apply_join_outcome` | First Batch 4 revision. HTTP `/watch` never posted | Removed. `facade_poll_join` calls `apply_join_outcome` when no session-task remains live. |
| `PeerMessageKind` as the join replace rule | ADR-046 JSONL kinds look like a conflict protocol | Not used. Join reads `SessionTask.handoff_summary`. |

JSONL ADR-046 routes stay (`append_message`, `read_messages`, HTTP
POST/GET `/v1/tasks/{id}/messages`).

`JoinIndex::post` inserts or updates by message id and unions
`supersedes`. `live_entries` returns entries whose ids are not listed in
any `supersedes` array. `SubtaskOrchestrator::apply_join_outcome` posts
each `JoinSummary`, persists `{task_id}.join.json` via `write_json_safe`,
sets `TaskState.handoff_summary` from live entries only, and calls
`TaskDocumentCondenser::record_join_evidence`. `StateEnvelope::load_for_task`
is the internal read of both sidecars.

## Pros and Cons

**Pros**

- The record stays a local, `serde_json`-readable file with a schema
  checked in CI, so resuming a task never depends on one vendor's store.
- Budget checks measure what the context window actually measures.
- A skipped instruction file no longer takes the rest of the instruction
  layer with it, and the manifest makes the skip visible.
- Peer joins get an explicit conflict rule instead of implicit concatenation.

**Cons**

- `ConversationManager` now holds two projections in parallel — the raw
  `ApiMessage` history for the current pulse and the `WorkingSetRecord`
  for continuity — and keeping them from drifting apart is an ongoing
  cost, not a one-time one.
- The accepted/pending split on memory candidates adds a manual step.
- Join is not multi-writer. Concurrent external writers would need a
  later ADR; `JoinIndex` is one in-process document keyed by message id.
- Two additional dependencies (`tiktoken`, `schemars`).

## Alternatives Considered

- **Keep the byte-count budget heuristic.** Rejected.
- **Hand-roll a token estimator instead of adopting `tiktoken`.** Rejected.
- **Hand-write the JSON Schema for `WorkingSetRecord`.** Rejected.
- **Keep free-text summary concatenation on peer join.** Rejected.
- **Adopt `loro` / `PeerMergeDoc` / `{id}.channel.crdt` as the join document.** Rejected: no second writer, typed JSON is inspectable, `loro` failed `cargo deny` (MPL-2.0).
- **Wrap `ExportMode::updates` / `import` / `oplog_vv` as the join surface.** Rejected.
- **Drive join supersession from `PeerMessageKind`.** Rejected.
- **Leave `JoinSummary.supersedes` empty on `poll_fan_out_join`.** Rejected.
- **Adopt opaque provider-side compaction as the primary continuity unit.** Rejected.
- **Load a single project-instructions file, as previously.** Rejected.
- **Treat the on-screen TUI projection as the resume source.** Rejected.
- **Notes-file fingerprint refresh as the continuity protocol.** Rejected.

## Validation

Phases 1–5 are on `main` (PR #444, PR #445, PR #446, PR #447).

- `working_set_record_round_trips_through_persist`
- `working_set_schema_matches_checked_in_file`
- `token_count_is_not_byte_quarter_heuristic`
- `get_encoding_o200k_base_count_matches_encode_len`
- `resume_restores_working_set_into_next_request`
- `compact_writes_working_set_before_clearing_pulses`
- `write_working_set_keeps_prior_objective`
- `compact_retains_objective_across_later_writes`
- `resume_surfaces_corrupt_working_set_without_dropping_task`
- `instruction_walk_falls_back_when_higher_file_is_over_budget`
- `pending_memory_candidates_are_not_injected`
- `join_applies_supersession_instead_of_concatenating_summaries`
- `live_entries_drop_superseded_ids`
- `snapshot_round_trips_through_persist`
- `repost_unions_supersedes_and_keeps_latest_body`
- `join_index_schema_matches_checked_in_file`
- `poll_fan_out_join_decides_sequential_supersession`
- `poll_fan_out_join_keeps_independent_fan_out_summaries`
- `poll_fan_out_join_same_agent_later_completion_replaces_earlier`
- `facade_poll_join_applies_live_handoff_and_drops_superseded_summaries`

Alongside those, a schema-diff step regenerates
`schema_for!(WorkingSetRecord)`, `schema_for!(MemoryCandidateStore)`,
and `schema_for!(JoinIndex)` and fails if the checked-in schema files
do not match.

## References

- `tiktoken` 4.1.2 — <https://docs.rs/tiktoken/4.1.2>
- `schemars` 1.2.2 — <https://docs.rs/schemars/1.2.2>
- `JoinIndex` / `StateEnvelope` — typed JSON join sidecar at `{id}.join.json`; schema at `schemas/join_index.schema.json`
- `ratatui` — <https://docs.rs/ratatui>
- Internal: `TASKS/PN-01-working-set-record.md`, ADR-045, ADR-046, ADR-049
