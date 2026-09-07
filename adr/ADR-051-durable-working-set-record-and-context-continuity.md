# ADR-051: Durable Working-Set Record and Context Continuity

**Status:** Proposed
**Chain:** ADR-023, ADR-024, ADR-029, ADR-033, ADR-038, ADR-045, ADR-046, ADR-049
**PR:** #443 (`work/vexcoder-restore-memory-compaction`)
**Implementation checklist:** `TASKS/PN-01-working-set-record.md`

## Context

Context sources are fragmented across a saved task document, an on-screen
TUI projection, a pulse-evidence snapshot, a peer JSONL log, and a flat
notes file. A saved task can restore the visible TUI surface, but
`TuiMode::apply_resumed_task` calls `reset_conversation_window` after
rebuilding the task document, which clears the live `ApiMessage` history.
`/compact` does the same after clearing completed pulses. The snapshot
bridge in `task_state_bridge.rs` is presentation-oriented — pulse input,
final text, changed files, tool summaries — not a serialized model
conversation, so it cannot stand in as the resume source.

Three heuristics carry more weight than they should:

- **Token budgeting** is `content.len() / 4` in both
  `session_notes::resolve_notes_load` and
  `project_instructions::estimate_tokens`. A byte count is not a token
  count for any real vocabulary, so the budget check itself is
  approximate exactly where it decides whether something survives.
- **Project instructions** (`load_project_instructions`) checks a fixed
  three-name list — `.vex/AGENTS.md`, `AGENTS.md`, `.vex/PROJECT.md` — in
  a single directory and stops at the first match. If that first match is
  over budget, the loader returns `OverBudget` immediately; it never
  tries the next candidate, and it never looks at a parent or child
  directory.
- **Memory** is one file, injected whole or skipped whole. There is no
  scope, provenance, or accepted/pending state, so a single stale line
  and a single verified fact are trusted equally, or dropped equally.

PR #443 already lands a Phase 0 stopgap: `ApiClient.notes_content` moved
from `Option<String>` to `Arc<RwLock<Option<String>>>` with a
`set_notes_content` setter; `ConversationManager::refresh_notes_content`
checks a `NotesFileFingerprint` (file length plus modified time) before
each `send_message` call and only re-reads when the file has actually
changed; and `populate_local_server_info` runs before the first pulse so
compaction sees the real context window instead of a default. This keeps
the notes file current in the system prompt. It does not restore model
working state on resume, and it is not a replacement for a durable
working-set record — later phases must not grow a second continuity
protocol beside it.

## Crate and API Evidence

Three upstream crates close the gaps above with primitives this codebase
does not currently have, and one class of managed provider API is
evaluated and rejected on the same grounds ADR-023/024 already used to
prefer local, inspectable state.

- **Token counting — `tiktoken`.** A pure-Rust byte-pair-encoding
  tokenizer. `tiktoken::get_encoding("o200k_base")` (or
  `encoding_for_model(name)` for a specific model family) returns an
  encoder; `encoder.count(text)` returns a token count on a path that
  does not allocate the token-id vector `encode` would produce, and
  `count_with_special_tokens` extends that to text containing special
  tokens. This replaces `content.len() / 4` with the number the model
  context window is actually measured in, at the cost of one dependency
  and a small embedded vocabulary table (selectable per encoding via
  feature flags, so a build can carry only the vocabularies it uses).
  Source: docs.rs/tiktoken.

- **Schema validation — `schemars`.** `#[derive(JsonSchema)]` plus the
  `schema_for!` macro generate a JSON Schema document from a Rust type,
  and schemars reads a type's `#[serde(...)]` attributes so the schema
  matches what `serde_json` actually produces. This gives the
  `WorkingSetRecord` a checked contract instead of a hand-maintained one,
  without changing how the record is written to disk. Source:
  docs.rs/schemars, crates.io/schemars.

- **Peer-channel merge — `loro`.** A CRDT (conflict-free replicated data
  type) document library. `LoroDoc::new()` creates a document;
  `get_map`/`get_list`/`get_text` attach named containers to it;
  `set_peer_id` gives each writer a stable identifier;
  `export(ExportMode::updates(&version_vector))` plus `import` exchange
  only the operations a peer is missing rather than a full log replay;
  and concurrent edits to the same container merge without a designated
  writer. This gives the peer channel a real merge rule (supersede by
  message id, apply in causal order) instead of waiting for every child
  to finish and concatenating free-text summaries. Source: docs.rs/loro.

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

