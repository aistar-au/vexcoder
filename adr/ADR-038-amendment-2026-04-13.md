# ADR-038 Amendment — Task-State Cold-Start Memory Bounds

- **Amends:** ADR-038 (Memory-First Architecture with Minimal Disk I/O)
- **Status:** Accepted
- **Date:** 2026-04-13
- **Deciders:** Core maintainer
- **Depends on:** ADR-038 (all batches A–H merged), ADR-034, ADR-045 (proposed)
- **Cross-reference:** ADR-045 (Replay-First Task Document) — if ADR-045 implements
  first, `TaskStateHeader` field names must be verified against whatever
  `RuntimeEventLog`-driven JSON schema it adopts for session-task state.

---

## Context

ADR-038 adopted a memory-first contract for turn assembly and established
`src/runtime/task_state/{mod,persist}.rs` as an "intended durable surface"
exempt from the minimal-disk-I/O mandate. Batches A–H completed the mandate
for context assembly, git snapshots, config loading, operator directory
modules, the strict disk-policy gate, and task-state persistence safety.

ADR-038 Batch H concluded that a write-ahead log was not warranted because
"task-state saves are per-session (not per-turn) and `write_json_safe` already
performs crash-safe writes." That conclusion was correct. However, the
exemption was silently broader than intended: it also left the **read side** of
task-state discovery outside the memory-first contract.

Three hotpaths in `persist.rs` perform unbounded O(N) disk I/O at
startup, where N is the number of `.vex/state/*.json` files:

| Hotpath | Symptom |
|---------|---------|
| `state_files_from()` — no scan cap | 15 s+ startup stall on large state dirs; OS paging when heap exceeds physical RAM |
| `live_session_task_counts_from()` — `load_live_summary()` per file | Full `read_to_string` + `from_str` per file even though only the `session_tasks` sub-array is needed |
| `find_session_task_in_saved_states()` — `TaskState::load()` per file | **Full deserialization** of every task JSON — the worst hotpath; `Vec<TurnEvidenceState>` and `BTreeMap<Capability,ApprovalScope>` are allocated and immediately discarded for every non-matching file |

Additionally, a design spec circulated as "ADR-038 Extension" contained three
technical errors that this amendment corrects before any implementation begins.

### Why not a new ADR

ADR-038 is **Accepted**, not **Locked**. Per the ADR-README status vocabulary,
Locked means "Accepted and immutable — no further amendments without a new
ADR." Accepted ADRs may be amended. This work extends the existing memory-first
mandate to a surface that was previously scoped out; it introduces no new
architectural decision.

### Stack overflow and ASLR — confirmed non-issues

These were raised as potential concerns. Both are verifiably not issues:

**Stack overflow** cannot occur here. Every scan path uses heap-driven
iteration: `state_search_dirs_from` returns a `Vec<PathBuf>`, the outer loop
is a `flat_map` over that vec, and `state_files_in_dir` builds and returns a
`Vec<TaskStateFile>`. There is no recursion, no user-controlled stack
allocation, and no deep call chain. Rust's default 2 MiB stack thread is never
stressed. `serde_json::from_reader` is iterative internally.

**ASLR** randomises heap base addresses but does not affect total bytes
allocated, the contiguity of `Vec` allocations, or page-fault behaviour when
physical RAM is exhausted. The paging symptom is caused by allocation
**volume** (O(N) full `TaskState` heap graphs), not by address-space layout.
The fix is algorithmic bounds on N, not address-space tuning.

Both facts are documented here and should be referenced in
`docs/src/performance.md`.

---

## Decision

Extend ADR-038's memory-first contract to the cold-start task-state discovery
path. The following sub-decisions are adopted:

1. All startup enumeration of `.vex/state/` must respect a configurable cap
   (`VEX_MAX_STARTUP_TASK_SCANS`, default 200).
2. Session-check and UI-list paths must operate on a **header projection**
   (`TaskStateHeader`) rather than the full `TaskState` graph.
3. `TaskState::load()` must never be called speculatively during
   cold-start discovery. It is called only when the caller has already
   identified the specific task it needs.
4. Header projections for repeated TUI restarts within the same process
   lifetime must be served from a bounded in-memory cache, following the
   same manual LRU pattern established in `src/runtime/context_cache.rs`.
