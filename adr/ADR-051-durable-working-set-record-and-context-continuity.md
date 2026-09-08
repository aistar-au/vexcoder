# ADR-051: Durable Working-Set Record and Context Continuity

**Status:** Accepted (Phases 1–4 on `main`; Phase 5 in this batch)
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
restore on `/resume` and `/compact`), 3 (hierarchical instructions), and 4
(`MemoryCandidate`) are on `main`. Phase 5 (this batch) is the `loro` peer-join
merge.

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
  This gives `WorkingSetRecord` and `MemoryCandidateStore` a checked
  contract instead of a hand-maintained one, without changing how the
  record is written to disk. Source: docs.rs/schemars/1.2.2.

- **Peer-channel merge — `loro`.** A CRDT (conflict-free replicated data
  type) document library. `LoroDoc::new()` creates a document;
  `get_map`/`get_list`/`get_text` attach named containers to it;
  `set_peer_id` gives each writer a stable identifier;
  `export(ExportMode::updates(&version_vector))` plus `import` exchange
  only the operations a peer is missing rather than a full log replay;
  and concurrent edits to the same container merge without a designated
  writer. This gives the peer channel a real merge rule (supersede by
  message id, apply in causal order) instead of concatenating every child
  `handoff_summary`. Implemented as `PeerMergeDoc` in this batch. Source:
  docs.rs/loro/1.16.0.

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
| Peer channel | ADR-046 JSONL sidecar (`append_message` / `read_messages`), two-layer locking, HTTP POST/GET `/v1/tasks/{id}/messages` | `SubtaskOrchestrator::apply_join_outcome` concatenating every child `handoff_summary` with `join("\n")` | `PeerMergeDoc` (`LoroDoc::new`, `set_peer_id`, `get_map`, `insert_container`, `export(ExportMode::Snapshot \| updates)`, `import`, `oplog_vv`). Join posts `JoinSummary` entries, keeps `live_entries` after message-id supersession, and `TaskDocumentCondenser::record_peer_join_evidence` writes `RecordedDecision.source_reference` as the CRDT message id. Snapshot path: `{task_id}.channel.crdt` |
| API caching | ADR-049's shared-prefix fingerprint and `ApiMessage.cache_hint` | Nothing; provider-specific mapping stays deferred | The working-set record is the local fallback, so `/resume` never depends on an opaque provider continuation item |

## Decision

### 1. `WorkingSetRecord` with a generated schema

```rust
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Durable unit of continuity, persisted under
/// `.vex/state/{task_id}.working-set.json` via `write_json_safe` and
/// `assert_durable_access`. The condenser is the sole writer.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkingSetRecord {
    pub schema_version: u32,
    pub updated_at: DateTime<Utc>,
    pub objective: String,
    pub constraints: Vec<String>,
    pub decisions: Vec<RecordedDecision>,
    pub changed_paths: Vec<PathChange>,
    pub verified_results: Vec<String>,
    pub unresolved_questions: Vec<String>,
    pub active_plan: String,
    pub next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecordedDecision {
    pub rationale: String,
    /// File location or a peer-channel message id.
    pub source_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PathChange {
    pub path: PathBuf,
    pub git_identity: String,
}
```

Persist and restore (implemented):

- `save` / `load` write and read the sidecar through `serde_json`.
- `try_load` / `try_load_from_search_dirs_from` return `Ok(None)` when the
  sidecar is absent and `Err` when a present file fails durable-access,
  read, or `serde_json` deserialize.
- `retain_durable_objective` keeps the first non-empty `objective` across
  later condenser writes. Episodic fields (`verified_results`,
  `changed_paths`, `next_action`) still come from the current pulse window.
- `as_prompt_block` serializes the `serde` `snake_case` field names for
  `ApiClient::set_supplementary_system_prompt`.

A test regenerates `schema_for!(WorkingSetRecord)` and fails if the
checked-in `schemas/working_set.schema.json` has drifted, so the on-disk
contract cannot go stale without a reviewed change.

### 2. Real token counts for every budget check

Published `tiktoken` 4.1.2 API (`docs.rs/tiktoken`): both lookup functions
return `Option<&'static CoreBpe>`, not `Result` and not an owned
`CoreBpe`. `CoreBpe::count` is the zero-allocation path.

```rust
use tiktoken::CoreBpe;

/// Replaces the `content.len() / 4` estimate previously used in
/// `session_notes` and `project_instructions`.
fn token_count(encoding: &CoreBpe, text: &str) -> usize {
    encoding.count(text) // zero-allocation path; no token vector built
}

fn encoder_for_model(model_name: &str) -> &'static CoreBpe {
    tiktoken::encoding_for_model(model_name).unwrap_or_else(|| {
        tiktoken::get_encoding("o200k_base").expect("bundled vocabulary")
    })
}
```