## Stay / Removed / Net Change

| Area | Stays | Removed | Net change |
| :--- | :--- | :--- | :--- |
| Resume hydration | The on-screen task-document projection built by `task_state_bridge.rs` | `reset_conversation_window` clearing `ApiMessage` history in `TuiMode::apply_resumed_task` and after `/compact` | Load a `WorkingSetRecord` on `/resume` and after `/compact`; seed the next request from it instead of an empty window |
| Memory / notes | The Phase 0 `Arc<RwLock<Option<String>>>` store and per-pulse fingerprint refresh (PR #443, already merged) | The flat file as the only durable memory unit, injected whole or not at all | Typed candidates (`user`, `feedback`, `project`, `reference`) carrying provenance, topic, and an accepted/pending state; only accepted candidates inject; over budget, the lowest-priority pending candidate drops first |
| Token budgeting | The budget-check call sites in `session_notes` and `project_instructions` | `content.len() / 4` in both places | A `tiktoken` count on the zero-allocation `count()` path |
| Project instructions | The three-name candidate list and its priority order | Single-directory, first-match, stop-on-over-budget loading | A root-to-leaf directory walk, one candidate per directory, closer files layered after farther ones, and a manifest recording what was skipped and why |
| Peer channel | The append-only JSONL sidecar, its locking, and the ADR-046 read/post routes | Waiting on every child plus free-text summary concatenation on join | A `loro` document per task: per-consumer read cursors, message-id supersession, evidence references written into the working-set record |
| API caching | ADR-049's shared-prefix fingerprint and `ApiMessage.cache_hint` | Nothing; provider-specific mapping stays deferred | The working-set record is the local fallback, so resume never depends on an opaque provider continuation item |

## Decision

### 1. `WorkingSetRecord` with a generated schema

```rust
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Durable unit of continuity, persisted under
/// `.vex/state/{task_id}.working-set.json` via the existing
/// durable-write helper. The condenser is the sole writer.
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
    /// e.g. `src/foo.rs:42` or a peer-channel message id.
    pub source_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PathChange {
    pub path: PathBuf,
    pub git_identity: String,
}
```

A lint step regenerates `schema_for!(WorkingSetRecord)` and fails the
build if a checked-in `working-set.schema.json` has drifted, so the
on-disk contract cannot go stale without a reviewed change.

### 2. Real token counts for every budget check

```rust
use tiktoken::CoreBpe;

/// Replaces the `content.len() / 4` estimate used today in
/// `session_notes::resolve_notes_load` and
/// `project_instructions::estimate_tokens`.
fn token_count(encoding: &CoreBpe, text: &str) -> usize {
    encoding.count(text) // zero-allocation path; no token vector built
}

fn encoder_for_model(model_name: &str) -> CoreBpe {
    tiktoken::encoding_for_model(model_name)
        .unwrap_or_else(|_| tiktoken::get_encoding("o200k_base").expect("bundled vocabulary"))
}
```

The encoder is cheap to hold for the life of a `ConversationManager` and
reused across pulses rather than rebuilt per call.

### 3. Hierarchical instruction loading

Walk from the repository root to the working directory. At each
directory, try the same fixed candidate names `load_project_instructions`
already checks. Concatenate root-first so closer files override farther
ones. A file that does not fit the remaining budget is skipped and
recorded in a manifest; the walk continues instead of stopping.

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
    encoding: &tiktoken::CoreBpe,
    token_budget: usize,
) -> InstructionSet {
    let mut sections = Vec::new();
    let mut manifest = Vec::new();
    let mut remaining = token_budget;

    for dir in directories_root_to_leaf(repo_root, cwd) {
        let Some((path, content)) = first_existing(&dir, CANDIDATE_FILES) else {
            continue;
        };
        let estimated = token_count(encoding, &content);
        let included = estimated <= remaining;
        if included {
            remaining -= estimated;
            sections.push(content);
        }
        manifest.push(InstructionSource { path, included, estimated_tokens: estimated });
    }

    InstructionSet { content: sections.join("\n\n"), manifest }
}

