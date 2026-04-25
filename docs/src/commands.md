# CLI and TUI Commands

This page documents the commands and options implemented in the current binary.

## CLI

### `vex`

Starts the interactive full-screen CLI UI. While a task is running, the task
surface uses the ratatui-native renderer for a human-readable header, optional
changed-file row, a full-height transcript body above the composer, and a
larger multiline composer. Tool calls, waiting-state telemetry, and assistant
responses stream into transcript paragraphs on that shared body instead of a
dedicated visible timeline strip. When completed turns record usage metadata, the
header appends a compact `~N.Nk ctx` cumulative session indicator. The prompt
surface keeps active `/` command hints, active `@path` file suggestions, a current
character count and focus marker in the composer header, submit-time `@path`
expansion, pasted blocks, and multiline editing available in the same
fullscreen layout. The composer auto-fits to the available display rows and
columns as the window grows, shrinks, or snaps to smaller layouts, so the
prompt surface reflows instead of holding onto a stale fixed-height block. For
repo-overview prompts, the runtime now steers the model
toward `list_files` at the workspace root or `codebase_search` before any
targeted `read_file`; `read_file` itself requires an explicit non-empty path.

#### Top-level options

The normalized CLI surface defines exactly eleven top-level flags. No
per-subcommand `--json`, `--format`, `--output`, `--force`, `--dir`,
`--host`, `--port`, `--stdin`, `--from-env`, `--subdir`, `--task`,
`--task-file`, `--max-turns`, or `--auto-approve` flags are accepted; the
functionality of those removed flags is either subsumed by the flags below or
is available through configuration.

| Option | Short | Effect |
|:---|:---:|:---|
| `--force-unstable-alignment` | `-f` | Grants `AutoApproveScope::Task` at startup, bypassing per-tool confirmation for the lifetime of the process. |
| `--project-map-only PROMPT` | `-p` | Executes a single batch turn with PROMPT text and terminates. When stdin is not a TTY, stdin content is prepended to PROMPT before routing. |
| `--expand-sector-view` | `-e` | Sets `expand_context = true` in the resolved `Config`, raising the file-scan cardinality bound during context assembly. |
| `--recall-coordinates [TASK_ID]` | `-r` | Loads a persisted `TaskState` before starting the session. When TASK_ID is absent, the most-recently-modified state file in the search path is selected. |
| `--bypass-integrity-locks` | `-b` | Sets `bypass_policy = true`, disabling durable-state disk-policy enforcement for the current process. |
| `--view-intended-trajectory` | `-v` | Resolves `ToolPolicy::Plan`, restricting the tool set to read-only operations. Mutually exclusive with `-t`. |
| `--use-alternate-navigator MODEL` | `-n` | Overrides `Config::model_name` with MODEL for this invocation. |
| `--display-internal-telemetry` | `-d` | Configures the logging layer to emit `tracing` events at `DEBUG` severity or above. Equivalent to `RUST_LOG=debug`. |
| `--telemetry-json` | — | Serializes the log stream as newline-delimited JSON. Requires `-d` to produce output. |
| `--restrict-payload-tools` | `-t` | Resolves `ToolPolicy::Plan`, restricting the tool set to the safe read-and-search subset. Mutually exclusive with `-v`. |
| `--set-map-encoding FORMAT` | `-m` | Selects the output encoding for batch and subcommand output. `"jsonl"` emits newline-delimited JSON turn records; `"text"` (default) emits plain text or Markdown. |

### `vex -r [task-id]` or `vex --recall-coordinates [task-id]`

Resumes a saved task. With no task id, resumes the most-recent saved task.

### `vex -p "PROMPT"` or `vex --project-map-only "PROMPT"`