5. The `startup-tracing` feature gate must expose optional allocation
   telemetry for profiling.

---

## Correction of Design Spec Errors

The circulated "ADR-038 Extension" spec contained three errors that would
produce compile failures or silent policy violations. All three are corrected
here as the accepted pre-implementation record.

### Error 1 — `serde_json` early-stop claim

**Original claim:** *"serde_json will stop deserializing once all named
fields in `TaskStateHeader` are satisfied, ignoring the rest of the JSON."*

**This is false.** `serde_json::from_reader` and `from_str` process the
entire JSON stream regardless of field coverage. There is no built-in
early-exit for derived `Deserialize` impls.

**Correct description:** Projecting a small struct (`TaskStateHeader`)
avoids allocating the large sub-graphs (`Vec<TurnEvidenceState>`,
`BTreeMap<Capability, ApprovalScope>`, `Vec<InterruptedCommand>`, etc.)
into the Rust heap. The stream still parses fully, but ~95% of
allocation work is eliminated because the fields that would construct
those sub-graphs are simply ignored by serde and never materialised.

All code comments and documentation must use this accurate description.

### Error 2 — `LazyTaskHandle.state` field undeclared

The spec's `LazyTaskHandle` struct definition was missing the `state`
field, yet the `resolve()` body wrote `self.state = Some(Box::new(state))`.

The correct struct definition includes `state: Option<Box<TaskState>>` as
declared in `src/runtime/task_state/lazy_task_handle.rs`.

### Error 3 — `assert_durable_access()` bypass

ADR-038 Batch G wired `assert_durable_access()` into every `TaskState`
disk read/write path. The spec's `TaskStateHeader::from_path()` used a
raw `std::fs::File::open()` call with no policy layer. This would be caught
by `make check-disk-policy`.

The correct implementation in `task_header.rs` calls
`crate::tools::operator::policy::assert_durable_access(path)?` before
opening the file.

---

## Existing Private Projection — Promotion, Not Replacement

`persist.rs` already contains a private `TaskStateLiveSummary` +
`SessionTaskLiveSummary` pair used by `load_live_summary()`. This is
effectively the same projection this amendment formalises as
`TaskStateHeader`. The implementation must **promote and replace** that
private pair rather than introduce a parallel pattern alongside it.

After this amendment:
- `TaskStateLiveSummary`, `SessionTaskLiveSummary`, and `load_live_summary()`
  are **deleted** from `persist.rs`.
- `TaskStateHeader` and `SessionTaskSummary` in `task_header.rs`
  are the single projection source for all scan paths.

---

## Implementation

### New files

- `src/runtime/task_state/task_header.rs` — `TaskStateHeader` and
  `SessionTaskSummary` projection structs with `from_path()`.
- `src/runtime/task_state/header_cache.rs` — bounded LRU header cache
  keyed by full path (process-global, `HashMap` + u64 tick pattern from
  `context_cache.rs`).
- `src/runtime/task_state/lazy_task_handle.rs` — test-only `LazyTaskHandle`
  scaffolding for the lazy-load API shape used by the cold-start tests.
- `src/config/startup.rs` — `StartupBudget` with `VEX_MAX_STARTUP_TASK_SCANS`
  (default 200), `VEX_STARTUP_CACHE_TTL_MS`, `VEX_TRACE_STARTUP_ALLOC`.

### Modified files

- `src/runtime/task_state/mod.rs` — add module declarations and re-export.
- `src/config.rs` — add `mod startup; pub use startup::StartupBudget;`.
- `src/runtime/task_state/persist.rs`:
  - Delete `TaskStateLiveSummary`, `SessionTaskLiveSummary`, `load_live_summary`.
  - Hard cutover: `state_files_from()` now always routes through the bounded
    top-k selector via `state_files_from_with_limit()`. The unbounded
    collect-then-truncate path and the private `state_files_in_dir` helper
    are removed entirely. When `limit` is `None`, the budget default
    (`StartupBudget::default().max_scans`) is applied automatically.
  - Replace `live_session_task_counts_from()` body with header-projection scan.
  - Replace `find_session_task_in_saved_states()` body with header-first scan.
