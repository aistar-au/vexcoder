# ADR-022 Amendment: Command Execution (2026-03-13)

**Date:** 2026-03-13
**Status:** Amended
**Related:** ADR-018, ADR-019, ADR-027
**Supersedes:** Previous ADR-022 command execution section

## Command Execution (AMENDED 2026-03-13)

**Aligned with Codex CLI / Copilot CLI pattern.**

Command execution uses **full capture** for agent observability:

- `Stdio::piped()` for stdout/stderr (not inherit)
- Output streamed to transcript via `StreamBlock` events
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
| Command capture | Pending | ADR-027 implementation |
| Signal handling | Pending | Ctrl+C propagation redesign |
| PTY support | Partial | Basic emulation, full tokio-pty pending |
| Working dir validation | Complete | Already in command.rs |
| Layout underflow | Pending | Saturating arithmetic in layout.rs |

## Compliance

All future command tools must:
1. Use capture path (no passthrough)
2. Stream output via `StreamBlock` events
3. Validate working directory before spawn
4. Support timeout and cancellation
