# ADR-027: Codex/Copilot CLI Alignment — Full-Screen TUI with Command Capture

**Date:** 2026-03-13
**Status:** Accepted (amended from overlay-passthrough to full-screen capture)
**Deciders:** Core maintainer
**Related tasks:** CORE-15, CORE-16, PE-01, PJ-03
**ADR chain:** ADR-018, ADR-019, ADR-022
**Supersedes:** ADR-018, ADR-019 (corrective amendment)
**Target Alternatives:** OpenAI Codex CLI, GitHub Copilot CLI

## Context

Previous discussions considered an "overlay TUI" pattern (similar to Aider) that
would keep pre-launch shell history visible. After comparing against industry
leaders:

| Tool | TUI Pattern | Command Output | Target User Experience |
|------|-------------|----------------|----------------------|
| OpenAI Codex CLI | Full-screen alternate buffer | Captured in transcript | Focused agent session |
| GitHub Copilot CLI | Full-screen alternate buffer | Captured in transcript | Focused agent session |
| Aider | Inline overlay | Native passthrough | Shell companion |

The decision was made to align with **Codex CLI and Copilot CLI** rather than
Aider, as:

1. Full-screen TUI provides better focus for complex agent tasks
2. Command capture enables agent observability (approvals, reasoning,
   interruptions)
3. This matches user expectation for "agent takeover" workflow

## Decision

1. **Full-screen TUI** (alternate screen buffer)
   - `EnterAlternateScreen` + `terminal.clear()` retained
   - Pre-launch scrollback hidden during session (acceptable trade-off)
   - Clean visual separation between agent and shell

2. **Command output captured in transcript**
   - `Stdio::piped()` for stdout/stderr
   - Output streamed to transcript via `StreamBlock`
   - Agent maintains full observability of captured command-session output
   - Enables mid-task approvals, interruptions, reasoning

3. **Follow-up implementation requirements**
   - Working directory validation before spawn
   - Ctrl+C propagation to the active command session during captured runs
   - Layout underflow protection via saturating arithmetic
   - `kill_on_drop(true)` for child-process cleanup

4. **PTY support for interactive tools**
   - Interactive tools (vim, top) use PTY emulation
   - Known limitation: some terminal features may not work perfectly
   - Future enhancement: full tokio-pty integration

## Implementation

### terminal.rs
- Add `EnterAlternateScreen` / `LeaveAlternateScreen`
- Add signal handler setup/cleanup
- Remove overlay-specific functions (see Dead Functions below)

### command.rs
- Keep `Stdio::piped()` capture (Codex pattern)
- Add working directory validation
- Add `kill_on_drop(true)` for cleanup
- Add timeout support
- Stream output to transcript via `StreamBlock`
- Remove passthrough path (see Dead Functions below)

### layout.rs
- Add saturating arithmetic to prevent underflow
- Add comprehensive test suite for edge cases
- Add `get_recommended_heights()` for responsive layouts
- Remove overlay layout helpers (see Dead Functions below)

### loop.rs
- Keep full render loop with all four panes
- Add proper Ctrl+C handling (cancel request)
- Add Ctrl+Q handling (quit application)

## Signal Handling

| Context | Behavior |
|---------|----------|
| TUI rendering | Ctrl+C cancels current LLM request |
| Command execution | Ctrl+C forwarded to the active command session via `kill_on_drop` |
| Input mode | Esc clears input, Enter submits |
| Application exit | Ctrl+Q quits cleanly, restores terminal |

## Functions Flagged for Removal

The following functions, structs, and constants become dead code under the
full-screen capture pattern and should be removed in follow-up implementation
PRs.

### src/terminal.rs

| Symbol | Kind | Reason |
|--------|------|--------|
| `terminal_supports_overlay()` | fn (private) | Full-screen mode always enters alternate buffer; no overlay detection needed |
| `enter_overlay_mode()` | fn (private) | Replaced by `setup()` with `EnterAlternateScreen`; overlay concept removed |
| `reenter_overlay()` | fn (pub) | No yield/re-enter cycle in full-screen pattern; terminal stays in alternate buffer |
| `TerminalGuard` | struct (pub) | Passthrough yield pattern removed; commands run inside the TUI transcript |
| `TerminalGuard::yield_for_command()` | method (pub) | Same as above — no terminal yielding in capture-first pattern |

**Callers that must be updated:**
- `src/runtime/command.rs:1` — imports `TerminalGuard`
- `src/runtime/command.rs:126` — calls `TerminalGuard::yield_for_command()`

### src/runtime/command.rs

