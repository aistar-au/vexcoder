# CLI and TUI Commands

This page documents the commands and flags implemented in the current binary.

## CLI

### `vex`

Starts the interactive full-screen CLI UI. While a task is running, the task
surface uses a direct ANSI renderer for a human-readable header, optional
changed-file row, adaptive timeline, prompt-anchored transcript area, and a
larger multiline composer. When completed turns record usage metadata, the
header appends a compact `~N.Nk ctx` cumulative session indicator. The prompt
surface keeps live `/` command hints, live `@path` file suggestions, a live
character count and focus marker in the composer header, submit-time `@path`
expansion, pasted blocks, and multiline editing available in the same
fullscreen layout. For repo-overview prompts, the runtime now steers the model
toward `list_files` at the workspace root or `codebase_search` before any
targeted `read_file`; `read_file` itself requires an explicit non-empty path.

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
  - Expands `@path` mentions inside the instruction before the plan turn starts.
  - Never enters the edit loop; patch requests are silently denied during the turn.
- `/init [environment]`
  - Scaffolds `.vex/config.toml`, `.vex/validate.toml`, and `AGENTS.md` in the current workspace.
  - Reports the selected environment label in the transcript when one is supplied.
- `/context`
- `/tools [desc]`
- `/usage`
- `/commands`
- `/help`

When a read-only turn asks for a repo summary instead of a specific file, the
runtime prefers `list_files` and `codebase_search` first. If the model emits a
`read_file` call without a concrete path, VexCoder returns a clarification
instead of looping the raw tool error.

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
  - While composing, the prompt footer shows matching file suggestions from the current workspace subtree.
  - When a file mention is active, `Up` and `Down` move the suggestion picker, `Enter` inserts the selected workspace-relative path into the composer, and `Esc` dismisses the picker so the raw mention can still be submitted unchanged.
  - Files are inlined as fenced text blocks. Missing paths are annotated inline instead of aborting the turn.
  - Directories render a compact workspace-relative listing.
  - Repo summaries still need tool evidence: use a plain prompt when you want the model to start with `list_files` or `codebase_search`, and use `@path` only when you already know the file or directory you want to inline.
- `!command`
  - Runs a shell command immediately from the workspace without starting a model turn when the composer is submitted.
  - Uses the same `run_command` approval gate as tool calls.
  - Starts a captured command session inside the managed TUI instead of yielding
    control back to the parent CLI session.
  - The transcript records the command, PID, streamed output, and final
    `[command session exit: N]` status.

## Tool inventory

The model can invoke the following tools during a turn. Read-only tools run
without confirmation; mutating tools require operator approval (or a
session/capability auto-approval grant).

### Read-only tools

| Tool | Purpose |
| :--- | :--- |
| `read_file` | Read file content from an explicit non-empty path. Accepts `offset` (1-based line) and `limit` for partial reads. For repo overviews, use `list_files` or `codebase_search` first. |
| `list_files` | List files and directories under a path, or the workspace root when omitted. Prefer this for initial repo exploration. |
| `list_directory` | Alias for `list_files`. |
| `search_files` | Search text across files and return matching lines. |
| `search` | Alias for `search_files`. |
| `find_files` | Find files by name pattern (glob) within the workspace. |
| `codebase_search` | Search the structural index for functions, types, and code patterns by name or keyword. Returns ranked code snippets with file paths and line numbers. When embeddings are configured, also performs semantic reranking. Prefer this over `read_file` for exploring unfamiliar code. |
| `git_status` | Show git repository status. |
| `git_diff` | Show git diff output. |

### Mutating tools

| Tool | Purpose |
| :--- | :--- |
| `write_file` | Write full file content. Files above `VEX_DIFF_PREFERRED_ABOVE_LINES` (default 200) trigger a warning suggesting `apply_patch` or `edit_file`. Files above `VEX_WRITE_FILE_MAX_LINES` (default 500) are rejected. |
| `edit_file` | Replace one exact unique snippet (`old_str` → `new_str`). Preferred for targeted edits. |
| `apply_patch` | Apply full-file content as a patch. Preferred for large-scale changes where `edit_file` is impractical. |
| `rename_file` | Rename or move a file within the workspace. |
| `run_command` | Execute a shell command in the workspace. |

### Search ranking

`codebase_search` uses a Tree-sitter-based structural index that extracts
functions, structs, enums, impls, traits, modules, constants, and type
aliases from Rust source files. The index is built at session start and
updated incrementally on file writes.

Results are scored by:

- Exact name match: highest priority
- Substring / fuzzy name match
- Content keyword match (per word)
- Recency of last modification

Results are capped at `VEX_SEARCH_MAX_RESULTS` (default 10). When an
embedding provider is configured (`VEX_EMBEDDING_PROVIDER`), results are
additionally reranked by semantic similarity using the persisted vector index
at `.vex/index/`.

## Error handling

### Context-overflow recovery

When the conversation exceeds the server's context window, VexCoder detects
the overflow from the HTTP 400 response body and provides actionable guidance:

- **Local endpoints:** suggests restarting the server with a larger context
  size (e.g. `--ctx-size 8192`) or using `/clear` to reset the conversation.
- **Remote endpoints:** suggests using `/clear` to reset the conversation.

The server's error message is shown verbatim (truncated to 300 characters).

For non-context-overflow HTTP 400 errors from local endpoints, the error
includes the detected protocol (MessagesV1 vs ChatCompat) and suggests
checking the model name, protocol format, and whether the server supports
streaming.

## Keyboard notes

- `Ctrl+C` requests cancellation for the active turn.
- `Alt+Up` and `Alt+Down` move the selected entry in the adaptive task timeline.
- `Tab` and `Shift+Tab` also move timeline selection forward and backward while the task surface is active.
- The visible timeline window scales with terminal height instead of staying fixed at six rows.
- `PageUp`, `PageDown`, `Ctrl+Up`, and `Ctrl+Down` scroll the transcript/output pane upward from the prompt edge instead of moving the cursor.
- `Ctrl+Home` jumps to the oldest visible transcript content, and `Ctrl+End` returns to the live bottom edge.
- The transcript pane keeps the full session scrollback visible while follow mode is on; new model responses append at the bottom instead of replacing the prior response view.
- Selecting older timeline entries manually switches the output pane into inspector detail for that step until follow mode resumes.
- `Shift+Enter` inserts a newline without submitting the turn.
- Pasted text is inserted into the larger multiline prompt surface during normal editing.
- The composer header shows a live focus indicator (`focused` / `unfocused`) and a character count that updates as you type.
