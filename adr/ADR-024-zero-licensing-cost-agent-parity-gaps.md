# ADR-024: Zero-Licensing-Cost Agent Parity Gaps

**Status:** Proposed (pre-release items complete; PG-03 tap auto-release pending external repo)  
**Chain:** ADR-022, ADR-023, ADR-014, ADR-006

## Context

A structured gap analysis against available open-source reference implementations identified 35 feature gaps outside ADR-022 and ADR-023 scope, covering sandboxing, headless mode, layered config, MCP, distribution, skills, and config-import and environment-onboarding tooling.

## Decision

### Config and instructions
- Layered config precedence: enterprise policy → user `~/.vex/config.toml` → trusted project `.vex/config.toml` → defaults; later layers override earlier.
- Project instructions file at `.vex/AGENTS.md` injected into system prompt when present.
- All config keys are neutral; no provider-branded names.

### Execution and sandboxing
- Non-interactive execution mode (`vex exec --task "…"`) exits 0 on `TaskStatus::Completed`, non-zero on `Failed`, `ApprovalDenied`, or `MaxTurnsReached`.
- OS-level sandbox (`pledge`/`seccomp` on Linux, `sandbox-exec` on macOS) wraps tool calls; network access denied by default.

### MCP
- MCP server integration via `mcp.servers` config table; servers started as subprocesses.
- Shell completions generated for Bash, Zsh, Fish via `vex completions <shell>`.

### Runtime and sessions
- Runtime model switching via `VEX_MODEL_NAME` / `VEX_MODEL_URL` or `/model` slash command.
- `@<path>` inline file injection appends `read_file` result to the active pulse.
- `!<command>` inline shell passthrough calls one-shot and appends output.
- User-defined slash commands in `.vex/commands/` TOML files.
- `/tools` enumerates active tool schemas; `/diff` renders working-tree diff zero-pulse.
- Pre/post tool-call hooks in config `[[hooks]]` array; `on_fail` values: `warn`, `block`, `ignore`.

### Memory and state
- `/memory` persists user notes to `~/.vex/notes.md`; appended, never overwritten.
- `vex export` serializes session as JSONL to stdout or file.
- `--resume <task-id>` restores task state from `.vex/state/`.
- Session-level token counter displayed in status bar.

### Distribution
- Binary distribution via GitHub Releases; macOS package via Homebrew tap.
- Skills registry discovery from `~/.vex/skills/` and `.vex/skills/` directories.

### Workspace tools
- `read_file`, `write_file`, `list_directory`, `search_files`, `run_command` registered as built-in tools.
- `codebase_search` (ADR-033 gap 12) indexes Rust symbols via `tree-sitter`.

## References

- [`tree-sitter`](https://docs.rs/tree-sitter) — workspace symbol indexing
- [`serde_json`](https://docs.rs/serde_json) — JSONL export ([RFC 8259](https://www.rfc-editor.org/rfc/rfc8259))
- MCP specification: <https://spec.modelcontextprotocol.io/>
- `pledge(2)` / `seccomp(2)` — kernel-enforced syscall filtering
