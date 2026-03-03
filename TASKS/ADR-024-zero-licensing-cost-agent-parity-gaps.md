# ADR-024: Zero-Licensing-Cost Agent Parity Gaps — Sandboxing, Headless Mode, Layered Config, MCP, Distribution, Skills, and Migration

**Date:** 2026-03-03  
**Status:** Proposed  
**Deciders:** Core maintainer  
**Location:** `TASKS/ADR-024-zero-licensing-cost-agent-parity-gaps.md`  
**ADR chain:** ADR-022 (as amended 2026-03-03 — amendment status: Proposed, must be locked before Phases G–H begin), ADR-023 (deterministic edit loop), ADR-014, ADR-006  
**Related:** `TASKS/ADR-022-free-open-coding-agent-roadmap.md` (zero-licensing-cost roadmap), `TASKS/ADR-023-deterministic-edit-loop.md`

---

## Scope Note

ADR-022 covers command execution, diff-native writes, capability-based approval, durable task state, and TUI task orientation. ADR-023 covers the deterministic edit loop, context assembly, model profiles, and semantic commands. This ADR covers the remaining feature gaps identified by a structured comparison against reference implementations that fall outside both prior ADRs' scope, including distribution, native packaging, skills registry, and migration tooling.

---

## Context

ADR-022 locked the first-milestone roadmap for `vexcoder` as a coding agent whose runtime and packaging dependencies carry exclusively permissive, no-cost licenses. A structured comparison against available reference implementations reveals the following gaps.

### Dependency licensing constraint

Every direct dependency of `vexcoder` must be licensed under a permissive, royalty-free license — specifically MIT, Apache 2.0, or a dual MIT/Apache 2.0 offering — such that building, distributing, and operating the application imposes no licensing fee, royalty obligation, or copyright assignment requirement on any party. This is the operative reason the project uses Rust (MIT/Apache 2.0) and ratatui (MIT): neither the language toolchain, the TUI framework, nor any crate in the dependency graph charges a licensing fee or restricts redistribution. The same constraint applies to all future Rust crate dependencies added under this ADR. Any crate carrying a commercial license, a copyleft license that would require source disclosure of this codebase, or a license that conditions use on a paid tier is prohibited without a dedicated ADR recording an explicit exception and its legal basis.

**Operational and runtime dependency scope:** This ADR also introduces optional operational dependencies — Docker (Apache 2.0, used by `DockerSandbox`), npm-distributed MCP server packages (licenses vary per package), Homebrew (BSD 2-Clause), and GitHub Actions CI tooling (license varies per action). These are not Rust crate dependencies compiled into the binary; they are operator-provided runtime components or CI infrastructure. The licensing constraint for these is therefore different: they are not required for the binary to build or run in `PassthroughSandbox` mode, and operators who use them accept their respective license terms independently. However, for long-term multi-year legal clarity the following rules apply:

- **Docker (`DockerSandbox`):** Docker Engine is Apache 2.0 for the community edition. Docker Desktop has a separate commercial license that applies to certain business uses. The ADR does not bundle Docker; operators install it independently. Documentation must note that operators using Docker Desktop in a commercial context must verify their Docker Desktop licensing.
- **MCP server packages:** The `[[mcp_servers]]` config allows operators to configure arbitrary npm packages as tool servers. `vexcoder` makes no representation about the licenses of third-party MCP packages. Documentation must note that operators are responsible for verifying the license of any MCP server package they configure.
- **CI tooling (GitHub Actions, `cross`, mingw toolchain):** These are build and release infrastructure, not runtime components. Their licensing does not affect the distributed binary's license obligations.
- **Homebrew tap:** The tap formula is maintained under the same license as the `vexcoder` repository.

### Gaps addressed by this ADR

| # | Gap | Status |
| :--- | :--- | :--- |
| 1 | No OS-level sandboxing | Proposed |
| 2 | No non-interactive execution mode | Proposed |
| 3 | No layered configuration | Proposed |
| 4 | No project instructions file | Proposed |
| 5 | No MCP server integration | Proposed |
| 6 | No shell completions | Proposed |
| 7 | No git commit attribution | Proposed |
| 8 | No runtime model switching | Proposed |
| 9 | No binary distribution pipeline | Proposed (post-first-milestone) |
| 10 | No skills registry or discovery mechanism | Proposed |
| 11 | No migration tooling for operators | Proposed |
| 12 | Code search / indexing | Formally deferred |
| 13 | No interactive permission-control command surface | Proposed |
| 14 | No session-lifecycle command surface | Proposed |
| 15 | No MCP command-level management surface | Proposed (extends Gap 5) |

### Gaps intentionally deferred by this ADR

| Gap | Rationale |
| :--- | :--- |
| Image/screenshot input | Deferred until the model backend seam (ADR-022 Phase 1) is stable and a multimodal local runtime target exists |
| Multi-agent / parallel task execution | Out of scope for the first milestone per ADR-022 Decision item 5 (single active task) |
| Cloud task delegation | Deferred indefinitely; contradicts the self-hostable, zero-licensing-cost posture established by the dependency licensing constraint above |
| Built-in web search | Depends on MCP (Gap 5). Implementing web search before MCP exists would permanently couple it to the core runtime |
| IDE extensions | Deferred to a post-first-milestone ADR per ADR-022 amendment Decision item 11. File-based editor extensions must use `vex exec` (Gap 2). Native GUI surfaces (IDE panels with live streaming, macOS native client) must use the `LocalApiServer` path reserved in Phase I |
| Conversation compaction / context-window management | Long-running sessions that approach the model's context limit have no managed strategy for pruning or summarising old turns. `ConversationCheckpoint` in `TaskState` records a `message_count` and `summary` string but neither is populated nor acted upon by the runtime today. Implementing compaction requires a dedicated ADR: the summarisation prompt, the trigger threshold, and whether the summary is injected as a system message or a synthetic turn all affect model behaviour and must be decided deliberately. Deferred until the edit loop and BatchMode are stable — compaction adds the most value for long `vex exec` runs, and those require BatchMode to exist first. **Command-surface note:** reference CLIs expose active context management commands (`/compact`, `/usage`). ADR-023 `EL-12` introduces `/context` for read-only token-estimate display. `/compact` (trigger summarisation) and a richer `/usage` (per-tool token attribution) are part of this deferred gap and must not be implemented without the dedicated compaction ADR. This gap is a formal deferral gate: do not implement conversation pruning or summarisation without a dedicated ADR. |

---

## Sequencing guard

**Phases G and H (distribution and macOS packaging) are post-first-milestone** and must not block milestone-1 correctness work (ADR-022 phases 1–8 and ADR-023 edit loop). They may not begin until the edit loop, approval system, and task state persistence are validated end-to-end. Any dispatcher that begins Phase G or H work before those milestones are green must be considered out of scope.

**Phase I (local API server) is post-Phase-H** and requires a dedicated ADR specifying wire protocol, local socket authentication, and streaming response format before any dispatcher begins work.

