# ADR-038: Memory-First Architecture with Minimal Disk I/O

- **Status:** Active
- **Date:** 2026-03-30
- **Deciders:** Core maintainer
- **Depends on:** ADR-029, ADR-030, ADR-033, ADR-034
- **Supersedes:** None
- **Superseded by:** None

## Context

Vex already keeps most session state in memory once a process is live, but the
turn hot path still performs avoidable synchronous work before the first token
can render.

The current blockers are concentrated in two places:

1. `src/runtime/context_assembler.rs` reads named and inferred files from disk
   on every turn with no process-local reuse.
2. The same path also runs `git status --short` and `git diff HEAD` before the
   model request, even for turns that do not ask about git state.

The repository already has two disk-backed layers that are worth keeping
explicitly durable:

- `src/tools/{index,search,semantic}.rs` persist code-search indexes under
  `.vex/index/`
- `src/runtime/task_state.rs` persists the multi-agent handoff map under
  `.vex/state/`

That means the remaining mandatory turn-time disk I/O is mostly accidental,
not architectural. If Vex wants lower time-to-first-chunk, it must remove
unnecessary synchronous reads before it spends more time on downstream SSE
micro-optimizations.

## Decision

Adopt a memory-first contract for turn assembly:

1. Automatic context assembly must prefer process-resident caches over repeated
   disk reads.
2. Automatic git status and diff collection is opt-in, not mandatory, for the
   default turn path.
3. Explicit git tools, review flows, search indexes, and task-state JSON stay
   as the intended disk-backed surfaces.
4. TTFC work lands before further SSE tuning because network streaming begins
   only after context assembly completes.

### Phase 1 (implemented in this tree)

- Add `src/runtime/context_cache.rs`, a bounded in-memory cache for small text
  files read during context assembly.
- Move git snapshot helpers into `src/runtime/git_snapshot.rs` so git capture
  is isolated from file-snapshot assembly.
- Extend `ContextAssembler` with cache hit and miss accounting.
- Make automatic git context opt-in through `VEX_CONTEXT_INCLUDE_GIT`.

### Follow-up phases

1. Split `src/config/load.rs` into cache, path, and merge modules and add
   process-local config caching.
2. Add an explicit disk-permission boundary around operator, search, and
   task-state I/O.
3. Add strict policy tests and CI gates for the allowed-disk contract.
4. Evaluate optional task-state WAL once the in-memory first-turn path is
   stable and measurable.

## Consequences

### Positive

- Repeated turns can reuse unchanged file snapshots from memory instead of
  rereading the same files from disk.
- TTFC improves immediately for repository questions that do not need git
  status or diffs.
- The hot path becomes more consistent with ADR-033 search persistence and
  ADR-030 task-state durability.

### Negative

- Automatic prompts no longer receive implicit git status and diff unless the
  operator opts in or the command explicitly asks for git data.
- Process-local caches add invalidation and eviction rules that need direct
  test coverage.
- This ADR is intentionally partial; config caching and strict disk-policy
  enforcement remain follow-up work.

## Implementation status

Phase 1 is implemented on `work/vexcoder-adr-038-memory-first-phase1` as of
2026-03-30.

Key source files:

- `src/runtime/context_cache.rs`
- `src/runtime/git_snapshot.rs`
- `src/runtime/context_assembler.rs`
- `docs/src/architecture.md`
- `docs/src/configuration.md`

## Validation

- Unit coverage for cache hits, invalidation, and eviction in
  `src/runtime/context_cache.rs`
- Context assembly coverage for cache reuse and opt-in git behavior in
  `src/runtime/context_assembler.rs`
- Focused validation with `cargo test -q runtime::context_assembler --lib`
- Full workspace validation and CI gating required before merge

## References

- [ADR-029](ADR-029-stream-parser-completeness-and-session-persistence.md)
- [ADR-030](ADR-030-runtime-task-state-and-orchestrator-control-flow.md)
- [ADR-033](ADR-033-hybrid-retrieval-context-architecture.md)
- [ADR-034](ADR-034-multi-agent-parallel-task-execution.md)