Runs one prompt turn and prints the result to stdout. If stdin is piped, the
stdin content is prepended to the prompt. For local inference endpoints,
protocol discovery runs before the turn starts when you configure a bare
`api_client.base_url`. On dual-protocol servers, discovery probes
`/v1/messages` first and falls back to `/v1/chat/completions` only when the
messages-v1 probe fails. If `model_url` already points at a concrete endpoint
path, that explicit path remains authoritative for the turn.

### `vex exec`

Runs a non-interactive single-turn batch task. Task content is supplied via
`-p/--project-map-only`. Output format is controlled by
`-m/--set-map-encoding`. Auto-approval scope is derived from
`-f/--force-unstable-alignment`.

Each JSONL turn record includes a `tokens` object with `input`, `output`, and
`estimated` fields.

### `vex doctor`

Runs a read-only environment health check. Validates config loading, checks
model endpoint reachability, reports sandbox fallback status, probes configured
MCP servers without starting them, inspects state-directory writability, and
verifies that any present policy file parses cleanly.

Exit code is non-zero only when one or more checks fail. Pass
`-m jsonl` to emit a JSON array of `{check,status,message}` objects instead
of the default plain-text report.

### `vex privacy`

Prints the current privacy summary for the CLI and LocalApiServer surfaces.

The output is structured text so it can be reviewed in an interactive shell, written to a
file, or copied into operator-workstation documentation.

### `vex credentials <set|get|delete|list>`

Manages the OS credential-store entries used by the runtime token fallback.

- `vex credentials set <account>` — acquires the secret using automatic source
  selection: (1) reads from stdin when stdin is not a TTY (pipe or redirect);
  (2) presents a masked interactive prompt when both stdin and stderr are
  terminals. The secret is never accepted as a positional argument, preventing
  leakage into shell history and process lists.
  Example: `printf '%s' "$VEX_MODEL_TOKEN" | vex credentials set model-token`
- `vex credentials get <account>` — prints the stored secret.
- `vex credentials delete <account>` — removes the stored secret.
- `vex credentials list` — prints the known account identifiers for the
  `"vexcoder"` service.

### `vex export <task-id>`

Exports a saved task from `.vex/state` (or `VEX_STATE_DIR`) to stdout.

Format is governed by `-m/--set-map-encoding`:
- `"jsonl"` — JSONL batch-turn records, matching the schema produced by `vex exec`.
- `"text"` (default) — Markdown, omitting full assistant response text and
  including only tool outcomes.

Output is always written to stdout; redirect with shell operators when a file
target is needed.

### `vex init`

Scaffolds `.vex/config.toml`, `.vex/validate.toml`, and `AGENTS.md` in the
current working directory. Non-destructive: files that already exist are
skipped without error. To target a different directory, use `cd` before
invoking `vex init`.

### `vex branch <name>`

Creates and switches to a new git branch from `HEAD`.

If a saved task state exists, VexCoder records the branch name on the most
recent task file in `.vex/state` (or `VEX_STATE_DIR`).

### `vex pr-summary`

Builds a diff from the current branch against the merge-base of the default
remote branch (`origin/HEAD`) and runs one model turn to propose a PR title and
body.

The result prints to stdout. The current template starts with a `Title:` line
followed by a Markdown body, so you can review it on your workstation or pipe it into your
own git-hosting CLI workflow.

### `vex completions <bash|zsh|fish|powershell>`

Writes shell completion scripts to stdout.

### `vex install-hooks` and `vex uninstall-hooks`

Installs or removes the repository `prepare-commit-msg` hook.

### `vex skills list`

Lists installed skills.

### `vex skills remove NAME`

Removes an installed skill by name.

## TUI slash commands

Commands entered inside the interactive UI start with `/`.

### Session and task state