---

## Decision

This ADR locks decisions for gaps 1–11 and gaps 13–15. Gap 12 is formally deferred with rationale recorded.

---

### Gap 1 — OS-level Sandboxing

Introduce an opt-in `SandboxDriver` abstraction as a required pre-dispatch wrapper around `CommandRunner` (ADR-022). The active driver is selected from `VEX_SANDBOX` or the config `sandbox` key at startup and must be applied to every `CommandRequest` before it reaches `CommandRunner`.

**Drivers:**

| Driver | Behaviour |
| :--- | :--- |
| `PassthroughSandbox` | No containment. Default. Preserves current behaviour. |
| `MacosSandboxExec` | Wraps command in `sandbox-exec -p <profile>`. Best-effort only — see deprecation note. |
| `DockerSandbox` | Wraps command in `docker run --rm <image> <args>`. Recommended stable containment path. |

**`sandbox-exec` deprecation note:** `sandbox-exec` has been deprecated since macOS 10.15. `MacosSandboxExec` is best-effort: if `sandbox-exec` is unavailable or returns a non-zero exit on the probe call, the runtime must emit a clear warning and fall back to `PassthroughSandbox`. The fallback is suppressed and the runtime must instead abort if the operator sets `sandbox_require = true` in config. This distinction is critical: silent containment failure is a safety issue.

The sandbox boundary applies to the execution layer only. `ApprovalPolicy` (ADR-022) is evaluated before dispatch; `SandboxDriver` is a secondary containment layer after approval. These two layers are separate and independently configurable.

```rust
// src/runtime/sandbox.rs

pub trait SandboxDriver: Send + Sync {
    fn driver_kind(&self) -> SandboxKind;
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest>;
}

pub enum SandboxKind {
    Passthrough,
    MacosSandboxExec,
    Docker,
}
```

---

### Gap 2 — Non-interactive Execution Mode (`vex exec`)

Introduce `BatchMode: RuntimeMode + FrontendAdapter` as the headless complement to `TuiMode`. `BatchMode` reads a task prompt from `--task` or `--task-file`, runs the agent loop to completion or `--max-turns`, and writes structured turn evidence to stdout or `--output <path>` in JSONL or plain-text format. No `ratatui` or `crossterm` dependencies are introduced.

Approval policy in `BatchMode` defaults to the capability policy file; interactive approval prompts are replaced with `deny` unless `--auto-approve` is passed explicitly with a scope.

```bash
vex exec --task "refactor src/foo.rs to use the new error type" \
         [--auto-approve once|task] \
         [--max-turns N] \
         [--output path] \
         [--format jsonl|text]
```

`BatchMode` is the designated integration point for file-based and CLI editor-surface extensions. Extensions that shell out to `vex exec`, read JSONL, and render it in a panel must use this path rather than embedding the runtime directly. Native GUI surfaces that require richer bidirectional communication should use the `LocalApiServer` path reserved in Phase I below.

---

### Gap 3 — Layered Configuration

Replace flat environment-variable-only configuration with a five-level resolution chain (highest wins):

| Priority | Source | Path |
| :--- | :--- | :--- |
| 1 | Environment variables | `VEX_*` as defined in ADR-022 |
| 2 | Repo-local config | `.vex/config.toml` (first ancestor directory containing `.vex/`) |
| 3 | User config | `~/.config/vex/config.toml` (XDG) or `~/.vex/config.toml` |
| 4 | System config | `/etc/vex/config.toml` |
| 5 | Compiled defaults | Inline `Default` impls |

TOML key names mirror `VEX_*` variable names in snake_case (e.g. `model_url`, `model_name`). `VEX_MODEL_TOKEN` is never read from any config file at any layer — only from the environment. Any config file containing a `model_token` key must be rejected at load time with a diagnostic.

Resolution errors (malformed TOML, unknown keys) are hard failures at startup with a diagnostic pointing to the offending file and key. A missing config file at any layer is not an error.

---

### Gap 4 — Project Instructions File

At session start, `RuntimeContext::start_session` searches for a project instructions file in order: `.vex/AGENTS.md`, `AGENTS.md`, `.vex/PROJECT.md`. The first file found is read and prepended to the system prompt as a verbatim block separated from the base prompt by a labelled delimiter. Files exceeding `VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS` (default: 4096 tokens, estimated at chars ÷ 4) are not injected; a warning is emitted and the file is skipped. The instructions file path is displayed in the TUI session header and in `BatchMode` JSONL output.

---

### Gap 5 — MCP Server Integration

Introduce `McpRegistry` loaded from the **user config file only** (`~/.config/vex/config.toml`) under a `[[mcp_servers]]` table. STDIO servers are launched as managed processes at session start and terminated at session end. HTTP servers are connected by URL. Tools advertised by MCP servers are merged into the tool dispatch table with `mcp.<server_name>.<tool_name>` namespace prefixing to prevent collisions with built-in tools. A new `Capability::McpTool` variant is added with a default approval scope of `once`.

`[[mcp_servers]]` must not be permitted in repo-local config (`.vex/config.toml`). Allowing committed repo config to auto-launch arbitrary processes is a supply-chain risk. Reject with a diagnostic at config load time.

```toml
# ~/.config/vex/config.toml — user config layer only

[[mcp_servers]]
name      = "filesystem"
transport = "stdio"
command   = "npx"
args      = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

[[mcp_servers]]
name      = "search"
transport = "http"
url       = "http://localhost:3000/mcp"
```

---

### Gap 6 — Shell Completions

Add `vex completions <shell>` using `clap_complete`. Supported shells: `bash`, `zsh`, `fish`, `powershell`. Output is written to stdout. No completion scripts are committed to the repository; they are generated at runtime and redirected by the operator.

---

### Gap 7 — Git Commit Attribution

Add `vex install-hooks` that writes a `prepare-commit-msg` hook to `.git/hooks/`. When a `vex` task has recorded changed files in the active `TaskState`, the hook appends a `Co-authored-by: vexcoder <vexcoder@localhost>` trailer and a `Vex-Task-Id: <task_id>` trailer to the commit message. The hook is a minimal POSIX shell script with no external dependencies beyond `git`. `vex uninstall-hooks` removes it. Hook installation is opt-in and never automatic.

---

### Gap 8 — Runtime Model Switching

Add `/model <name>` to `TuiMode::handle_slash_command`. The command updates `RuntimeContext`'s active model name in place; it does not restart the process or reset conversation history. The new model name takes effect on the next `start_turn` call. `/model` with no argument prints the currently active model name. Switching `ModelBackendKind` or `ModelProtocol` mid-session is rejected with a clear error message and leaves context unchanged.

---

### Gap 9 — Binary Distribution Pipeline and macOS Packaging

**Sequencing:** all Phase G and H work is post-first-milestone. See sequencing guard above.

#### Phase G — GitHub Releases pipeline

