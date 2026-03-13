# ADR-027: Full-Screen TUI With Command-Session Capture

**Date:** 2026-03-13
**Status:** Accepted (amended from overlay-passthrough to full-screen capture)
**Deciders:** Core maintainer
**Related tasks:** CORE-15, CORE-16, PE-01, PJ-03
**ADR chain:** ADR-018, ADR-019, ADR-022
**Supersedes:** ADR-018, ADR-019 (corrective amendment)
## Context

Previous discussions considered an inline overlay pattern that would keep
pre-launch shell history visible. This ADR instead chooses the hosted-agent
style full-screen session model:

1. Full-screen TUI provides better focus for complex agent tasks
2. Command capture enables agent observability (approvals, reasoning,
   interruptions)
3. This matches user expectation for "agent takeover" workflow

## Decision

1. **Interactive sessions own the full terminal**
   - The TUI enters the alternate screen buffer for normal interactive use.
   - Pre-launch shell scrollback is hidden during the session and restored on exit.

2. **Normal command execution stays inside the TUI**
   - Inline `!command` execution is a captured command session, not parent-shell passthrough.
   - Captured stdout/stderr are rendered inside the managed transcript.
   - The UI surfaces command string, PID when available, and session status.

3. **Command execution uses one runtime contract**
   - Working directory validation is mandatory before spawn.
   - Child cleanup uses `kill_on_drop(true)` and process-tree termination on cancel.
   - Validation and model-visible `run_command` calls use the same command runner contract as inline command sessions.

4. **Interactive terminal tools remain a distinct path**
   - PTY attach remains available for tools that require a real terminal.
   - Full async PTY integration is deferred.

## Landed Implementation

### Terminal lifecycle
- `src/terminal.rs` enters the alternate screen, clears the terminal, and restores terminal state on exit.
- `src/bin/vex.rs` renders the task layout against the full viewport instead of the earlier bottom-overlay path.

### Command-session runtime
- `src/runtime/command.rs` captures stdout/stderr with `Stdio::piped()`, validates the working directory, and uses `kill_on_drop(true)` for spawned commands.
- Cancellation terminates the command tree with `kill -9 -- -<pgid>` on Unix and `taskkill /F /T` on Windows.
- `src/app.rs` starts captured `!command` sessions, renders command/PID/status rows in the activity area, and keeps command output inside the transcript.

### Validation and model tool execution
- `src/runtime/validation.rs` runs validation commands concurrently and now accepts a working directory for each spawned command.
- `src/runtime/edit_loop.rs` carries the active working directory into validation runs.
- `src/state/conversation/tools.rs` routes model-visible `run_command` through the same command runner and workspace working directory rather than a separate direct process spawn.

### Working Directory Handling
- `WorkingDirCommandRunner` was removed from the TUI path.
- `src/app.rs` now sets the working directory directly on each `CommandRequest` when starting a captured command session.
- `src/runtime/command.rs` validates that directory with `validate_working_dir()` before spawn for one-shot, streaming, and PTY-backed execution.
- Validation and model-visible command execution also pass the workspace working directory directly on `CommandRequest` instead of relying on a wrapper runner struct.

## Signal Handling

| Context | Behavior |
|---------|----------|
| TUI rendering | Ctrl+C cancels current LLM request |
| Command execution | Ctrl+C requests cancellation; the command runner terminates the active command session tree |
| Input mode | Esc clears input, Enter submits |
| Application exit | Ctrl+Q quits cleanly, restores terminal |

## Current Limits And Follow-ups

- Model-visible `run_command` still returns a completed tool result; it does not yet stream live command output into the model tool loop.
- Concurrent inline command sessions now share the same managed transcript, but saved task evidence still records the batch at task-turn level rather than as independent structured session records.
- PTY-backed interactive tools still depend on `portable_pty` through `src/runtime/command.rs::attach_pty()`. That dependency remains live in this branch and was not removed by the full-screen capture cutover. A full async PTY integration remains future work.

## Regression Coverage

- `tests/layout_underflow_tests.rs` covers the small-terminal layouts called out during the cutover (`10x3`, `20x5`, `40x12`) and asserts that three-pane and four-region splits stay bounded within the viewport without panicking.
- `tests/signal_handling_tests.rs` covers the command-session cancellation path and the runtime turn-cancellation token reset path that back the interactive Ctrl+C behavior.
- `src/app.rs` retains the command-session cancellation regression around inline `!command` execution and turn completion.

## Consequences

### Positive
- Matches the full-screen hosted-agent session model
- Agent has full observability of captured command output
- Enables approval workflows and mid-task interruptions without yielding back to the parent shell
- Clean, focused agent session (no shell distraction)
- Aligns inline command sessions, validation, and model-visible command execution around one runtime contract

### Negative / Trade-offs
- Pre-launch shell history not visible during session
- Long command output pushes transcript off-screen (user must scroll TUI)
- Interactive tools require PTY emulation (some limitations)
- Moves project away from the inline overlay companion-shell pattern

## Compliance Notes

- Do not reintroduce inherited-stdio passthrough for normal command sessions
- Do not remove alternate screen from the interactive session lifecycle
- Working directory validation remains mandatory before spawn
- Command execution must use the runtime command runner and sandbox contract