| Symbol | Kind | Reason |
|--------|------|--------|
| `run_passthrough_one_shot()` | method (pub) | All commands use capture path; no inherited-stdio passthrough |
| `ParentSigintGuard` | struct (private, unix) | SIGINT suppression not needed when the command session is captured instead of inheriting parent-terminal stdio |
| `ParentSigintGuard::ignore()` | method | Same as above |
| `PtySession` | struct (pub) | Replaced by stub PTY in capture path; `portable_pty` dependency removable |
| `PtySession::read_output()` | method (pub) | Same as above |
| `attach_pty()` (real impl) | method | Full `portable_pty` implementation replaced by capture-with-PTY stub |
| `OutputChunk` | struct (pub) | Replaced by `StreamBlock` events for transcript streaming |
| `StreamKind` | enum (pub) | Same as above — stream identification moves to `StreamBlock` variant |
| `CommandHandle.cancel_tx` | field | Cancellation mechanism changes to `kill_on_drop` + timeout |
| `cancel()` | trait method | Same as above — oneshot cancel replaced by process kill |

**Callers that must be updated:**
- `src/app.rs:611,660` — calls `run_passthrough_one_shot()`
- `src/runtime.rs:24-25` — re-exports `OutputChunk`, `StreamKind`
- `src/state/conversation/tools.rs:153,195` — uses `DefaultCommandRunner::new()`, `run_one_shot()`

### src/ui/layout.rs

| Symbol | Kind | Reason |
|--------|------|--------|
| `MAX_OVERLAY_HISTORY_ROWS` | const | Overlay sizing constants not needed in full-screen mode |
| `MAX_OVERLAY_INPUT_ROWS` | const | Same |
| `MAX_TASK_OVERLAY_ROWS` | const | Same |
| `bottom_overlay_area()` | fn (pub) | Bottom-anchored overlay concept removed; full viewport available |
| `split_overlay_three_pane_layout()` | fn (pub) | Replaced by full-screen three-pane layout |
| `split_overlay_four_region_layout()` | fn (pub) | Replaced by full-screen four-region layout |
| `ThreePaneLayout` | struct (pub) | Struct return type replaced by tuple `(Rect, Rect, Rect)` |
| `FourRegionLayout` | struct (pub) | Struct return type replaced by tuple `(Rect, Rect, Rect, Rect)` |

**Callers that must be updated:**
- `src/bin/vex.rs:18,410` — calls `split_overlay_three_pane_layout()`
- `src/ui/render.rs:5,215,216` — imports and calls `split_overlay_four_region_layout()`, `bottom_overlay_area()`

### src/ui/render.rs

| Symbol | Kind | Reason |
|--------|------|--------|
| `render_task_layout()` | fn (pub) | Uses overlay four-region layout; must be rewritten for full-screen four-region |

**Note:** `render_task_layout()` is not removed, but must be rewritten to use
the new full-screen `split_four_region_layout()` instead of the overlay variant.

### Dependency removal

| Dependency | Reason |
|------------|--------|
| `portable_pty` | Real PTY replaced by capture stub; full PTY via `tokio-pty` is a future enhancement |

## Consequences

### Positive
- Matches Codex CLI / Copilot CLI user experience
- Agent has full observability of captured command output
- Enables approval workflows, mid-task interruptions
- Clean, focused agent session (no shell distraction)
- Documents the required follow-up fixes for validation, signals, and layout

### Negative / Trade-offs
- Pre-launch shell history not visible during session
- Long command output pushes transcript off-screen (user must scroll TUI)
- Interactive tools require PTY emulation (some limitations)
- Moves project away from Aider pattern (but aligns with Codex/Copilot)

## Compliance Notes

- All command tools must use capture path (no passthrough)
- Do not remove alternate screen (required for Codex pattern)
- `StreamBlock` events required for agent observability
- Working directory validation mandatory before spawn

## Migration Plan

1. Apply source patches to terminal.rs, command.rs, layout.rs, loop.rs
2. Update callers in app.rs, runtime.rs, vex.rs, render.rs
3. Remove `portable_pty` from Cargo.toml
4. Add integration tests for signal handling
5. Document PTY limitations in user guide

## Test Requirements

- `tests/command_capture_tests.rs` (new)
  - Command output captured in transcript
  - Working directory validation
  - Signal propagation (Ctrl+C)
  - Timeout handling
- `tests/layout_underflow_tests.rs` (new)
  - Small terminal stability (10x3, 20x5, 40x12)
  - No panic on any terminal size
- `tests/signal_handling_tests.rs` (new)
  - Ctrl+C during command execution
  - Ctrl+C during LLM streaming
  - Ctrl+Q clean exit

## Competitive Positioning

| Feature | Codex CLI | Copilot CLI | vexcoder (ADR-027) |
|---------|-----------|-------------|-------------------|
| Full-screen TUI | Y | Y | Y |
| Command capture | Y | Y | Y |
| Agent approvals | Y | Y | Y |
| Open source | Y | N | Y |
| Model flexibility | Limited | GitHub only | Any OpenAI-compatible |
| Self-hostable | N | N | Y |
| Zero licensing cost | N | N | Y |
| Batch mode | Limited | Limited | Y Full |
| Session notes | N | N | Y |
| Git hooks | Basic | Basic | Y Full |
