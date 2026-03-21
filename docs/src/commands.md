# CLI and TUI Commands

This page documents the commands and flags implemented in the current binary.

## CLI

### `vex`

Starts the interactive full-screen CLI UI. While a task is running, the task
surface uses a direct ANSI renderer for a human-readable header, optional
changed-file row, adaptive timeline, prompt-anchored transcript area, and a
larger multiline composer. When completed turns record usage metadata, the
header appends a compact `~N.Nk ctx` cumulative session indicator. The prompt
surface keeps submit-time `/` commands, submit-time `@path` expansion, pasted
blocks, and multiline editing available in the same fullscreen layout.

### `vex --resume [task-id]`

Resumes a saved task. With no task id, VexCoder offers recent tasks for
selection.

### `vex -p "PROMPT"` or `vex --print "PROMPT"`

Runs one prompt turn and prints the result to stdout. If stdin is piped, the
stdin content is prepended to the prompt.

### `vex exec --task "TEXT"`

Runs a non-interactive batch task.

Useful flags:

- `--task-file PATH`
- `--max-turns N`
- `--auto-approve once|task`
- `--format jsonl|text`
- `--output PATH`

Each JSONL turn record includes a `tokens` object with `input`, `output`, and
`estimated` fields.

### `vex doctor [--json]`

Runs a read-only environment health check. It validates config loading, checks
model endpoint reachability, reports sandbox fallback status, probes configured
MCP servers without starting them, inspects state-directory writability, and
verifies that any present policy file parses cleanly.

Exit code is non-zero only when one or more checks fail. `--json` emits a JSON
array of `{check,status,message}` objects.

### `vex export <task-id> [--format jsonl|markdown] [--output PATH] [--force]`

Exports a saved task from `.vex/state` (or `VEX_STATE_DIR`).

- `jsonl` matches the batch-turn schema used by `vex exec`
- `markdown` omits full assistant response text and only includes tool outcomes
- `--output PATH` writes to a file instead of stdout
- `--force` allows overwriting an existing output file

### `vex init [--dir PATH]`

Creates `.vex/config.toml`, `.vex/validate.toml`, and `AGENTS.md` without
overwriting existing files.

### `vex branch <name>`

Creates and switches to a new git branch from `HEAD`.

If a saved task state exists, VexCoder records the branch name on the most
recent task file in `.vex/state` (or `VEX_STATE_DIR`).

### `vex pr-summary`

Builds a diff from the current branch against the merge-base of the default
remote branch (`origin/HEAD`) and runs one model turn to draft a PR title and
body.

The result prints to stdout. The current template starts with a `Title:` line
followed by a Markdown body, so you can review it locally or pipe it into your
own git-hosting CLI workflow.

### `vex migrate config [--output PATH]`

Writes a TOML fragment based on legacy environment variables.

### `vex completions <bash|zsh|fish|powershell>`

Writes shell completion scripts to stdout.

### `vex install-hooks` and `vex uninstall-hooks`

Installs or removes the repository `prepare-commit-msg` hook.

### `vex skills list`

Lists installed skills.

### `vex skills install SOURCE [--subdir PATH]`

Installs a skill from a git URL or tarball URL.

### `vex skills remove NAME`

Removes an installed skill by name.

## TUI slash commands

Commands entered inside the interactive UI start with `/`.

### Session and task state

- `/new`
- `/resume [task-id]`
- `/clear`
- `/fork [label]`
- `/quit`
- `/exit`
- `/about`

### Memory

- `/memory`
- `/memory add <note>`
- `/memory clear`

### Permissions

- `/permissions`
- `/allow <capability> [once|session]`
- `/deny <capability>`

### Model and diff helpers

- `/model`
- `/model <name>`
- `/diff`
- `/diff --staged`

### Edit loop

- `/edit <instruction>`
- `/fix`

### Read-only semantic turns

- `/explain [path]`
- `/review [--base <git-ref>] [--files <glob>] [<instruction>]`
  - Starts a single review turn without entering the edit loop.
  - With no flags, reviews `git diff HEAD`.
  - `--base <git-ref>` reviews `git diff <git-ref>` after validating the ref.
  - `--files <glob>` assembles matching workspace files instead of a diff and cannot be combined with `--base`.
  - Patch requests are silently denied during the turn.
- `/plan <instruction>`
  - Generates a concise implementation plan for the given instruction.
  - Assembles workspace context via `ContextAssembler`; renders `plan_template.txt`.
  - Never enters the edit loop; patch requests are silently denied during the turn.
- `/context`
- `/tools [desc]`
- `/usage`
- `/commands`
- `/help`

`/usage` prints the most recent turn's token counts and the cumulative session
totals. If the runtime does not return usage metadata, the values are estimated
from character counts and marked `(estimated)`. `/new` and `/clear` reset the
session totals.

### Test generation

- `/generate-tests [path] [--framework <name>]`
  - Starts a single semantic turn using the test-generation prompt template.
  - Assembles context for the requested path, or the most recently assembled file when no path is provided.
  - Only test-file mutations are allowed; source-file edits must use `/edit`.

### Custom commands

- `/.vex/commands/*.toml`
- `~/.config/vex/commands/*.toml`
  - Custom slash commands load at session start from project and user command directories.
  - Project-scoped commands override user-scoped commands with the same name.
  - Templates support `{{context}}` and `{{input}}` substitution.

### Validation helpers

- `/run [command]`
- `/test`
  - Run without starting a model turn.
  - Command output is captured for the transcript, with per-command stdout,
    stderr, and exit status summarized after each command completes.

### Free-form input transforms

- `@path`
  - Expands a workspace-relative file or directory into the prompt when the turn is submitted.
  - Files are inlined as fenced text blocks. Missing paths are annotated inline instead of aborting the turn.
  - Directories render a compact workspace-relative listing.
- `!command`
  - Runs a shell command immediately from the workspace without starting a model turn when the composer is submitted.
  - Uses the same `run_command` approval gate as tool calls.
  - Starts a captured command session inside the managed TUI instead of yielding
    control back to the parent CLI session.
  - The transcript records the command, PID, streamed output, and final
    `[command session exit: N]` status.

## Keyboard notes

- `Ctrl+C` requests cancellation for the active turn.
- `Alt+Up` and `Alt+Down` move the selected entry in the adaptive task timeline.
- `Tab` and `Shift+Tab` also move timeline selection forward and backward while the task surface is active.
- The visible timeline window scales with terminal height instead of staying fixed at six rows.
- `PageUp`, `PageDown`, `Ctrl+Up`, and `Ctrl+Down` scroll the transcript/output pane upward from the prompt edge instead of moving the cursor.
- `Ctrl+Home` jumps to the oldest visible transcript content, and `Ctrl+End` returns to the live bottom edge.
- `Shift+Enter` inserts a newline without submitting the turn.
- Pasted text is inserted into the larger multiline prompt surface during normal editing.