fn directories_root_to_leaf(repo_root: &Path, cwd: &Path) -> Vec<PathBuf> {
    let relative = cwd.strip_prefix(repo_root).unwrap_or(cwd);
    let mut out = vec![repo_root.to_path_buf()];
    let mut acc = repo_root.to_path_buf();
    for component in relative.components() {
        acc = acc.join(component);
        out.push(acc.clone());
    }
    out
}

fn first_existing(dir: &Path, names: &[&str]) -> Option<(PathBuf, String)> {
    names
        .iter()
        .find_map(|name| {
            let candidate = dir.join(name);
            std::fs::read_to_string(&candidate).ok().map(|c| (candidate, c))
        })
}
```

`/context` renders `manifest` so an operator can see which files loaded
and which were skipped for budget, rather than inferring it from prompt
size.

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

Only `Accepted` candidates are read into the record. `Pending` candidates
need an operator `/memory accept` (or equivalent) before use. Over
budget, the lowest-priority `Pending` candidate is dropped first; nothing
is silently skipped in bulk the way a single over-budget file is today.

### 5. Peer-channel merge

```rust
use loro::{ExportMode, LoroDoc, VersionVector};

pub struct PeerChannel {
    doc: LoroDoc,
}

impl PeerChannel {
    pub fn new(peer_id: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer_id).expect("peer id fits the configured width");
        Self { doc }
    }

    /// Posts a message and marks any earlier ids it supersedes.
    pub fn post(&self, id: &str, body: &str, supersedes: &[String]) {
        let messages = self.doc.get_map("messages");
        let entry = messages
            .insert_container(id, loro::LoroMap::new())
            .expect("fresh message id");
        entry.insert("body", body).expect("map insert");
        entry
            .insert("supersedes", supersedes.to_vec())
            .expect("map insert");
        self.doc.commit();
    }

    /// Exports only the operations a peer at `since` is missing.
    pub fn export_updates_since(&self, since: &VersionVector) -> Vec<u8> {
        self.doc
            .export(ExportMode::updates(since))
            .expect("export from a committed doc")
    }

    pub fn import_updates(&self, bytes: &[u8]) {
        self.doc.import(bytes).expect("well-formed peer update");
    }
}
```

Join applies `supersedes` and writes an evidence reference into the
working-set record instead of waiting for every child and concatenating
free-text summaries. The phase 5 anchor test
`join_applies_supersession_instead_of_concatenating_summaries` in
`TASKS/PN-01-working-set-record.md` is the acceptance check for this
behavior; the code above is a sketch toward that test, not a finished
implementation.

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
  compared to today's silent whole-file injection.
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
- **Load a single project-instructions file, as today.** Rejected: an
  over-budget file silently disables the whole layer, and a single file
  cannot express directory-scoped conventions in a larger repository.
- **Treat the on-screen TUI projection as the resume source.** Rejected:
  ratatui's immediate-mode rendering means that view is reconstructed
  from application state every frame and is not itself a second copy of
  that state — ADR-045 reaches the same conclusion for snapshot replay.

## Validation

Phases 1 through 5 in `TASKS/PN-01-working-set-record.md` name the
acceptance tests for this decision:

- `working_set_record_round_trips_through_persist`
- `resume_injects_working_set_into_next_request`
- `compact_writes_working_set_before_clearing_pulses`
- `instruction_walk_falls_back_when_higher_file_is_over_budget`
- `pending_memory_candidates_are_not_injected`
- `join_applies_supersession_instead_of_concatenating_summaries`

Alongside those, a schema-diff step regenerates
`schema_for!(WorkingSetRecord)` and fails if the checked-in schema file
does not match, so the persisted contract cannot change without a
reviewed diff. None of the above is implemented in the Phase 0 slice
already on this branch; Phase 0 is notes refresh only and must not grow
into a second continuity protocol ahead of Phase 1.

## References

- `tiktoken` — pure-Rust BPE tokenizer, zero-allocation `count()` path:
  <https://docs.rs/tiktoken>
- `schemars` — JSON Schema generation from Rust types via
  `#[derive(JsonSchema)]`: <https://docs.rs/schemars>,
  <https://crates.io/crates/schemars>
- `loro` — CRDT framework for local-first documents, version-vector
  incremental sync: <https://docs.rs/loro>
- `ratatui` — immediate-mode rendering with intermediate buffers
  (existing project dependency, cited here for the resume-source
  rationale): <https://docs.rs/ratatui>
- Internal: `TASKS/PN-01-working-set-record.md`, ADR-045, ADR-046,
  ADR-049
