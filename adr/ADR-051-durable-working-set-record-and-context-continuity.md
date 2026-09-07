# ADR-051: Durable Working-Set Record and Context Continuity

**Status:** Proposed
**Chain:** ADR-023, ADR-024, ADR-029, ADR-033, ADR-038, ADR-045, ADR-046, ADR-049
**PR:** #443 (`work/vexcoder-restore-memory-compaction`)

## Context

The runtime is strong on local, inspectable, bounded state for a single pulse.
Context sources remain fragmented across a saved task document, a TUI
transcript, a pulse evidence snapshot, a peer JSONL log, and a flat notes
file. A saved task can restore the on-screen transcript, but `/resume`
clears the live model-message window. Long-running work therefore depends on
lossy heuristic history, an all-or-nothing notes file, and manual
re-discovery rather than one durable, evidence-backed working context.

The reset is explicit. `TuiMode::apply_resumed_task` reconstructs a
`TaskDocument` from a snapshot and then calls `reset_conversation_window`,
which clears the live `ApiMessage` history. `/compact` does the same after
clearing completed pulses. The snapshot bridge in
`src/runtime/task_document/task_state_bridge.rs` is presentation-oriented:
it stores pulse input, final text, changed files, and tool invocation
summaries, not a serialized model conversation.

Material shortcomings:

1. **Surface-only restoration.** Resume rebuilds the task document and the
   TUI transcript, then starts the next model request from an empty
   conversation window. Decisions, rationale, tool evidence, unresolved
   questions, and the prior tool-call trace are not rehydrated for the
   model.
2. **Heuristic compaction.** Local defaults keep 14 API messages; remote
   defaults keep 32. Token estimation is byte-count divided by 4.
   Overflow handling condenses tool results and then drops older messages.
   The heuristic summary path prefers earlier user-message first lines and
   does not treat assistant conclusions as a source of truth.
3. **Flat persistent memory.** One notes file is read in full and injected,
   or skipped entirely when its estimated budget is exceeded. There is no
   project or worktree scope, topic retrieval, provenance, expiry, or
   contradiction handling. Auto-memory extracts bullet-like lines from
   assistant text; it is not a reviewed write path.
4. **Shallow instruction composition.** `load_project_instructions` selects
   the first existing file among `.vex/AGENTS.md`, `AGENTS.md`, and
   `.vex/PROJECT.md`. An over-budget higher-priority file returns
   `OverBudget` and prevents fallback. There is no directory walk, path
   scoped rule loading, merge order, or source manifest.
5. **Underused provider cache controls.** ADR-049 defines a runtime-owned
   shared-prefix fingerprint and `ApiMessage.cache_hint`, but
   provider-specific continuation and compaction controls remain deferred.
   Local heuristic truncation still carries the continuity load.
6. **Log-only peer channel.** ADR-046 gives an auditable append-only JSONL
   channel with locks, addressing, and bounded reads. It has no durable
   consumer cursor, supersession rules, conflict model, or evidence
   linking. Join waits for every child to reach a completed state and then
   concatenates free-text summaries.

Historical ADRs reduced the gap without closing it. ADR-023 assembled
context and a short handoff. ADR-024 named notes, resume, and search.
ADR-029 and ADR-045 widened task-state persistence. ADR-033 improved
retrieval. ADR-038 kept assembly memory-first. ADR-049 named a reusable
prefix. Each added a store. None defined one structured working set that
is refreshed from evidence and injected on resume.

The memory-note refresh retained on this branch (shared `ApiClient` notes
storage, per-pulse reload, local server-info preload) is a stopgap. It
keeps the existing notes file in the system prompt instead of conversation
history. It does not restore model working state on resume and does not
replace the working-set record.

## Decision

- Introduce a **versioned working-set record** as the primary durable unit
  of continuity. The record captures:
  - objective
  - constraints
  - decisions with source references
  - changed paths plus version-control identity
  - verified results
  - unresolved questions
  - active plan
  - next action
  - record version and updated-at
- Persist the record beside existing task-state JSON under
  `.vex/state/`. The record is local, readable, and independent of any
  provider conversation identifier.
- Hydrate the working-set record on `/resume` and after `/compact`. The
  live model-message window is reconstructed from this record plus any
  provider-native compaction fallback the transport already exposes. Do
  not start the next pulse from an empty conversation window when a
  working-set record exists.
