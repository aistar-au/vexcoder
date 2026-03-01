# Task CORE-17: Task State Persistence

**Target File:** new `src/runtime/task_state.rs`, `src/runtime.rs`

**ADR:** ADR-022 Phase 5

**Depends on:** CORE-16 (`test_approval_policy_read_file_auto_allows_without_prompt` must be green)

---

## Issue

No durable task state exists. All execution context is in-memory and lost on process
exit. ADR-022 requires `TaskState` to be serialized to `VEX_STATE_DIR` after every
meaningful status transition, and for interrupted tasks to be explicitly resumable.

---

## Decision

1. Add `src/runtime/task_state.rs` with `TaskState` as a `serde` struct:

```rust
#[derive(Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub status: TaskStatus,
    pub active_grants: std::collections::HashMap<Capability, ApprovalScope>,
    pub changed_files: Vec<std::path::PathBuf>,
    pub command_history: Vec<CommandEvidence>,
    pub conversation_snapshot: ConversationCheckpoint,
    pub interrupted_sessions: Vec<InterruptedCommand>,
}
#[derive(Serialize, Deserialize)]
pub struct CommandEvidence { pub program: String, pub exit_code: Option<i32>, pub interrupted: bool }
#[derive(Serialize, Deserialize, Default)]
pub struct ConversationCheckpoint { pub messages: Vec<serde_json::Value> }
#[derive(Serialize, Deserialize)]
pub struct InterruptedCommand { pub program: String, pub interrupted_at: String }
```

2. Implement `TaskState::save(dir: &Path)` using atomic write:
   write to `<dir>/<id>.tmp`, then `fs::rename` to `<dir>/<id>.json`.
3. Implement `TaskState::load(dir: &Path, id: &str) -> Result<TaskState>`.
4. On resume, any `command_history` entry without an exit code is marked
   `interrupted = true`.
5. Read `VEX_STATE_DIR` from env (default `.vex/state`). Create the directory if
   absent.

---

## Definition of Done

1. `TaskState::save` writes a valid JSON file atomically.
2. `TaskState::load` returns the same state that was saved.
3. A state file written then loaded has identical `status`, `changed_files`,
   `active_grants`, `conversation_snapshot`, and `interrupted_sessions`.
4. `CommandEvidence` entries with `exit_code: None` have `interrupted = true` after
   reload.
5. `active_grants` keys are `Capability` enum variants, not raw strings.
6. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_task_state_survives_atomic_write_and_reload`

```rust
#[test]
fn test_task_state_survives_atomic_write_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let state = TaskState {
        id: "task-001".into(),
        status: TaskStatus::Completed,
        active_grants: std::collections::HashMap::from([
            (Capability::ApplyPatch, ApprovalScope::Once),
        ]),
        changed_files: vec![std::path::PathBuf::from("src/main.rs")],
        command_history: vec![
            CommandEvidence { program: "cargo test".into(), exit_code: None, interrupted: true },
        ],
        conversation_snapshot: ConversationCheckpoint::default(),
        interrupted_sessions: vec![
            InterruptedCommand { program: "cargo build".into(), interrupted_at: "2026-03-01T00:00:00Z".into() },
        ],
    };
    state.save(dir.path()).expect("save failed");
    let loaded = TaskState::load(dir.path(), "task-001").expect("load failed");
    assert_eq!(loaded.status, TaskStatus::Completed);
    assert_eq!(loaded.changed_files, state.changed_files);
    assert!(loaded.command_history[0].interrupted);
    assert_eq!(loaded.interrupted_sessions.len(), 1);
}
```

**What NOT to do:**
- Do not wire `TaskState` into the TUI in this task — that is FEAT-19.
- Do not modify `src/app.rs`, `src/state/`, or `src/api/`.
- Do not add new `UiUpdate` variants.
- Do not implement multi-task concurrency; one active task at a time is the scope.
