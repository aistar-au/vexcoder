# CLI and TUI Commands

This page documents the commands and flags implemented in the current binary.

## CLI

### `vex`

Starts the interactive terminal UI as a primary-terminal overlay. The bottom
status/transcript/input panes stay inside the current terminal instead of
switching to an alternate screen buffer, so pre-launch shell scrollback remains
available.

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
  - Run directly in the terminal without starting a model turn.
  - Command output stays in the terminal; the transcript keeps a compact summary
    with per-command exit status.

### Free-form input transforms

- `@path`
  - Expands a workspace-relative file or directory into the prompt before the model turn starts.
  - Files are inlined as fenced text blocks. Missing paths are annotated inline instead of aborting the turn.
  - Directories render a compact workspace-relative listing.
- `!command`
  - Runs a shell command immediately from the workspace without starting a model turn.
  - Uses the same `run_command` approval gate as tool calls.
  - Yields terminal control while the command runs, so interactive subprocesses
    use the parent terminal directly.
  - The transcript records that output was shown in the terminal plus the final
    `[exit: N]` status.

## Keyboard notes

- `Ctrl+C` requests cancellation for the active turn.
- Pasted text is inserted into the input box during normal editing.