The encoder is cheap to hold for the life of a `ConversationManager` and
reused across pulses rather than rebuilt per call. The in-tree wrapper is
`src/runtime/token_count.rs`.

### 3. Hierarchical instruction loading

Walk from the repository root to the working directory. At each
directory, try the same fixed candidate names the previous
single-directory loader checked. Concatenate root-first so closer files
override farther ones. A file that does not fit the remaining budget is
skipped and recorded in a manifest; the walk continues instead of
stopping. Token counts use `token_count` (default `o200k_base` encoder).

```rust
use std::path::{Path, PathBuf};

const CANDIDATE_FILES: &[&str] = &[".vex/AGENTS.md", "AGENTS.md", ".vex/PROJECT.md"];

pub struct InstructionSource {
    pub path: PathBuf,
    pub included: bool,
    pub estimated_tokens: usize,
}

pub struct InstructionSet {
    pub content: String,
    pub manifest: Vec<InstructionSource>,
}

pub fn load_hierarchical_instructions(
    repo_root: &Path,
    cwd: &Path,
    token_budget: usize,
) -> InstructionSet {
    let mut sections = Vec::new();
    let mut manifest = Vec::new();
    let mut remaining = token_budget;

    for dir in directories_root_to_leaf(repo_root, cwd) {
        let Some((path, content)) = first_existing(&dir, CANDIDATE_FILES) else {
            continue;
        };
        let estimated = crate::runtime::token_count::token_count(&content);
        let included = estimated <= remaining;
        if included {
            remaining -= estimated;
            sections.push(content);
        }
        manifest.push(InstructionSource { path, included, estimated_tokens: estimated });
    }

    InstructionSet { content: sections.join("\n\n"), manifest }
}
```

`/context` renders the session-start `InstructionSet` manifest so an
operator can see which files loaded and which were skipped for budget,
rather than inferring it from prompt size or walking the disk again.

### 4. Typed, reviewable memory candidates

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    User,
    Feedback,
    Project,
    Reference,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Pending,
    Accepted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryCandidate {
    pub source: CandidateSource,
    pub topic: String,
    pub body: String,
    pub status: CandidateStatus,
    pub source_reference: String,
}
```

Only `Accepted` candidates are read into the prompt, via
`inject_accepted`. `Pending` candidates need an operator `/memory accept`
before use. Over budget, the lowest-priority accepted candidate is
dropped first; nothing is silently skipped in bulk the way a single
over-budget file was previously.

### 5. Peer-join merge (`PeerMergeDoc`)

JSONL `PeerMessage` append/read (ADR-046) stays. Join merge uses `loro`
1.16 (`docs.rs/loro`):

```rust
use anyhow::Result;
use loro::{ExportMode, LoroDoc, LoroList, LoroMap, VersionVector};

pub struct PeerMergeDoc {
    doc: LoroDoc,
}

