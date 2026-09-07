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
- Tests under `src/app/tests/session/`, `src/runtime/task_document/tests.rs`,
  `src/state/conversation/tests/history.rs`

**ADR:** ADR-051

**Depends on:** ADR-045 Batch 1 (sole-writer task document), ADR-046 (peer
channel), ADR-049 shared-prefix contract. The notes-refresh stopgap on
PR #443 may land first and must not define a second continuity protocol.

---

## Issue

Resume restores the TUI surface, not the model's working state. Compaction
is a count-and-byte heuristic that drops assistant conclusions first.
Persistent memory is one notes file, injected whole or skipped. Project
instructions pick the first existing file and fail closed on over-budget
content. The peer channel is a durable log without a state-merge protocol.

PN-01 makes the durable unit a versioned working-set record and wires
resume, compaction, instructions, memory, and join through that record.

---

## Decision

### Phase 0 — notes refresh stopgap (this PR)

Keep the existing per-pulse notes reload and shared `ApiClient` notes
storage. Preload local server info before the first pulse so compaction
thresholds see the real context window. This is not the working-set
record. Later phases must not extend this stopgap into a parallel protocol.

### Phase 1 — record schema and persistence

Define `WorkingSetRecord` with objective, constraints, decisions (each
with a source reference), changed paths plus git identity, verified
results, unresolved questions, active plan, next action, version, and
updated-at. Persist under `.vex/state/{task_id}.working-set.json` using
the existing durable-write helper. The condenser is the sole writer.

### Phase 2 — hydrate on resume and compact

`/resume` loads the record and seeds the next model request with a
compact working-set block after project instructions. `/compact` writes
the record before clearing completed pulses and the conversation window,
then injects the same block. If no record exists, keep current behavior.

### Phase 3 — hierarchical instruction loading

Walk repository root to working directory. Load at most one instruction
file per directory from `.vex/AGENTS.md`, `AGENTS.md`, `.vex/PROJECT.md`.
Concatenate root-first. Skip an over-budget file, record the skip in a
source manifest, and continue. `/context` shows the manifest.

### Phase 4 — reviewable memory candidates

Replace whole-file injection with typed candidates (`user`, `feedback`,
`project`, `reference`) that carry provenance, topic, and
accepted/pending state. Only accepted candidates inject. Pending
candidates require an operator `/memory accept` (or equivalent) before
use. Over-budget handling drops pending candidates first.

### Phase 5 — peer join merge

Add a durable read cursor per consumer, a supersedes field, and optional
evidence references into the working-set record. Join applies
supersession and record fields rather than concatenating child summaries.

---

## Constraints

- Do not store opaque provider continuation blobs as the only resume
  artifact. Provider identifiers may be stored as hints.
- Do not send the full repository or git diff by default.
- Do not inject pending memory candidates.
- Do not fail the instruction layer because the first candidate file is
  over budget.
- Do not modify the on-disk session transcript during compaction.
- Do not introduce a second notes protocol alongside the working-set
  record after Phase 1.
- Prompt text and docs must stay inside `scripts/check_forbidden_names.sh`.
- Must not regress existing tests.

---

## Definition of Done

1. Working-set record round-trips through save and load.
2. `/resume` with a record seeds the next model request; the live window
   is not empty of working-set content.
3. `/compact` writes the record before clearing pulses.
4. Hierarchical instruction load concatenates root-to-leaf and falls back
   when a higher file is over budget.
5. Memory injection uses accepted candidates only.
6. Join merge applies supersession rather than free-text concatenation.
7. `cargo test --all-targets` is green.
8. `bash scripts/check_forbidden_names.sh` is clean.

---

## Anchor tests

```rust
#[test]
fn working_set_record_round_trips_through_persist() { ... }

#[test]
fn resume_injects_working_set_into_next_request() { ... }

#[test]
fn compact_writes_working_set_before_clearing_pulses() { ... }

#[test]
fn instruction_walk_falls_back_when_higher_file_is_over_budget() { ... }

#[test]
fn pending_memory_candidates_are_not_injected() { ... }

#[test]
fn join_applies_supersession_instead_of_concatenating_summaries() { ... }
```

**What NOT to do:**

- Do not copy third-party product wording, command names, or docs into
  this checklist or the implementation.
- Do not treat the TUI transcript as the model conversation.
- Do not skip Phase 1 and try to hydrate from notes text alone.
- Do not add provider-specific cache syntax until it maps from ADR-049.