- `/new` — save the current task and start a fresh session with a new task ID.
- `/resume [task-id]` — restore a previously saved task. Lists recent tasks when no ID is given.
- `/compact` — reset conversation history, turn evidence, and token counters while keeping the current task ID and permission grants. Use this to recover from context-window overflow or to free up context budget.
- `/fork [label]` — save the current task and start a new task seeded with the same grants.
- `/undo` — revert the last file-modifying tool call from the in-memory checkpoint stack. Binary-safe: restores raw bytes for text and binary files and removes rename destinations when applicable. Returns a diagnostic when the stack is empty or when undo is disabled via `[undo] enabled = false`.
- `/quit` / `/exit` — end the session.
- `/about` — show version and build info.

### Memory

- `/memory`
- `/memory add <note>`
- `/memory clear`
- `/memory auto on` — enable automatic memory extraction for the current session. After each assistant turn, short factual notes are extracted and appended to the notes file with `[auto]` tags.
- `/memory auto off` — disable automatic memory extraction for the current session.
- `/memory auto clear` — remove all `[auto]`-tagged notes from the notes file.

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
  - Expands `@path` mentions inside the instruction before the edit loop starts so picked files can be inlined as context.
  - Grants task-scoped `write-file`, `apply-patch`, and `run-command` permissions for the active edit workflow unless that capability is already session-scoped.
- `/fix`
  - Restores the edit loop from the last validation failure and re-seeds the same task-scoped edit permissions without reducing existing session grants.

### Read-only semantic turns

- `/explain [path]`
  - Accepts either a plain workspace-relative path or `@path`; `@path` is normalized to the requested file target before context assembly runs.
- `/review [--base <git-ref>] [--files <glob>] [<instruction>]`
  - Starts a single review turn without entering the edit loop.
  - With no options, reviews `git diff HEAD`.
  - `--base <git-ref>` reviews `git diff <git-ref>` after validating the ref.
  - `--files <glob>` assembles matching workspace files instead of a diff and cannot be combined with `--base`.
  - Expands `@path` mentions inside the free-form review instruction before the review turn starts. When `--files` receives `@glob`, the leading `@` is stripped before file matching.
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
- `/mcp [list|show <server>]`
  - Zero-turn MCP inspection surface.
  - `/mcp` and `/mcp list` show loaded servers, transports, and tool counts.
  - `/mcp show <server>` lists the server's fully qualified `mcp.<server>.<tool>` names.
  - If no servers are loaded, the transcript shows `[mcp] no MCP servers loaded`.
- `/tools [desc]`
  - Zero-turn tool inventory.
  - Always shows built-in tools and retrieval/mutation guidance.
  - Includes loaded MCP tools under a dedicated `[tools:mcp]` section.
  - `/tools desc` adds one-line descriptions from the tool schemas.
- `/usage`
- `/commands`
- `/help`

When a read-only turn asks for a repo summary instead of a specific file, the
runtime prefers `list_files` and `codebase_search` first. If the model emits a
`read_file` call without a concrete path, VexCoder returns a clarification
instead of looping the raw tool error, even when the malformed `read_file`
arrives in the same parallel tool round as other read-only calls.

`/usage` prints the most recent turn's token counts and the cumulative session
totals. If the runtime does not return usage metadata, the values are estimated
from character counts and marked `(estimated)`. `/new` and `/compact` reset the
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
- `/reindex`
  - Rebuilds the codebase structural index in the background without blocking the TUI. Reports completion back to the transcript when finished.
  - Refuses to run when `[search].enabled = false`.

### Free-form input transforms