impl PeerMergeDoc {
    pub fn new(peer_id: u64) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer_id)?; // LoroResult; PeerID is u64
        Ok(Self { doc })
    }

    pub fn post(&self, id: &str, agent_id: &str, body: &str, supersedes: &[String]) -> Result<()> {
        let messages = self.doc.get_map("messages");
        let entry = messages.insert_container(id, LoroMap::new())?;
        entry.insert("body", body)?;
        entry.insert("agent_id", agent_id)?;
        let list = entry.insert_container("supersedes", LoroList::new())?;
        for (index, superseded_id) in supersedes.iter().enumerate() {
            list.insert(index, superseded_id.as_str())?;
        }
        self.doc.commit();
        Ok(())
    }

    pub fn export_updates_since(&self, since: &VersionVector) -> Result<Vec<u8>> {
        Ok(self.doc.export(ExportMode::updates(since))?)
    }

    pub fn import_updates(&self, bytes: &[u8]) -> Result<()> {
        self.doc.import(bytes)?;
        Ok(())
    }
}
```

`live_entries` returns map entries whose ids are not listed in any
`supersedes` array. `SubtaskOrchestrator::apply_join_outcome` posts each
`JoinSummary`, persists `ExportMode::Snapshot` to
`.vex/state/{task_id}.channel.crdt` via `write_bytes_safe`, sets
`TaskState.handoff_summary` from live entries only, and calls
`TaskDocumentCondenser::record_peer_join_evidence` so the condenser remains
the sole writer of `{task_id}.working-set.json`. The phase 5 anchor test is
`join_applies_supersession_instead_of_concatenating_summaries`.
`poll_fan_out_join` still reports `all_done` when no session task remains
live; the merge rule is supersession, not concatenation.

## Pros and Cons

**Pros**

- The record stays a local, `serde_json`-readable file with a schema
  checked in CI, so resuming a task never depends on one vendor's store.
- Budget checks measure what the context window actually measures,
  removing a class of bugs where a notes file or instruction file is
  skipped (or wrongly kept) purely because a byte estimate was off.
- A skipped instruction file no longer takes the rest of the instruction
  layer with it, and the manifest makes the skip visible.
- Peer joins get an explicit conflict rule instead of an implicit one
  ("last full summary wins because we waited for everyone").

**Cons**

- `ConversationManager` now holds two projections in parallel — the raw
  `ApiMessage` history for the current pulse and the `WorkingSetRecord`
  for continuity — and keeping them from drifting apart is an ongoing
  cost, not a one-time one.
- The accepted/pending split on memory candidates adds a manual step
  compared to the previous silent whole-file copy into the prompt.
- `loro` is a new, non-trivial dependency; the peer channel's merge logic
  moves from "append and read" to a real CRDT surface, which is a larger
  change to reason about and test than the JSONL sidecar it replaces.
- Three additional dependencies (`tiktoken`, `schemars`, `loro`) each
  bring their own version and vocabulary-data footprint; the tokenizer's
  per-encoding feature flags keep this bounded but do not remove it.

## Alternatives Considered

- **Keep the byte-count budget heuristic.** Rejected: it is cheap but
  wrong in a way that compounds — the same estimate gates both the notes
  file and every instruction candidate, so one bad estimate can both
  over-admit and under-admit content in the same pulse.
- **Hand-roll a token estimator instead of adopting `tiktoken`.** Rejected:
  matching a real BPE vocabulary by hand duplicates work an existing,
  benchmarked crate already does, for a worse and unmaintained result.
- **Hand-write the JSON Schema for `WorkingSetRecord` instead of deriving
  it.** Rejected: a hand-written schema drifts from the Rust type
  silently; `schemars` ties the two together and a CI diff check catches
  drift immediately.
- **Keep free-text summary concatenation on peer join.** Rejected: it has
  no conflict rule, so two children editing related state produce a
  summary that is only as good as whichever text happened to be
  concatenated last.
- **Adopt opaque provider-side compaction as the primary continuity
  unit.** Rejected for the reasons in Crate and API Evidence above: it
  trades away local inspection and portability for a convenience this
  project does not need, having already chosen a local-first design in
  ADR-023 and ADR-038.
- **Load a single project-instructions file, as previously.** Rejected: an
  over-budget file silently disables the whole layer, and a single file
  cannot express directory-scoped conventions in a larger repository.
- **Treat the on-screen TUI projection as the resume source.** Rejected:
  ratatui's immediate-mode rendering means that view is reconstructed
  from application state every frame and is not itself a second copy of
  that state — ADR-045 reaches the same conclusion for snapshot replay.
- **Notes-file fingerprint refresh as the continuity protocol.** Rejected:
  re-reading a flat notes file before each pulse keeps that file current
  in the system prompt; it does not restore model working state on
  `/resume` or `/compact`. Phase 4 typed candidates cover the notes
  problem. Do not grow a second continuity protocol beside
  `WorkingSetRecord`.

## Validation

Phases 1–4 are on `main` (PR #444, PR #445, PR #446). Phase 5 is this
batch.

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
- `export_updates_import_on_second_peer`

Alongside those, a schema-diff step regenerates
`schema_for!(WorkingSetRecord)` and `schema_for!(MemoryCandidateStore)`
and fails if the checked-in schema files do not match, so the persisted
contract cannot change without a reviewed diff.

## References

- `tiktoken` 4.1.2 — `get_encoding` / `encoding_for_model` return
  `Option<&'static CoreBpe>`; `CoreBpe::count` is the zero-allocation
  path: <https://docs.rs/tiktoken/4.1.2>
- `schemars` 1.2.2 — JSON Schema 2020-12 generation from Rust types via
  `#[derive(JsonSchema)]` and `schema_for!`:
  <https://docs.rs/schemars/1.2.2>
- `loro` 1.16.0 — `LoroDoc::{new,set_peer_id,get_map,export,import,oplog_vv,from_snapshot}`,
  `ExportMode::{Snapshot,updates}`, `LoroMap::insert_container`,
  `LoroList::insert`: <https://docs.rs/loro/1.16.0>
- `ratatui` — immediate-mode rendering with intermediate buffers
  (existing project dependency, cited here for the resume-source
  rationale): <https://docs.rs/ratatui>
- Internal: `TASKS/PN-01-working-set-record.md`, ADR-045, ADR-046,
  ADR-049