Add a `release.yml` GitHub Actions workflow triggered on semver tags (`v*.*.*`). The workflow produces pre-built binaries for the following targets:

| Target | CI runner | Notes |
| :--- | :--- | :--- |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | Static binary via musl |
| `aarch64-unknown-linux-musl` | `ubuntu-latest` | Cross-compiled via `cross` |
| `x86_64-apple-darwin` | `macos-latest` | Native runner |
| `aarch64-apple-darwin` | `macos-latest` | Native runner (Apple Silicon) |
| `x86_64-pc-windows-gnu` | `ubuntu-latest` | Cross-compiled via `cross` + mingw toolchain |

**Windows target note:** `x86_64-pc-windows-msvc` requires a Windows CI runner and the MSVC toolchain. `x86_64-pc-windows-gnu` (mingw) is cross-compilable from Linux via `cross` with no Windows runner required. Use `gnu` as the default Windows target. A future ADR may add an `msvc` build on a Windows runner if installer tooling requires it.

Each target produces a compressed archive (`vex-<version>-<target>.tar.gz` or `.zip` for the Windows target) attached to the GitHub Release. A `checksums.txt` file containing `sha256` hashes for all archives is published alongside them.

A Homebrew tap formula (`homebrew-vex`) is maintained as a separate repository. The release workflow updates the tap formula automatically via a repository dispatch event on successful release.

#### Phase H — macOS application wrapper

A native macOS application under `packaging/macos/` that:

- Launches and manages the `vex` binary as a managed process.
- Embeds the compiled `vex` binary in the app bundle at `Contents/MacOS/vex`.
- Reads `VEX_MODEL_TOKEN` from the system keychain via `Security.framework` and injects it as an environment variable into the managed process at launch. It must not write the token to disk.
- Presents a terminal surface (initially: launches the system terminal with the embedded binary; an embedded `NSTextView`-based terminal surface is a separately-scoped follow-up and not required for Phase H correctness).
- Distributes via a `.dmg` attached to GitHub Releases.

**Code signing and notarisation (required for distribution):** the macOS wrapper must be signed with a Developer ID Application certificate and notarised via `xcrun notarytool` before distribution. An unsigned `.dmg` will be blocked by Gatekeeper on every supported macOS version. The release workflow must include a signing and notarisation step. The certificate and App Store Connect API key must be stored as GitHub Actions secrets (`APPLE_DEVELOPER_ID_CERT`, `APPLE_NOTARYTOOL_KEY`). If these secrets are absent, the workflow must skip signing and attach a clearly labelled "unsigned development build" to the release rather than failing silently.

**Phase H boundary constraint:** the native macOS application in Phase H is a packaging and credential layer only. It must not contain agent logic, model calls, conversation state, or tool dispatch. All such logic remains exclusively in the Rust binary. Any PR to `packaging/macos/` that modifies any file under `src/` in the same changeset is out of scope for Phase H and must be rejected.

This constraint applies to Phase H specifically. It does not prohibit a future native macOS client that communicates with a `LocalApiServer: RuntimeMode + FrontendAdapter` (see Phase I below). That path involves adding a new `RuntimeMode` implementation to `src/` — which is an intended use of the runtime trait architecture — and a native macOS client that connects to it over a local socket or loopback interface. The architectural relationship is the same as any API client to a local server; the network path is shorter than a cloud API but the interface contract is identical. Phase I requires a dedicated ADR and must not begin before Phase H and the milestone-1 correctness work are validated end-to-end.

#### Phase I — Local API server surface (reserved)

Formally reserved for a post-Phase-H ADR. The `LocalApiServer` is the third `RuntimeMode + FrontendAdapter` implementation after `TuiMode` and `BatchMode`. It exposes the shared runtime core over a local HTTP or Unix domain socket, enabling rich bidirectional communication with native GUI clients (native macOS application, web frontend, IDE extension) without duplicating any Rust logic in those clients. The server binds to loopback only by default; no external network exposure without an explicit operator configuration and a dedicated ADR.

The relationship to cloud API servers is direct: architecturally, `LocalApiServer` and a cloud-hosted API server are the same construct — a `RuntimeMode` implementation that accepts requests and streams responses. The network path differs (loopback vs internet); the interface contract does not. This means a future cloud-hosted or enterprise-licensed deployment follows the same expansion path: a `RuntimeMode` implementation that routes to a remote transport rather than a local socket.

`LocalApiServer` must not begin implementation until `BatchMode` is validated end-to-end and Phase H is complete. The ADR for Phase I must specify the wire protocol, authentication model for the local socket, and the streaming response format before any dispatcher begins work.

---

### Gap 10 — Skills Registry and Discovery

Introduce a lightweight skills registry backed by `.agents/skills/registry.toml`. Skills are directories containing a `SKILL.md` entrypoint and optional supporting assets. The registry is a flat manifest — no dependency resolution, no semver solver, no transitive install.

```toml
# .agents/skills/registry.toml

[[skills]]
name    = "vex-branch-contract"
version = "1.0.0"
source  = "local"
path    = ".agents/skills/vex-branch-contract"

[[skills]]
name    = "edit-loop"
version = "1.0.0"
source  = "local"
path    = ".agents/skills/edit-loop"
```

New sub-commands:

```bash
vex skills list
vex skills install <source> [--subdir <path>]
vex skills remove <name>
```

**Remote install rules (normative):** `vex skills install` accepts exactly two source types:

1. A **git repository URL** — shallow-cloned; `--subdir <path>` selects a subdirectory within the repo as the skill root.
2. A **tarball URL** (`.tar.gz` or `.zip`) — downloaded and extracted; must contain `<skill-name>/SKILL.md` at its root.

"Raw URL directory fetch" is not supported and must not be implemented. There is no standard mechanism for fetching a directory tree from a raw URL; any implementation would be non-deterministic across hosting providers.

The `vex skills` commands are thin CLI utilities; they do not start the agent loop. Skills are passive workflow documents consumed by agents running in `TuiMode` or `BatchMode`; they are not executable code loaded into the runtime.

---

### Gap 11 — Migration Tooling

Add a `vex migrate config` sub-command that reads the environment for legacy variable names used in pre-ADR-022 vexcoder deployments and emits a `.vex/config.toml` fragment using the current ADR-022 neutral names. The command is non-destructive: it writes to stdout by default; `--output <path>` writes to a file.

**Legacy → current variable mapping:**

| Legacy variable | Current replacement | Notes |
| :--- | :--- | :--- |
| `VEX_API_PROTOCOL=anthropic` | `model_protocol = "messages-v1"` | |
| `VEX_API_PROTOCOL=openai` | `model_protocol = "chat-compat"` | |
| `VEX_STRUCTURED_TOOL_PROTOCOL=on` | `tool_call_mode = "structured"` | |
| `VEX_STRUCTURED_TOOL_PROTOCOL=off` | `tool_call_mode = "tagged-fallback"` | |
| `VEX_MODEL_URL` (full endpoint path) | `model_url` (base URL, endpoint stripped) | Strip `/v1/messages` or `/v1/chat/completions` suffix |

