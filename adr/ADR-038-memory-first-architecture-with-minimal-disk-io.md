# ADR-038: Memory-First Architecture with Minimal Disk I/O

- **Status:** Accepted
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

1. `src/runtime/context_assembler/{mod,reads}.rs` still owns the automatic
  named-file and inferred-related-path snapshot path, so this module family is
  the seam where the remaining TTFC refactors have to keep shrinking sync I/O.
2. Automatic turn assembly used to run `git status --short` and `git diff HEAD`
  before the model request even when the prompt did not ask for git state.

The repository already has two disk-backed layers that are worth keeping
explicitly durable:

- `src/tools/{index,search,semantic}.rs` persist code-search indexes under
  `.vex/index/`
- `src/runtime/task_state/{mod,persist}.rs` persists the multi-agent handoff map
  under `.vex/state/`

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

1. ~~Split `src/config/load.rs` into cache, path, and merge modules and add
   process-local config caching.~~ Config cache added in Phase 2
   (`src/config/cache.rs`). Path, merge, and parse extraction completed in
   Batch C (PR #279): `src/config/load/{paths,merge,parse}.rs`.
2. ~~Add an explicit disk-permission boundary around operator, search, and
  task-state I/O.~~ `src/disk_policy.rs` added in Phase 2. Batch D (PR #280)
  split `src/tools/operator.rs` into `src/tools/operator/{mod,core,file_ops,
  git_ops,search}.rs` so operator-level enforcement can land in a smaller,
  policy-focused follow-up.
3. ~~Split `src/runtime/context_assembler.rs` into orchestration and read
  helpers.~~ Batch E (PR #281) moves path extraction, snapshot conversion, and
  related-path inference into `src/runtime/context_assembler/reads.rs`, with
  orchestration and tests retained in `src/runtime/context_assembler/mod.rs`.
4. ~~Add strict policy tests and CI gates for the allowed-disk contract.~~
  Batch F (PR #281) adds `enforce()` / `enforce_runtime()` to
  `src/disk_policy.rs`, `tests/disk_policy_tests.rs`, `make check-disk-policy`,
  and the `arch-contracts.yml` CI step.
5. ~~Wire operator/search/task-state call-sites through the disk-policy boundary
  now that the strict-mode helper and CI harness exist.~~ Batch G (PR #282)
  adds `src/tools/operator/policy.rs` wrapper and wires `assert_durable_access()`
  into `TaskState::save()` / `TaskState::load()`. Also fixes cross-platform
  `check_path()` to handle Windows backslash separators.
6. ~~Evaluate optional task-state WAL once the in-memory first-turn path is
  stable and measurable.~~ Batch H (PR #283) decomposes
  `src/runtime/task_state.rs` (807 lines) into
  `src/runtime/task_state/{mod.rs, persist.rs}`, isolating all persistence
  logic. WAL evaluation concluded: not warranted because task-state saves
  are per-session (not per-turn) and `write_json_safe` already performs
  crash-safe writes (temp + fsync + rename).

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
- Operator-level policy wiring and WAL evaluation are complete as of Batch H
  (PR #283). No remaining follow-up items.

## Implementation status

Phase 1 is implemented on `work/vexcoder-adr-038-memory-first-phase1` as of
2026-03-30. Merged in PR #276.

Phase 1a: Search lane tightening merged in PR #277 (search config during index
warmup, incremental refresh independence from auto_index).

Phase 2 (Level 0 foundation + config cache) introduced:

- `src/disk_policy.rs` -- DiskPermission enum, check_path() classifier,
  DiskPolicyMode with VEX_DISK_POLICY env var
- `src/config/cache.rs` -- OnceLock-based config cache, Config::load_cached()

Batch C (config/load.rs directory module) introduced:

- `src/config/load/paths.rs` -- path discovery and resolution
  (find_repo_local_config, user_config_path, expand_home,
  resolve_working_dir, load_model_profile)
- `src/config/load/merge.rs` -- layer merge helpers (apply_over,
  apply_*_over, resolve_auto_memory_config)
- `src/config/load/parse.rs` -- enum + header parsing
  (parse_model_backend, parse_model_protocol, infer_model_protocol,
  parse_model_headers_json, legacy protocol value helpers)
- `src/config/load/mod.rs` -- orchestration, resolve_*, validate_*,
  read_env_layer, migrate_config_from_env, and the test suite retained

Merged in PR #279.

Batch D (operator directory module) introduced in PR #280:

- `src/tools/operator/mod.rs` -- shared types, helpers, and retained tests
- `src/tools/operator/core.rs` -- workspace confinement, path normalization,
  and gitignore-aware workspace walking
- `src/tools/operator/file_ops.rs` -- file reads, file writes, patch apply,
  edit_file, rename_file, and list_files
- `src/tools/operator/git_ops.rs` -- git status/diff/log/show/add/commit
  helpers and command execution
- `src/tools/operator/search.rs` -- literal search, content search, glob
  matching, and file discovery helpers

Batch E/F (context assembler split + strict disk-policy gate) introduced in
PR #281:

- `src/runtime/context_assembler/mod.rs` -- types, assemble/render
  orchestration, env resolution, and retained tests
- `src/runtime/context_assembler/reads.rs` -- candidate path extraction,
  snapshot conversion, and related-path inference helpers
- `src/disk_policy.rs` -- `enforce()` and `enforce_runtime()` layered on top of
  `check_path()` and `resolve_policy_mode()`
- `tests/disk_policy_tests.rs` -- strict/warn/off enforcement coverage and
  allowed/forbidden path classification
- `Makefile` / `.github/workflows/arch-contracts.yml` -- `check-disk-policy`
  target and CI enforcement step

Key source files:

- `src/runtime/context_cache.rs`
- `src/runtime/git_snapshot.rs`
- `src/runtime/context_assembler/mod.rs`
- `src/runtime/context_assembler/reads.rs`
- `src/disk_policy.rs`
- `src/config/cache.rs`
- `src/config/load/mod.rs`
- `src/config/load/paths.rs`
- `src/config/load/merge.rs`
- `src/config/load/parse.rs`
- `src/tools/operator/mod.rs`
- `src/tools/operator/core.rs`
- `src/tools/operator/file_ops.rs`
- `src/tools/operator/git_ops.rs`
- `src/tools/operator/search.rs`
- `tests/disk_policy_tests.rs`
- `docs/src/architecture.md`
- `docs/src/configuration.md`

## Validation

- Unit coverage for cache hits, invalidation, and eviction in
  `src/runtime/context_cache.rs`
- Context assembly coverage for cache reuse and opt-in git behavior in
  `src/runtime/context_assembler/mod.rs`
- Disk-policy enforcement coverage in `tests/disk_policy_tests.rs` (including
  Windows backslash separator tests)
- Operator policy module tests in `src/tools/operator/policy.rs`
- Focused validation with `cargo test -q runtime::context_assembler --lib`
- `make check-disk-policy`
- Full workspace validation and CI gating required before merge

## References

- [ADR-029](ADR-029-stream-parser-completeness-and-session-persistence.md)
- [ADR-030](ADR-030-runtime-task-state-and-orchestrator-control-flow.md)
- [ADR-033](ADR-033-hybrid-retrieval-context-architecture.md)
- [ADR-034](ADR-034-multi-agent-parallel-task-execution.md)