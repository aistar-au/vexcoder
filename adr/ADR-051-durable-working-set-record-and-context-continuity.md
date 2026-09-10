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