These are vexcoder's own pre-ADR-022 variable names. No third-party SDK variable names are mapped. Any migration from third-party tooling is the operator's responsibility and is documented in `docs/src/migration.md` but not automated.

`docs/src/migration.md` must include the complete legacy-to-current variable mapping table, the `vex migrate config` usage guide, and a command alias reference (`/help` → `/commands`, etc.). The migration doc is the canonical source of truth; `vex migrate config` is a convenience generator that must match it exactly.

---

---

### Gap 13 — Interactive Permission-Control Command Surface

Reference CLIs expose runtime commands that let operators inspect and mutate the active capability grant set without restarting the process. Vexcoder's `active_grants: HashMap<Capability, ApprovalScope>` on `TaskState` is already the correct data structure; this gap adds the command surface to read and write it directly from the TUI.

**Commands added to `try_handle_slash_command` (ADR-023 §6 dispatch):**

```
/permissions
    Renders the current active_grants table to transcript via push_history_line.
    No model turn. Output format:
      [permissions]
        ApplyPatch   : once
        RunCommand   : session
        McpTool      : (none)
    If active_grants is empty, renders "[permissions] no active grants".

/allow <capability> [once|session]
    Grants the named Capability at the specified scope. Scope defaults to "once"
    if omitted. Valid capability names are the kebab-case lowercase of each
    Capability variant (e.g. "apply-patch", "run-command", "mcp-tool").
    Unknown capability name → "[allow: unknown capability '<name>']", no grant.
    Updates active_grants on the live TaskState in-session; does not persist to
    disk (grants are session-scoped by design; TaskState::save is not called).
    Emits "[allow: apply-patch granted for session]" on success.

/deny <capability>
    Removes the named capability from active_grants if present.
    Unknown name or not-currently-granted → emits "[deny: apply-patch not in
    active grants]" and returns without error.
    Emits "[deny: apply-patch removed]" on success.
```

**Constraints:**

- `/allow` and `/deny` must never start a model turn. All output is via `push_history_line`.
- Capability names in the command surface must be derived from the `Capability` enum's variant list at compile time. No hardcoded string list is permitted — the kebab-case conversion must be a function that iterates the enum to prevent silent drift.
- `/allow session` does not persist to `.vex/state/`. Session grants expire when the process exits. Persistence of capability policy belongs to `.vex/config.toml` (ADR-024 Gap 3 layered config), not to interactive grants.
- `/permissions` renders the live `active_grants` from `TuiMode`'s task-state reference, not a cached snapshot.

**Anchor tests:** `test_tui_permissions_renders_empty_grants`; `test_tui_allow_grants_capability_once`; `test_tui_allow_defaults_to_once_scope`; `test_tui_deny_removes_grant`; `test_tui_allow_unknown_capability_emits_error`; `test_tui_allow_does_not_call_start_turn`.

---

### Gap 14 — Session-Lifecycle Command Surface

Reference CLIs expose commands to reset the active session and resume a previously interrupted task. Vexcoder has `TaskState::new`, `TaskState::save`, and `TaskState::load` (confirmed from `src/runtime/task_state.rs`) but no command surface over them.

**Commands added to `try_handle_slash_command`:**

```
/new
    Resets the active session: clears conversation history in RuntimeContext,
    creates a new TaskState with a fresh TaskId (format: "task-<utc-ms>"),
    clears active_edit_loop on TuiMode, and emits
    "[new session: <new-task-id>]" to transcript. The previous TaskState is
    saved to VEX_STATE_DIR before the reset (TaskState::save) so it can be
    resumed. No model turn is started.

/resume [<task-id>]
    Loads a previously saved TaskState from VEX_STATE_DIR via TaskState::load.
    With <task-id>: loads that specific task. Without argument: lists the five
    most recently modified state files and prompts the operator to select by
    number (rendered via push_history_line; input handled via the existing
    overlay input path).
    On successful load: restores active_grants and changed_files from the saved
    state; emits "[resumed: <task-id> status=<status>]".
    Note: conversation history is NOT restored — TaskState does not persist
    message content. The operator resumes with an empty conversation but with
    grants and file-change context intact.
    Unknown or unreadable task-id → "[resume: task '<id>' not found]", no state
    change.
```

**`/undo` — formally deferred:** An `/undo` command would revert the most recently applied patch. This requires either a git-based rollback (which requires the repo to be git-managed and the patch to have been committed or stashable) or a file-snapshot mechanism before each apply. Neither is in scope for this ADR. `/undo` is a formal deferral gate: do not implement it without a dedicated ADR specifying the rollback strategy.

**`/rename` — deferred:** Renaming a task-id after creation is low-priority cosmetic infrastructure. Deferred indefinitely.

**Constraints:**

- `/new` must call `TaskState::save` before resetting. A new session must not begin if the save fails; emit the error and abort.
- `/resume` must not attempt to restore conversation history. `ConversationCheckpoint.message_count` may be displayed informationally; the content is not stored.
- `/resume` without argument must not start a model turn. The selection overlay must use the existing `PendingApproval` input path.
- Both commands must clear `active_edit_loop` on `TuiMode` to prevent stale loop state. `[source: task_state.rs — TaskState::state_dir() for VEX_STATE_DIR resolution]`

**Anchor tests:** `test_tui_new_saves_current_state_before_reset`; `test_tui_new_creates_fresh_task_id`; `test_tui_resume_restores_active_grants`; `test_tui_resume_does_not_restore_conversation`; `test_tui_resume_unknown_id_emits_error`; `test_tui_new_clears_active_edit_loop`.

---

### Gap 15 — MCP Command-Level Management Surface (extends Gap 5)

Gap 5 defined `McpRegistry` config and tool dispatch. Reference CLIs additionally expose runtime commands to inspect which MCP servers are active and what tools they advertise. This gap adds that read-only command surface.

**Commands added to `try_handle_slash_command`:**

```
/mcp list
    Renders all loaded MCP servers from the live McpRegistry to transcript.
    No model turn. Output format:
      [mcp servers]
        my-server   : running  (12 tools)
        other-server: running  (3 tools)
    If McpRegistry is empty: "[mcp] no MCP servers configured".
    If McpRegistry is not yet loaded (session startup still in progress):
    "[mcp] registry not yet available".

/mcp show <server-name>
    Renders all tool names advertised by the named server.
    Output format:
      [mcp: my-server]
        mcp.my-server.read_file
        mcp.my-server.write_file
        ...
    Unknown server name → "[mcp: '<name>' not found]".
```

**Constraints:**

