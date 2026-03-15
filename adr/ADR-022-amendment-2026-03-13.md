# ADR-022 Amendment: Command Execution (2026-03-13)

**Date:** 2026-03-13
**Status:** Amended
**Related:** ADR-018, ADR-019, ADR-027, ADR-030
**Supersedes:** Previous ADR-022 command execution section

## Command Execution (AMENDED 2026-03-13)

**Aligned with the current full-screen command-capture runtime pattern.**

Command execution uses **full capture** for agent observability:

- `Stdio::piped()` for stdout/stderr (not inherit)
- Output rendered inside the managed transcript from captured stdout/stderr
- Agent maintains full visibility of captured child-process output
- Enables mid-task approvals, interruptions, reasoning

### Implementation Details

```rust
let mut command = Command::new(&req.program);
command
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .stdin(Stdio::piped())
    .kill_on_drop(true);

// Validate working directory before spawn
if let Some(dir) = &req.working_dir {
    if !dir.exists() || !dir.is_dir() {
        return Err(CommandError::InvalidWorkingDir(dir.clone()));
    }
    command.current_dir(dir);
}
```

### Signal Handling

- Ctrl+C during command: Forwarded to the active command session via `kill_on_drop`
- Ctrl+C during LLM: Cancels current request
- Ctrl+Q: Clean application exit

### PTY Support

- Interactive tools (vim, top) use PTY emulation
- Known limitation: some terminal features may not work perfectly
- Future enhancement: full tokio-pty integration

## Roadmap Updates

| Task | Status | Notes |
|------|--------|-------|
| Command capture | Complete | Captured in the managed transcript for command-session runs |
| Signal handling | Complete | `kill_on_drop(true)` + process group kill on cancel |
| PTY support | Partial | Basic emulation via `portable_pty`, full tokio-pty pending |
| Working dir validation | Complete | Already in command.rs |
| Layout underflow | Complete | Saturating arithmetic in layout.rs |
| Concurrent validation | Complete | ValidationSuite::run executes commands concurrently in the workspace working dir |
| run_command tool | Complete | Model-visible dispatch uses the same command runner contract and workspace working dir |
| Edit loop | Complete | assemble→model→apply→validate→retry in edit_loop.rs |

## Compliance

All future command tools must:
1. Use capture path (no passthrough)
2. Render captured stdout/stderr inside the managed session output
3. Validate working directory before spawn
4. Support timeout and cancellation
5. Keep managed command-session lifetime under runtime ownership even when the
   provider stream has already ended