- Implement **hierarchical instruction loading**. Walk from the repository
  root to the working directory. Load at most one instruction file per
  directory from a documented candidate list. Concatenate root-first so
  closer files override earlier ones. When a file exceeds the remaining
  byte or token budget, skip that file, record the skip in a source
  manifest, and continue the walk. Never fail the whole instruction layer
  because one over-budget file was found first.
- Treat persistent memory as **typed, reviewable candidates** rather than
  trusted flat facts. Extraction must include provenance, topic scope, and
  an explicit accepted or pending state before injection. Over-budget
  notes degrade by dropping lowest-priority pending candidates, not by
  skipping the entire file.
- Use provider-native compaction and prefix-cache controls where the
  transport can map them from the ADR-049 shared-prefix contract. Keep a
  deterministic local fallback: the working-set record itself. Do not
  depend on opaque provider blobs as the only resume artifact.
- Evolve the peer channel into a **state-merge protocol**. Add durable
  consumer cursors, supersession of earlier messages by id, evidence
  references into the working-set record, and a join merge that applies
  those rules instead of concatenating free-text summaries.

## Rationale

A local working-set record preserves the properties this codebase already
defends: readable compaction, an auditable peer channel, bounded context
injection, and no silent full-repository dump. Managed conversation APIs
achieve continuity with server-side state or encrypted compaction
payloads. Those designs trade away inspectability and the ability to
rebuild a session outside one vendor store.

Hierarchical file-based instruction loading closes the silent failure
where an over-budget first file disables the instruction layer. A source
manifest makes the winning files visible in `/context` and in tests.

Reviewable memory candidates close the all-or-nothing notes cliff. The
existing notes file remains a storage surface; injection becomes scoped
and gated rather than whole-file or nothing.

A runtime-owned record plus an optional provider mapping matches ADR-049:
the runtime names the durable unit once, and transports map it later.
Opaque provider continuation identifiers may be stored as hints, never as
the sole source of truth.

## Consequences

- Task resume and manual compaction preserve verified conclusions and
  open questions via the working-set record instead of discarding the
  assistant reasoning that produced them.
- Project instructions degrade through hierarchical loading and budget
  fallbacks, including directory-scoped conventions in large repositories.
- Auto-memory moves from silent flat extraction to typed candidates with
  provenance, topic scope, and explicit acceptance.
- Subagent joins resolve conflicts through supersession and evidence
  links rather than free-text concatenation.
- The runtime keeps a readable local audit trail of continuity and does
  not require a proprietary compaction endpoint to resume work.
- The notes-refresh stopgap on this branch remains valid until the
  candidate injector lands. It must not grow into a second continuity
  protocol.

## Alternatives Considered

- **Adopt opaque server-side compaction and continuation identifiers as
  the durable unit.** Rejected because it sacrifices local
  inspectability, portability, and the ability to reconstruct the
  conversation outside a specific vendor store.
- **Keep the flat notes file and heuristic message truncation.** Rejected
  because it fails to preserve assistant reasoning and scales poorly,
  including a silent budget cliff.
- **Use a persistent remote sandbox as the continuity store.** Rejected
  because it couples the architecture to managed compute and removes the
  local, versioned working-set record.
- **Load only a single project-instructions file.** Rejected because an
  over-budget file silently disables the feature, and a single file cannot
  express directory-scoped conventions.
- **Treat the TUI transcript as the resume source.** Rejected because the
  transcript is a presentation projection. ADR-045 already deprecates
  lossy snapshots as an accepted resume source for replay-relevant state.

## References

- Internal continuity audit of fragmented context construction
  (September 2026), limited to in-tree code paths.
- [ADR-049](ADR-049-shared-prefix-prompt-caching-and-fork-controls.md)
- [ADR-045](ADR-045-replay-first-task-document-and-single-writer-state.md)
- [ADR-046](ADR-046-agent-peer-message-channel.md)
- [ADR-038](ADR-038-memory-first-architecture-with-minimal-disk-io.md)
- [ADR-033](ADR-033-hybrid-retrieval-context-architecture.md)
- [ADR-023](ADR-023-deterministic-edit-loop.md)
- Implementation checklist: `TASKS/PN-01-working-set-record.md`