- Both commands are read-only and must never start a model turn or modify `McpRegistry`.
- `McpRegistry` is read-only after session start; `/mcp` commands observe only, never mutate.
- Tool names in `/mcp show` output must use the full `mcp.<server_name>.<tool_name>` namespace as registered in the dispatch table (Gap 5), so operators can use them as references in free-form prompts.
- `/mcp add` and `/mcp remove` are explicitly out of scope for this ADR. Runtime MCP server management (adding servers mid-session) requires dynamic subprocess lifecycle management and a dedicated ADR.

**Anchor tests:** `test_tui_mcp_list_renders_loaded_servers`; `test_tui_mcp_list_empty_registry`; `test_tui_mcp_show_renders_tool_names`; `test_tui_mcp_show_unknown_server_emits_error`; `test_tui_mcp_commands_do_not_start_turn`.

---

### Gap 12 — Code Search / Indexing (Formally Deferred)

A `src/index/` module providing structured code search, symbol lookup, or semantic indexing is explicitly deferred to a post-first-milestone ADR.

`ContextAssembler` (ADR-023) provides sufficient context for the current task scale using pattern-matching-based related-file inference with no external dependencies. A code index is warranted only when real usage evidence shows that pattern matching is insufficient for the tasks being performed. Adding an index before that evidence exists would be premature optimisation that adds a significant dependency surface. This gap is recorded as a formal deferral gate: `src/index/` must not be implemented without a dedicated ADR.

---

## Normative additions

### Environment variables (additions to ADR-022 table)

| Variable | Purpose | Default |
| :--- | :--- | :--- |
| `VEX_SANDBOX` | Sandbox driver: `passthrough`, `macos-exec`, `docker` | `passthrough` |
| `VEX_SANDBOX_PROFILE` | Path to `sandbox-exec` profile or Docker image name | `""` (built-in default) |
| `VEX_SANDBOX_REQUIRE` | Abort rather than fall back if sandbox is unavailable: `true`/`false` | `false` |
| `VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS` | Token budget for project instructions injection | `4096` |

### New `Capability` variant (addition to ADR-022 enum)

```rust
// src/runtime/policy.rs

enum Capability {
    ReadFile,
    WriteFile,
    ApplyPatch,
    RunCommand,
    Network,
    Browser,    // reserved, per ADR-022
    McpTool,    // new — any tool dispatched through McpRegistry
}
```

### Config TOML canonical keys (additions)

```toml
# .vex/config.toml (repo-local) or ~/.config/vex/config.toml (user)

sandbox          = "passthrough"   # or "macos-exec", "docker"
sandbox_profile  = ""              # path or image name; empty = built-in default
sandbox_require  = false           # abort rather than fall back if sandbox unavailable

max_project_instructions_tokens = 4096

# MCP servers — user config ONLY. Rejected in repo-local config.
[[mcp_servers]]
name      = "filesystem"
transport = "stdio"
command   = "npx"
args      = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
```

---

## Migration plan

### Phase A — Layered config, project instructions, migration tooling

| Objective | Completion condition |
| :--- | :--- |
| Replace flat env-var config with layered chain | `VEX_*` env vars override; `.vex/config.toml` and `~/.config/vex/config.toml` loaded and merged; missing files not errors |
| Inject project instructions | Injected when present and within budget; warning emitted and skipped when over budget |
| Ship `vex migrate config` | Maps all legacy `VEX_API_PROTOCOL` / `VEX_STRUCTURED_TOOL_PROTOCOL` values correctly; non-destructive |
| Populate `docs/src/migration.md` | Complete variable rename table, command alias reference, `vex migrate config` usage guide |

**Note:** ModelProfile integration (ADR-023 EL-08 — the `model_profile` TOML key and `VEX_MODEL_PROFILE` env var) is explicitly deferred until after Phase A is locked and green. EL-07 (struct + files) may proceed in parallel; EL-08 may not.

**Note:** `ModelProfile` config integration (ADR-023 EL-08 — the `model_profile` TOML key and `VEX_MODEL_PROFILE` env var) is explicitly gated on Phase A completion. EL-08 must not begin until the layered config chain above is locked and green. This sequencing is normative: ADR-023 EL-07 (struct and profile files) may proceed in parallel; EL-08 may not.

### Phase B — Shell completions, git hooks, skills registry

| Objective | Completion condition |
| :--- | :--- |
| `vex completions <shell>` | Valid completion output for `bash`, `zsh`, `fish`, `powershell` |
| `vex install-hooks` / `vex uninstall-hooks` | Hook writes and removes cleanly; no agent loop started |
| `vex skills list\|install\|remove` | `registry.toml` created and updated correctly |
| Remote install determinism | Only git URL (with optional `--subdir`) or tarball URL accepted; other forms rejected with a diagnostic |

### Phase C — Runtime model switching

| Objective | Completion condition |
| :--- | :--- |
| `/model <name>` changes active model name | Name-only switching works; conversation history preserved |
| `/model` prints current model | No turn started |
| Backend/protocol change rejected | Clear error message; context unchanged |

### Phase D — OS-level sandboxing

| Objective | Completion condition |
| :--- | :--- |
| `SandboxDriver` trait + `PassthroughSandbox` | Passthrough is behaviourally identical to current codebase |
| `MacosSandboxExec` (best-effort) | Wraps `RunCommand`; warns and falls back when unavailable; aborts when `sandbox_require = true` |
| `DockerSandbox` | Runs commands in container when enabled; reports clear error if Docker absent |
| Evidence | Sandbox kind visible in TUI session header and `BatchMode` JSONL output |

### Phase E — Non-interactive execution mode

| Objective | Completion condition |
| :--- | :--- |
| `BatchMode: RuntimeMode + FrontendAdapter` | No `ratatui`/`crossterm` imports |
| `vex exec` sub-command | Runs to completion without TUI |
| Exit codes | 0 on `TaskStatus::Completed` only; non-zero on `Failed`, `ApprovalDenied`, or `MaxTurnsReached`. `MaxTurnsReached` must exit non-zero because the task was not completed — a CI pipeline must not treat it as success |
| JSONL evidence | Includes turn evidence, changed files, command history |

### Phase F — MCP server integration

| Objective | Completion condition |
| :--- | :--- |
| `McpRegistry` with STDIO and HTTP transports | STDIO server launches at session start; tools appear in `/commands` |
| `Capability::McpTool` approval wiring | MCP tool calls trigger approval prompt at `once` scope by default |
| Clean shutdown | Server terminates at session exit |
| Repo-local prohibition | `[[mcp_servers]]` in repo-local config rejected with diagnostic |

### Phase G — Binary distribution pipeline (post-first-milestone)

| Objective | Completion condition |
| :--- | :--- |
| GitHub Releases workflow | Tagging `v*.*.*` produces release with all five target archives |
| Checksums | `checksums.txt` with `sha256` published alongside archives |
| Homebrew tap | Formula updated automatically via repository dispatch |

### Phase H — macOS application wrapper (post-first-milestone)