- `@path`
  - Expands a workspace-relative file or directory into the prompt when the turn is submitted.
  - While composing, the prompt footer searches the entire repo tree, including nested subdirectories, ranks matches by basename and path relevance, and keeps a bounded top-ranked candidate set per keystroke instead of sorting the full workspace on every keypress.
  - When a file mention is active, `Up` and `Down` move the suggestion picker through the full match list, `Enter` inserts the selected workspace-relative path into the composer, and `Esc` dismisses the picker so the raw mention can still be submitted unchanged.
  - Files are inlined as fenced text blocks. Missing paths are annotated inline instead of aborting the turn.
  - Directories render a compact workspace-relative listing.
  - Slash commands with free-form instructions (`/edit`, `/plan`, `/review`) expand selected `@path` mentions before the model turn starts. `/explain` treats `@path` as the requested file target.
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
| `list_dir` | Non-recursive directory listing. Workspace-confined and `.gitignore`-aware. Optional `path` (defaults to workspace root); optional `max_entries` (default 200, hard cap 500). |
| `glob_files` | Workspace-wide glob matching. `.gitignore`-aware with bounded results. Required `pattern` (supports `*`, `**`, `?`, `[abc]`, `[a-z]`, `[^x]`); optional `max_results` (default 50, hard cap 200). |
| `codebase_search` | Search the structural index for functions, types, and code patterns by name or keyword. Returns ranked code snippets with file paths and line numbers. When embeddings are configured, also performs semantic reranking. Prefer this over `read_file` for exploring unfamiliar code. |
| `git_status` | Show git repository status. |
| `git_diff` | Show git diff output. |

### Mutating tools

| Tool | Purpose |
| :--- | :--- |
| `write_file` | Write full file content. Files above `VEX_DIFF_PREFERRED_ABOVE_LINES` (default 200) trigger a warning suggesting `apply_patch` or `edit_file`. Files above `VEX_WRITE_FILE_MAX_LINES` (default 500) are rejected. |
| `edit_file` | Replace one exact unique snippet (`old_str` → `new_str`). Preferred for targeted edits. Transcript previews keep multiline diff hunks so added and removed rows stay visible during review. |
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
- Parent scope match
- Content keyword match (per word)

Results are capped at `VEX_SEARCH_MAX_RESULTS` (default 10). When an
embedding provider is configured (`VEX_EMBEDDING_PROVIDER`), results are
additionally reranked by semantic similarity using the persisted vector index
at `.vex/index/`.

## Error handling

### Context-overflow recovery

When the conversation exceeds the server's context window, VexCoder detects
the overflow from the HTTP 400 response body and provides actionable guidance:

- **Local endpoints:** suggests restarting the server with a larger context
  size (e.g. `--ctx-size 8192`) or using `/compact` to reset the conversation.
- **Remote endpoints:** suggests using `/compact` to reset the conversation.

The server's error message is shown verbatim, capped at 300 characters.

For non-context-overflow HTTP 400 errors from local endpoints, the error
includes the inferred protocol (MessagesV1 vs ChatCompat) and suggests
checking the model name, protocol format, and whether the server supports
streaming.

## Keyboard notes

- `Ctrl+C` requests cancellation for the active turn.
- `Alt+Up` and `Alt+Down` move the selected entry in the adaptive task timeline.
- `Tab` and `Shift+Tab` also move timeline selection forward and backward while the task surface is active.
- The visible timeline window scales with display height instead of staying fixed at six rows.
- The composer auto-fits to the current display row and column budget, so snapping the display to half-screen or quarter-screen sizes reflows the prompt surface instead of overflowing or leaving empty space.
- `PageUp`, `PageDown`, `Ctrl+Up`, and `Ctrl+Down` scroll the transcript/output pane upward from the prompt edge instead of moving the cursor.
- `Ctrl+Home` jumps to the oldest visible transcript content, and `Ctrl+End` returns to the current bottom edge.
- The transcript pane keeps the full session scrollback visible while follow mode is on; new model responses append at the bottom instead of replacing the prior response view.
- Transcript scrolling follows wrapped display rows, so long paragraphs, embedded newlines, and multiline diff previews remain reachable in both fullscreen and fallback transcript views.
- Selecting older timeline entries manually switches the output pane into inspector detail for that step until follow mode resumes.
- `Shift+Enter` inserts a newline without submitting the turn.
- Pasted text is inserted into the larger multiline prompt surface during normal editing.
- The composer header shows a current focus indicator (`focused` / `unfocused`) and a character count that updates as you type.