- `src/app/task_facade.rs`:
  - All facade list operations (`facade_list_tasks`, `facade_list_session_tasks`,
    `facade_task_graph`, `facade_list_todos`) pre-allocate `Vec::with_capacity`
    from the bounded file set to prevent incremental reallocation.

### session_task_id format contract

The `is_candidate` predicate in `find_session_task_in_saved_states` depends on
the format `"{parent_task_id}-{agent_id}-{uuid}"` from `SessionTask::new` in
`src/runtime/session_task.rs`. This format was verified at the implementation
site (lines 70–74) and all call-sites use `SessionTask::new` exclusively.
If ADR-045 changes the ID format, this predicate must be updated.

---

## Consequences

### Positive

- Cold-start RSS peak drops from O(N × TaskState) to O(min(N, 200) × TaskStateHeader).
  For a state dir with 10k task files and average 50 kB JSON, peak heap during
  scan falls from ~500 MB to under 10 MB.
- OS paging on startup is eliminated for all workloads within the default cap.
- `find_session_task_in_saved_states` drops from O(N) full loads to O(1) full
  load in the expected case (one match per scan).
- The disk-policy enforcement contract (Batch G) is maintained across all new
  read paths.
- No new crate dependencies. The LRU cache reuses the established
  `HashMap` + tick pattern from `context_cache.rs`.

### Negative

- Users with more than 200 task files will see older tasks excluded from the
  recent-task UI list by default. The UI should display a hint:
  *"Showing 200 most recent tasks. Set VEX_MAX_STARTUP_TASK_SCANS to increase."*
- The `session_task_id` prefix-match heuristic in `find_session_task_in_saved_states`
  depends on the `"{parent_task_id}-{agent_id}-{uuid}"` format from
  `SessionTask::new`. If ADR-045 changes the ID format, the `is_candidate`
  predicate must be updated.
- Task-state header `modified_millis` maps to `TaskState.updated_at` (u64, JSON
  epoch millis), not to the filesystem mtime used by `TaskStateFile.modified_millis`
  (u128). Files edited externally may have stale `updated_at` values. This is
  acceptable: the sort is best-effort for UI ordering, not a correctness guarantee.

### Relationship to ADR-045

ADR-045 (Proposed) introduces `RuntimeEventLog` as the accepted persisted
truth and reclassifies `persistable_snapshot` as a compatibility export. If
ADR-045 lands before this amendment is implemented:

- Verify that `TaskStateHeader` field names (`id`, `updated_at`,
  `session_tasks`) remain valid in whatever JSON schema ADR-045 adopts.
- If ADR-045 adds new session-task identity fields that change the composite
  ID format, update the `is_candidate` predicate in
  `find_session_task_in_saved_states` before merging.
- Nothing in this amendment conflicts with ADR-045's single-writer invariant:
  `TaskStateHeader::from_path` is a read-only projection and does not
  write to any `TaskDocument` field.

---

## Validation

```
# task_header.rs
test_parses_id_and_updated_at
test_defaults_updated_at_to_zero_for_legacy_json
test_parses_session_tasks_liveness
test_ignores_large_fields_without_allocating_them

# header_cache.rs
test_cache_hit_after_first_read
test_cache_invalidates_when_file_changes
test_cache_evicts_lru_entry_at_cap

# lazy_task_handle.rs
test_resolve_is_idempotent
test_header_accessible_before_resolve

# persist.rs (new tests)
test_state_files_from_with_limit_returns_newest_n
test_state_files_from_with_limit_zero_is_rejected_at_budget_level
test_state_files_from_with_none_limit_applies_default_cap
test_live_session_task_counts_respects_scan_cap
test_live_session_task_counts_uses_header_only
test_find_session_task_skips_full_load_on_non_matching_files
test_find_session_task_loads_only_candidate_file
test_find_session_task_handles_legacy_json_without_session_tasks

# config/startup.rs
test_defaults_to_200_when_env_unset
test_rejects_zero_max_scans_and_falls_back_to_default

# Gate targets (CI)
cargo test --all-targets
make check-disk-policy
make gate
```

## References

- [ADR-038](ADR-038-memory-first-architecture-with-minimal-disk-io.md)
- [ADR-034](ADR-034-multi-agent-parallel-task-execution.md)
- [ADR-045](ADR-045-replay-first-task-document-and-single-writer-state.md)