| Objective | Completion condition |
| :--- | :--- |
| macOS application layer — process management | Launches `vex` process; embeds binary in bundle |
| Keychain credential storage | `VEX_MODEL_TOKEN` sourced from keychain; injected as env var; not written to disk |
| No agent logic (Phase H) | Wrapper contains no runtime, model, or state code. Full native client capability deferred to Phase I (`LocalApiServer`) |
| Code signing and notarisation | Binary signed with Developer ID; notarised via `xcrun notarytool`; unsigned builds labelled clearly |
| Release artifact | `.dmg` attached to GitHub Release |
| Boundary preserved | No PR to `packaging/macos/` requires changes to `src/` |

---

## Validation and acceptance

### Acceptance scenarios

| # | Scenario | Expected result |
| :--- | :--- | :--- |
| 1 | Place `.vex/config.toml` in repo; start `vex` with no `VEX_*` env vars | Config values active |
| 2 | Set `VEX_API_PROTOCOL=anthropic`; run `vex migrate config` | Output contains `model_protocol = "messages-v1"` |
| 3 | `vex exec --task "list Rust source files" --format jsonl` | JSONL to stdout; no TUI |
| 4 | `VEX_SANDBOX=macos-exec`; spawn a command | Wrapped in `sandbox-exec`; warn and fall back when unavailable |
| 5 | `VEX_SANDBOX=macos-exec VEX_SANDBOX_REQUIRE=true`; `sandbox-exec` absent | Process aborts with diagnostic |
| 6 | `vex install-hooks`; commit inside a `vex` task | `Vex-Task-Id` trailer present in commit message |
| 7 | Declare STDIO MCP server in user config; start `vex` | Server tools appear in `/commands`; approval prompted on use |
| 8 | `/model new-model` mid-session | Next turn uses new name; history intact |
| 9 | `vex completions zsh` | Valid zsh completion syntax |
| 10 | Tag `v1.0.0` | Release has archives + `checksums.txt` for all five targets |
| 11 | `vex skills install <git-url> --subdir skills/edit-loop` | Skill installed; appears in `vex skills list` |
| 12 | Open macOS app | Token sourced from keychain; no agent logic in native layer; app signed and notarised |

### Required tests

```rust
// src/config/tests.rs

#[test]
fn config_layered_env_overrides_file() {
    std::env::set_var("VEX_MODEL_NAME", "env-model");
    let cfg = Config::load_layered_from_fixture("model_name = \"file-model\"").unwrap();
    assert_eq!(cfg.model_name, "env-model");
}

#[test]
fn config_file_layer_fills_missing_env() {
    std::env::remove_var("VEX_MODEL_NAME");
    let cfg = Config::load_layered_from_fixture("model_name = \"file-model\"").unwrap();
    assert_eq!(cfg.model_name, "file-model");
}

#[test]
fn config_model_token_rejected_from_file() {
    let result = Config::load_layered_from_fixture("model_token = \"secret\"");
    assert!(result.unwrap_err().to_string().contains("model_token"));
}

#[test]
fn migrate_config_maps_vex_api_protocol_anthropic() {
    let output = migrate_config_from_env(&[("VEX_API_PROTOCOL", "anthropic")]);
    assert!(output.contains("model_protocol = \"messages-v1\""));
}

#[test]
fn migrate_config_maps_structured_tool_protocol_on() {
    let output = migrate_config_from_env(&[("VEX_STRUCTURED_TOOL_PROTOCOL", "on")]);
    assert!(output.contains("tool_call_mode = \"structured\""));
}

#[test]
fn project_instructions_within_budget_injected() {
    let ctx = RuntimeContext::new_with_project_file("# do not use unwrap");
    assert!(ctx.system_prompt().contains("do not use unwrap"));
}

#[test]
fn project_instructions_over_budget_skipped() {
    let huge = "x".repeat(4096 * 5);
    let ctx = RuntimeContext::new_with_project_file(&huge);
    assert!(!ctx.system_prompt().contains(&huge));
}

#[test]
fn mcp_server_config_rejected_in_repo_local() {
    let toml = "[[mcp_servers]]\nname=\"bad\"\ntransport=\"stdio\"\ncommand=\"echo\"\n";
    let result = Config::load_repo_local_from_str(toml);
    assert!(result.unwrap_err().to_string().contains("mcp_servers"));
}

// src/runtime/sandbox/tests.rs

#[tokio::test]
async fn sandbox_passthrough_is_identity() {
    let driver = PassthroughSandbox;
    let req = CommandRequest::new("echo", &["hello"]);
    let wrapped = driver.wrap(req.clone()).unwrap();
    assert_eq!(wrapped.command, req.command);
    assert_eq!(wrapped.args, req.args);
}

#[tokio::test]
async fn sandbox_macos_exec_falls_back_when_unavailable() {
    // Simulate sandbox-exec not present
    let driver = MacosSandboxExec::new_with_probe_override(|| false);
    let req = CommandRequest::new("echo", &["hello"]);
    let result = driver.wrap(req);
    // Must succeed (warn + fallback), not error
    assert!(result.is_ok());
    assert_eq!(result.unwrap().sandbox_kind, SandboxKind::Passthrough);
}

#[tokio::test]
async fn sandbox_macos_exec_aborts_when_required_and_unavailable() {
    let driver = MacosSandboxExec::new_with_probe_override(|| false).require();
    let req = CommandRequest::new("echo", &["hello"]);
    assert!(driver.wrap(req).is_err());
}

#[tokio::test]
async fn batch_mode_exits_zero_on_completion() {
    let result = run_batch_mode("echo hello", 3).await.unwrap();
    assert_eq!(result.status, TaskStatus::Completed);
}

// .agents/skills/tests.rs

#[test]
fn skills_registry_install_and_remove() {
    let mut registry = SkillsRegistry::load_from_fixture("").unwrap();
    registry.install("test-skill", SkillSource::Local(Path::new("fixtures/test-skill"))).unwrap();
    assert!(registry.get("test-skill").is_some());
    registry.remove("test-skill").unwrap();
    assert!(registry.get("test-skill").is_none());
}

#[test]
fn skills_install_rejects_raw_url() {
    let result = SkillsRegistry::validate_source("https://raw.githubusercontent.com/x/y/main/skill/SKILL.md");
    assert!(result.is_err());
}

// src/app/tests.rs

#[test]
fn model_switch_name_only_succeeds() {
    let mut ctx = RuntimeContext::default();
    ctx.set_model_name("model-v2").unwrap();
    assert_eq!(ctx.model_name(), "model-v2");
}

#[test]
fn model_switch_backend_kind_is_error() {
    let mut ctx = RuntimeContext::default();
    assert!(ctx.set_model_backend(ModelBackendKind::ApiServer).is_err());
}
```

---

## Rationale

### Why is sandboxing a separate driver layer rather than part of `CommandRunner`?

