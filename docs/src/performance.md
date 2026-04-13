# Performance

## Task-state cold start

The CLI app keeps recent-task discovery on a bounded cold-start path.
Startup scans keep an in-memory top-k buffer of at most
`VEX_MAX_STARTUP_TASK_SCANS` task-state files by default and prefer the newest
copies when the same task id exists in both the workspace state directory and a
legacy fallback directory. Directory entries are streamed directly into that
selector so bounded startup paths do not materialise a full per-directory
`Vec<TaskStateFile>` before truncation.

Cold-start scans read a small header projection from each selected task-state
file instead of deserialising the full `TaskState` graph. The projection keeps
the fields needed for recent-task discovery and live session-task counts while
skipping the large turn-history, approval, and command-evidence collections.

Production cold-start paths currently use header-only scans plus direct
candidate loads. `LazyTaskHandle` remains test-only scaffolding rather than a
shipped runtime abstraction.

## Why the projection helps

The improvement comes from allocation avoidance, not from stopping JSON parsing
early. The JSON decoder still consumes the full stream, but the large task-state
subgraphs are not materialised into Rust heap structures unless the caller has
already identified the specific task it needs to open.

This keeps cold-start memory proportional to the configured scan cap instead of
the total number of task files on disk.

When the binary is built with the `startup-tracing` feature, setting
`VEX_TRACE_STARTUP_ALLOC=1` emits a single `[startup-alloc]` line for each
bounded cold-start scan. The trace reports how many task-state files were
scanned, how many were retained under the cap, and the approximate bytes held
by the selected `TaskStateFile` set.

## Non-issues that do not need tuning

Two concerns often come up during cold-start investigations and are not the
root cause here:

- Memory stack overflow is not part of this path. Task-state discovery is iterative and
  heap-driven; it does not recurse over task files.
- Address-space layout randomisation does not change the total bytes allocated.
  If startup memory grows, the cause is allocation volume rather than address
  placement.

## Alternative designs

The current implementation keeps the existing JSON file format, streams
directory entries into a bounded top-k selector, and removes the largest
avoidable allocation inside each selected file with a typed header projection.
Other approaches remain possible when the task-state surface needs a different
latency or scale profile:

- A manual streaming extractor built on `serde_json::Deserializer` can skip
  more work than a derived projection, but it also adds schema-coupled parsing
  code that is harder to evolve and test than the current typed header.
- A dedicated sidecar index could persist the header projection separately and
  avoid parsing the full JSON document during cold start, but it must also
  solve invalidation and crash-consistency so the sidecar cannot drift from the
  task-state JSON.
- Directory sharding by time bucket or prefix can reduce `read_dir` and
  metadata pressure when the task-state surface grows large enough that one
  flat directory dominates cold-start cost.
- A single-writer event log paired with periodic summary checkpoints can make
  discovery replay-oriented rather than document-oriented without replaying the
  full history on every cold start.
- A database-backed index can support richer queries, but it also raises schema,
  migration, and operator-portability costs that the current file-based design
  avoids.
