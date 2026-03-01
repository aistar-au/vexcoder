# Task FEAT-18: Command Cancellation and PTY

**Target File:** `src/runtime/command.rs`

**ADR:** ADR-022 §Execution and Approval Model

**Depends on:** FEAT-17 (`test_command_runner_one_shot_captures_exit_code_and_stdout` must be green)

---

## Issue

`DefaultCommandRunner` from FEAT-17 supports one-shot and streaming execution but does
not yet support cancellation or PTY attachment. ADR-022 requires cancellation via
SIGINT/SIGTERM with a `Cancelling` status transition, and PTY attach for interactive
commands.

---

## Decision

1. Extend `CommandHandle` to carry a `tokio::sync::oneshot::Sender<()>` cancel signal.
2. Implement `cancel()`: send the cancel signal and deliver SIGINT (Unix) or
   `TerminateProcess` (Windows) to the child process.
3. Add `CancellationStatus` to the result or expose it via the handle so callers can
   observe the `Cancelling` → `Failed`/`Completed` transition.
4. Add `attach_pty()` to `CommandRunner` trait and implement via the `portable-pty`
   crate (or equivalent). PTY sessions expose a read half as an `OutputChunk` stream.
5. PTY lifecycle: dropping the `PtySession` closes the master fd and waits for the
   child.

---

## Definition of Done

1. `runner.cancel(handle)` terminates a running child and returns `Ok(())`.
2. A cancelled streaming run does not deadlock or panic.
3. `attach_pty()` compiles and spawns a child in a PTY on Linux.
4. Integration test covering cancellation race completes within 2 seconds.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_command_runner_cancel_transitions_to_cancelling`

```rust
#[tokio::test]
async fn test_command_runner_cancel_transitions_to_cancelling() {
    let runner = DefaultCommandRunner::new();
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    let req = CommandRequest { program: "sleep".into(), args: vec!["30".into()] };
    let handle = runner.run_streaming(req, tx).await.expect("spawn failed");
    runner.cancel(handle).await.expect("cancel failed");
    // must complete without hanging; child must be gone
}
```

**What NOT to do:**
- Do not add approval gating in this task — that is CORE-16.
- Do not modify anything outside `src/runtime/command.rs`.
- Do not add new `UiUpdate` variants or touch the TUI.
- Do not make PTY mandatory for one-shot or streaming paths.