`CommandRunner` is responsible for spawning and managing processes. Sandboxing is a containment policy applied to command arguments before dispatch. Keeping `SandboxDriver` as a pre-dispatch wrapper means `CommandRunner` implementations remain transport-pure and independently testable. Capability approval and execution containment are also orthogonal concerns: an operator may want to sandbox all commands regardless of approval state, or run in passthrough mode in a trusted environment. Conflating them creates policy interactions that are hard to reason about.

### Why is `BatchMode` the designated integration point for CLI editor extensions?

An editor extension that embeds the Rust runtime directly (via FFI or a native module) introduces a tight coupling between the extension's release cycle and the runtime's. `vex exec` over a subprocess is loosely coupled: the extension shells out, reads JSONL, and renders it. The runtime can evolve without breaking the extension as long as the JSONL output schema is stable. Any editor can integrate without language-specific bindings.

This applies to file-based and CLI editor surfaces. A native GUI application that requires richer bidirectional communication — streaming partial results, session state queries, live approval prompts — should use the `LocalApiServer` path (Phase I) rather than `vex exec`. The two integration paths are complementary: `BatchMode` for simple, stateless editor surfaces; `LocalApiServer` for full native clients.

### Why is the Windows target `gnu` rather than `msvc`?

`x86_64-pc-windows-gnu` is cross-compilable from Linux via `cross` and the mingw toolchain, requiring no Windows CI runner. `x86_64-pc-windows-msvc` requires a Windows runner and the Visual Studio toolchain. For an initial release, the gnu target provides broad compatibility with no additional CI infrastructure cost. The msvc target may be added in a future ADR if Windows installer tooling specifically requires it.

### Why does the macOS wrapper require code signing?

An unsigned binary distributed as a `.dmg` will be quarantined and blocked by Gatekeeper on every macOS version since 10.15. A user presented with "vexcoder cannot be opened because the developer cannot be verified" will not reach a working installation. Distribution without signing is not a viable path for adoption and must not be treated as an acceptable fallback.

### Why is the skills registry a flat manifest with no dependency resolution?

Skills are workflow documents, not compiled libraries. They have no transitive dependencies, version conflicts, or ABI requirements. A flat manifest with local paths and optional source URLs is sufficient. Adding a semver solver would be solving a problem that does not exist and would make the system significantly harder to audit and maintain.

### Why is `vex migrate config` limited to vexcoder's own legacy variables?

The migration tooling exists to help operators who were running `vexcoder` before ADR-022. Third-party SDK or CLI configurations are the operator's responsibility to translate and are outside the scope of automated migration. Claiming to migrate third-party configurations would require testing against those tools' variable schemas, which introduces a maintenance dependency on external projects.

### Why is code indexing a formal deferral gate rather than simply unscheduled?

Recording a deferral explicitly prevents a dispatcher from treating the absence of an ADR as permission to proceed. The `src/index/` path is named, the rationale for not building it yet is on record, and any future implementation is required to go through a new ADR. Without this gate, the constraint is invisible to automated agents processing the task backlog.

---

## Alternatives considered

### Implement sandbox as a capability-approval outcome rather than a driver layer

Rejected. Approval and containment are orthogonal. A user may want to always sandbox regardless of approval state. Conflating them produces policy interactions where the containment posture is unpredictable from configuration alone.

### Use a package manager for skills distribution

Rejected. Skills are workflow documents. A package manager adds lockfiles, dependency resolution, and version conflicts for a problem that requires none of those mechanisms. A flat manifest is sufficient and significantly easier to audit and reason about.

### Make the macOS wrapper a full native UI replacing the TUI

Rejected. A native UI that replaces the TUI would require duplicating or closely tracking the Rust TUI state in the native layer indefinitely. Any change to the Rust TUI would require a corresponding native change. Wrapping the terminal surface preserves the single canonical implementation and eliminates that maintenance surface.

### Use `x86_64-pc-windows-msvc` as the Windows build target from the start

Rejected for the first release. Requires a Windows CI runner and Visual Studio toolchain setup. The gnu target is cross-compilable from the existing Linux runner with no additional infrastructure. May be revisited in a future ADR.

### Map third-party SDK variable names in `vex migrate config`

Rejected. The migration command exists for operators running vexcoder before ADR-022, not for operators migrating from unrelated tools. Including third-party variable mappings would introduce a maintenance dependency on external projects' naming conventions.

---

## Consequences

**Easier after this ADR:**
- Operators can install `vex` from GitHub Releases without building from source.
- macOS users have a native application wrapper with keychain-backed credential storage.
- Operators migrating from pre-ADR-022 deployments have a single command to produce the correct config fragment.
- CI pipelines can drive the agent headlessly via `vex exec`.
- External tool servers can be integrated without changes to the core binary.
- Mutating commands can be sandboxed independently of the approval layer.
- Skills can be discovered and installed without manual directory copying.

**Harder or more complex:**
- Release workflow must cross-compile for five targets. The gnu Windows target avoids a Windows runner but produces binaries that depend on the mingw runtime.
- macOS wrapper requires a Developer ID certificate and App Store Connect API key stored as CI secrets. Loss of these credentials blocks future signed releases.
- MCP server lifecycle adds async surface area to the session start/stop path. STDIO server crashes must be handled gracefully.
- Sandbox drivers are platform-specific. `MacosSandboxExec` is deprecated upstream; `DockerSandbox` requires Docker. Both absent conditions must be clearly reported at startup.

**Constraints imposed on future work:**
- `VEX_MODEL_TOKEN` must never be read from any config file layer. Files containing `model_token` must be rejected with a diagnostic at load time.
- All new direct dependencies introduced under this ADR must be licensed under MIT, Apache 2.0, or a dual MIT/Apache 2.0 offering. Any deviation requires a separate ADR recording an explicit exception and its legal basis.
- `[[mcp_servers]]` must not be permitted in repo-local config. Reject with a diagnostic.
- `SandboxDriver::wrap` must be called on every `CommandRequest` before it reaches `CommandRunner`. Bypassing it must use `PassthroughSandbox` explicitly. This includes `CommandRequest` instances produced by `ValidationSuite::run` (ADR-023) and by tool dispatch during edit-loop turns — the sandbox boundary applies uniformly to all subprocess execution regardless of the call site.
- `BatchMode` must not import `ratatui` or `crossterm`. The REF-02 CI grep check must stay green.
- The native macOS application layer (Phase H) must not contain agent logic. Any changeset to `packaging/macos/` that also modifies `src/` is out of scope for Phase H and must be rejected. This constraint is Phase H scoped — a future `LocalApiServer: RuntimeMode + FrontendAdapter` implementation will legitimately reside in `src/` and is the intended expansion path for a full native macOS client.
- `src/index/` must not be implemented without a dedicated ADR. Gap 12 is a formal gate.
- Phases G and H must not begin until milestone-1 correctness work is validated end-to-end.
- Runtime code and config must use only neutral, non-branded names. Documentation may reference external tools by name where necessary for operator clarity.

---

## Dispatcher checklist

