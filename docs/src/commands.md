# CLI and TUI Commands

This page documents the commands and flags implemented in the current binary.

## CLI

### `vex`

Starts the interactive terminal UI.

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

### `vex init [--dir PATH]`

Creates `.vex/config.toml`, `.vex/validate.toml`, and `AGENTS.md` without
overwriting existing files.

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
- `/commands`
- `/help`

### Validation helpers

- `/run [command]`
- `/test`

### Free-form input transforms

- `@path`
  - Expands a workspace-relative file or directory into the prompt before the model turn starts.
  - Files are inlined as fenced text blocks. Missing paths are annotated inline instead of aborting the turn.
  - Directories render a compact workspace-relative listing.
- `!command`
  - Runs a shell command immediately from the workspace without starting a model turn.
  - Uses the same `run_command` approval gate as tool calls and records stdout/stderr plus `[exit: N]` in the transcript.

## Keyboard notes

- `Ctrl+C` requests cancellation for the active turn.
- Pasted text is inserted into the input box during normal editing.
