# ADR-027: Primary-Terminal Overlay TUI and Interactive Command Passthrough

**Date:** 2026-03-13
**Status:** Accepted
**Deciders:** Core maintainer
**Related tasks:** PK-04, CORE-15, CORE-16, CORE-17
**ADR chain:** ADR-006, ADR-007, ADR-018, ADR-019, ADR-024
**Supersedes:** ADR-018, ADR-019

## Context

The managed TUI work landed on a full-screen terminal path that entered the
alternate screen buffer and treated command execution as transcript-managed
output. That cut against the product requirement that the live runtime remain a
terminal-native surface and that operators retain access to the shell context
that existed before `vex` launched.

At the same time, the `!<command>` command surface from ADR-024 exists to run
workspace commands without starting a model turn. In practice, the earlier
capture-first path still kept those commands inside the TUI transcript instead
of yielding the terminal to the subprocess.

## Decision

1. The interactive TUI runs as a primary-terminal overlay.
   - Do not enter the alternate screen buffer.
   - Do not clear the full viewport on each render.
   - Render the active status/history/input panes in a bottom-anchored overlay
     region so earlier shell output remains visible above the agent UI on tall
     terminals.

2. Operator-invoked command surfaces yield terminal control while they run.
   - `!<command>` executes with inherited stdio after approval and without a
     model turn.
   - `/run` and `/test` execute their validation commands directly in the
     terminal and report compact exit summaries back into the transcript.
   - The runtime re-enters overlay mode after the subprocess exits.

3. Internal machine-readable capture remains allowed for non-operator paths that
   need structured output.
   - Edit-loop and validation retry plumbing may continue to use captured tails
     when the runtime needs them for model-visible diagnostics.
   - This ADR changes the operator-facing command surface, not the existence of
     every captured subprocess helper inside the runtime core.

## Implementation Notes

- `src/terminal.rs`
  - remove alternate-screen enter/leave from setup/restore
  - add `TerminalGuard` for temporary yield and overlay re-entry
- `src/bin/vex.rs`
  - stop polling/rendering while a passthrough command owns the terminal
  - render the idle TUI in a bottom-anchored overlay layout
- `src/ui/layout.rs`
  - add overlay layout helpers for the chat view and task view
- `src/ui/render.rs`
  - render task layout inside the overlay region instead of the full viewport
- `src/runtime/command.rs`
  - add a passthrough helper for inherited-stdio subprocess execution
  - ignore parent `SIGINT` while an interactive passthrough command is active
    so Ctrl+C reaches the child without tearing down `vex`
- `docs/src/commands.md`
  - document overlay rendering and terminal-native command output behavior

## Consequences

### Positive

- Pre-launch shell scrollback stays available during interactive use.
- `!<command>`, `/run`, and `/test` show output where terminal users expect it:
  in the terminal itself.
- Interactive subprocesses no longer compete with a full-screen alternate-screen
  takeover path.

### Trade-offs

- The transcript now records a compact summary for passthrough commands instead
  of replaying their stdout/stderr line-by-line.
- Interactive passthrough requires explicit frontend suspension while the child
  owns the terminal.

## Compliance Notes

- Do not reintroduce alternate-screen entry for the main interactive runtime.
- Interactive passthrough commands must yield terminal control before spawn and
  re-enter overlay mode afterward.
- The `Capability::RunCommand` approval gate remains mandatory for `!<command>`
  and hook-owned subprocesses.