| ID | Task | Status |
| :--- | :--- | :--- |
| **PA-01** | Layered config resolution chain | [ ] |
| **PA-02** | Project instructions injection | [ ] |
| **PA-03** | `vex migrate config` sub-command | [ ] |
| **PA-04** | `docs/src/migration.md` complete and accurate | [ ] |
| **PB-01** | `vex completions <shell>` | [ ] |
| **PB-02** | `vex install-hooks` / `vex uninstall-hooks` | [ ] |
| **PB-03** | `vex skills list\|install\|remove` + `registry.toml` | [ ] |
| **PC-01** | `/model <name>` runtime model switching | [ ] |
| **PD-01** | `SandboxDriver` trait + `PassthroughSandbox` | [ ] |
| **PD-02** | `MacosSandboxExec` driver (best-effort + require flag) | [ ] |
| **PD-03** | `DockerSandbox` driver | [ ] |
| **PE-01** | `BatchMode: RuntimeMode + FrontendAdapter` | [ ] |
| **PE-02** | `vex exec` sub-command with JSONL/text output | [ ] |
| **PF-01** | `McpRegistry` with STDIO and HTTP transports | [ ] |
| **PF-02** | `Capability::McpTool` and approval wiring | [ ] |
| **PG-01** | GitHub Releases workflow — Linux and macOS targets | [ ] |
| **PG-02** | GitHub Releases workflow — Windows (gnu) target | [ ] |
| **PG-03** | Homebrew tap formula + auto-update dispatch | [ ] |
| **PH-01** | macOS application layer — process management + terminal surface | [ ] |
| **PH-02** | macOS application layer — keychain credential storage + env injection | [ ] |
| **PH-03** | macOS code signing, notarisation, and `.dmg` release attachment | [ ] |
| **PI-01** | `/permissions` — renders active_grants table; no model turn | [ ] |
| **PI-02** | `/allow <cap> [once\|session]` — grants capability; enum-derived names; no persist | [ ] |
| **PI-03** | `/deny <cap>` — removes capability from active_grants | [ ] |
| **PI-04** | `/new` — saves current TaskState, resets session, new TaskId | [ ] |
| **PI-05** | `/resume [<task-id>]` — loads TaskState; grants restored; conversation not restored | [ ] |
| **PI-06** | `/mcp list` — renders loaded servers and tool counts from McpRegistry | [ ] |
| **PI-07** | `/mcp show <server>` — renders full-namespace tool names for named server | [ ] |
| **PI-08** | `/plan` and `/context` — see ADR-023 EL-11/EL-12 (tracked there; listed here for cross-ref) | [ ] |

## Dispatcher reporting contract (mandatory per checklist item)

When checking a box above, append an evidence block under this section:

```markdown
### [PA-01 … PI-08] - <short title>
- Dispatcher: <name/id>
- Commit: <sha>
- Files changed:
  - `path/to/file` (+<insertions> -<deletions>)
- Validation:
  - `cargo test --all-targets` : pass/fail
  - `check_no_alternate_routing.sh` : pass/fail
  - `check_forbidden_imports.sh` : pass/fail
- Notes:
  - <what was built and why>
```

---

## Compliance notes for agents

| Rule | Enforcement |
| :--- | :--- |
| Do not read `VEX_MODEL_TOKEN` from any config file | Reject files containing `model_token` with a diagnostic at load time |
| Do not permit `[[mcp_servers]]` in repo-local `.vex/config.toml` | Reject with a diagnostic at load time |
| Do not bypass `SandboxDriver::wrap` | All `CommandRequest` instances must pass through the active driver before reaching `CommandRunner` |
| Do not import `ratatui` or `crossterm` in `BatchMode` source files | REF-02 CI grep check must stay green |
| Do not conflate `SandboxDriver` (containment) with `ApprovalPolicy` (capability gating) | Both layers remain active and independent in the dispatch path |
| Do not allow `/model` to change `ModelBackendKind` or `ModelProtocol` | Name-only switching; reject backend/protocol changes with a clear error |
| Do not inject project instructions exceeding token budget | Emit warning and skip the file; do not truncate |
| Do not auto-install git hooks | `vex install-hooks` must be an explicit operator action |
| Do not add agent logic, model calls, or conversation state to `packaging/macos/` | Phase H constraint: packaging and credential layer only. A future `LocalApiServer: RuntimeMode + FrontendAdapter` in `src/` is the correct expansion path for a full native client — it is not a violation of this rule |
| Do not implement `src/index/` without a dedicated ADR | Gap 12 is a formal deferral gate |
| Do not implement conversation compaction, turn pruning, or `ConversationCheckpoint` summarisation without a dedicated ADR | Formal deferral gate; `/compact` and richer `/usage` are part of this gate |
| Do not implement `/undo` without a dedicated ADR specifying rollback strategy | Gap 14 formal deferral gate |
| `/allow` and `/deny` must derive capability names from the `Capability` enum at compile time | No hardcoded string list permitted; drift between enum and command surface must be a compile error |
| `/allow session` must not call `TaskState::save` | Session grants are in-memory only; persistence belongs to config layering (Gap 3) |
| `/mcp add` and `/mcp remove` must not be implemented under this ADR | Runtime MCP lifecycle management requires a dedicated ADR |
| `/new` must call `TaskState::save` before resetting; abort if save fails | Data loss prevention — never discard a live task state without a successful save |
| Do not begin Phases G or H before milestone-1 correctness work is validated | Sequencing guard |
| Do not add a dependency licensed under a commercial, copyleft, or conditionally-paid license | All direct dependencies must carry MIT, Apache 2.0, or dual MIT/Apache 2.0 licensing; exceptions require a dedicated ADR with explicit legal basis |
| Do not use provider-branded names or proprietary product references in runtime code, config keys, or default values | Documentation may reference external tools by name for operator clarity; runtime behaviour must remain neutral |

---

## Appendix — ADR-022 Amendment (2026-03-03)

The amendment that authorises native packaging and editor-surface work is recorded in `TASKS/ADR-022-amendment-2026-03-03.md` and must be applied to `TASKS/ADR-022-free-open-coding-agent-roadmap.md` before Phases G–H work begins. See that file for exact application instructions.

---

## References

- `TASKS/ADR-022-free-open-coding-agent-roadmap.md` — zero-licensing-cost coding agent roadmap (permissive-dependency constraint, self-hostable posture)
- `TASKS/ADR-022-amendment-2026-03-03.md` — terminal-first constraint scoped to milestone 1
- `TASKS/ADR-023-deterministic-edit-loop.md` — edit loop, context assembly, model profiles, semantic commands
- `TASKS/completed/ADR-014-runtime-core-policy-dedup-and-enforcement.md` — policy separation
- `TASKS/completed/ADR-006-runtime-mode-contracts.md` — runtime mode contracts
- `docs/src/migration.md` — canonical variable rename table and migration guide
- `.agents/skills/registry.toml` — skills registry manifest