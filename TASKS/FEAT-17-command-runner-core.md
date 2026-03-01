# Task FEAT-17: Command Runner Core

**Target File:** new `src/runtime/command.rs`, `src/runtime.rs`

**ADR:** ADR-022 Phase 2

**Depends on:** REF-09 (`test_model_backend_kind_parses_from_env_var` must be green)
**Parallel-safe with:** CRIT-19 (different target files)

---

## Issue

No general command execution capability exists in the current codebase. `ToolOperator`
provides file and git helpers only. ADR-022 requires `CommandRunner` as a first-class
built-in capable of one-shot and streaming execution with structured output evidence.

---

## Decision

1. Add `src/runtime/command.rs` with these types:

```rust
pub struct CommandRequest { pub program: String, pub args: Vec<String> }
pub struct CommandResult  { pub exit_code: i32, pub stdout: String, pub stderr: String }
pub struct OutputChunk    { pub stream: StreamKind, pub text: String }
pub enum   StreamKind     { Stdout, Stderr }
pub struct CommandHandle  { /* opaque cancel token */ }

pub trait CommandRunner: Send + Sync {
    async fn run_one_shot(&self, req: CommandRequest) -> Result<CommandResult>;
    async fn run_streaming(
        &self, req: CommandRequest,
        tx: tokio::sync::mpsc::Sender<OutputChunk>,
    ) -> Result<CommandHandle>;
    async fn cancel(&self, handle: CommandHandle) -> Result<()>;
}
```

2. Add `DefaultCommandRunner` backed by `tokio::process::Command`.
3. Export from `src/runtime.rs`.
4. One-shot captures full stdout+stderr and exit code.
5. Streaming spawns the child and forwards chunks to `tx` until completion or cancel.

---

## Definition of Done

1. `CommandRunner` trait and `DefaultCommandRunner` compile.
2. One-shot execution of a real command returns correct exit code and output.
3. Streaming execution delivers at least one `OutputChunk` before the process exits.
4. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_command_runner_one_shot_captures_exit_code_and_stdout`

```rust
#[tokio::test]
async fn test_command_runner_one_shot_captures_exit_code_and_stdout() {
    let runner = DefaultCommandRunner::new();
    let req = CommandRequest { program: "echo".into(), args: vec!["hello".into()] };
    let result = runner.run_one_shot(req).await.expect("run failed");
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("hello"));
}
```

**What NOT to do:**
- Do not wire `CommandRunner` into the approval system in this task — that is CORE-16.
- Do not modify `src/tools/operator.rs`, `src/app.rs`, or `src/state/`.
- Do not add PTY support in this task — that is FEAT-18.
- Do not add new `UiUpdate` variants or touch the TUI render path.
