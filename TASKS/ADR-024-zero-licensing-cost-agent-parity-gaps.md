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

Every direct dependency of `vexcoder` must be licensed under a permissive, royalty-free license — specifically MIT, Apache 2.0, or a dual MIT/Apache 2.0 offering — such that building, distributing, and operating the application imposes no licensing fee, royalty obligation, or copyright assignment requirement on any party. This is the operative reason the project uses Rust (MIT/Apache 2.0) and ratatui (MIT): neither the language toolchain, the TUI framework, nor any crate in the dependency graph charges a licensing fee or restricts redistribution. Crates introduced directly by this ADR: `clap_complete` (Gap 6, MIT/Apache 2.0). All satisfy the constraint. The same constraint applies to all future Rust crate dependencies added under this ADR. Any crate carrying a commercial license, a copyleft license that would require source disclosure of this codebase, or a license that conditions use on a paid tier is prohibited without a dedicated ADR recording an explicit exception and its legal basis.

**Operational and runtime dependency scope:** This ADR also introduces optional operational dependencies — Docker (Apache 2.0, used by `DockerSandbox`), npm-distributed MCP server packages (licenses vary per package), Homebrew (BSD 2-Clause), and GitHub Actions CI tooling (license varies per action). These are not Rust crate dependencies compiled into the binary; they are operator-provided runtime components or CI infrastructure. The licensing constraint for these is therefore different: they are not required for the binary to build or run in `PassthroughSandbox` mode, and operators who use them accept their respective license terms independently. However, for long-term multi-year legal clarity the following rules apply:

- **Docker (`DockerSandbox`):** Docker Engine is Apache 2.0 for the community edition. Docker Desktop has a separate commercial license that applies to certain business uses. The ADR does not bundle Docker; operators install it independently. Documentation must note that operators using Docker Desktop in a commercial context must verify their Docker Desktop licensing.
- **MCP server packages:** The `[[mcp_servers]]` config allows operators to configure arbitrary npm packages as tool servers. `vexcoder` makes no representation about the licenses of third-party MCP packages. Documentation must note that operators are responsible for verifying the license of any MCP server package they configure.
- **CI tooling (GitHub Actions, `cross`, mingw toolchain):** These are build and release infrastructure, not runtime components. Their licensing does not affect the distributed binary's license obligations. **mingw runtime library exception:** the mingw runtime libraries (`libgcc`, `libwinpthread`) are distributed under the GCC Runtime Library Exception, which explicitly permits static linking into permissively-licensed binaries without copyleft propagation. No licensing obligation is imposed on the distributed `vex` binary by the mingw toolchain.
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
| 16 | No user persistent notes (`/memory`) | Proposed |
| 17 | No project bootstrapping sub-command (`vex init`) | Proposed |
| 18 | No graceful exit command or session metadata display | Proposed |
| 19 | No `@<path>` inline file injection | Proposed |
| 20 | No `!<command>` inline shell passthrough | Proposed |
| 21 | No user-defined slash commands | Proposed |
| 22 | No `/tools` active tool enumeration | Proposed |
| 23 | No `/diff` zero-turn working-tree diff display | Proposed |
| 24 | No git workflow integration beyond commit attribution | Proposed |
| 25 | No test generation semantic command (`/generate-tests`) | Proposed |
| 26 | No pre/post-tool-call hooks system | Proposed |
| 27 | No environment health check sub-command (`vex doctor`) | Proposed |
| 28 | No session-level token counter | Proposed |
| 29 | No conversation and task export sub-command (`vex export`) | Proposed |
| 30 | No `--resume` CLI startup flag | Proposed |
| 31 | No MCP HTTP server authentication headers | Proposed (extends Gap 5) |
| 32 | No `-p`/`--print` one-shot plain-text flag | Proposed |
| 35 | No model-callable workspace exploration tools (`search_files`, `list_dir`, `glob_files`) | Proposed |

### Gaps intentionally deferred by this ADR

| Gap | Rationale |
| :--- | :--- |
| Image/screenshot input | Deferred until the model backend seam (ADR-022 Phase 1) is stable and a multimodal local runtime target exists |
| Multi-agent / parallel task execution | Out of scope for the first milestone per ADR-022 Decision item 5 (single active task) |
| Cloud task delegation | Deferred indefinitely; contradicts the self-hostable, zero-licensing-cost posture established by the dependency licensing constraint above |
| Inline code completion (LSP/language-server) | Fundamentally different runtime category from a turn-based agent. Requires a persistent language-server process, real-time keystroke handling, and IDE surface integration — none of which are compatible with the terminal-first, turn-based interaction model. Deferred indefinitely. |
| Enterprise governance (audit logs, seat management, org policy) | Single-user, self-hosted by design. Multi-tenant governance infrastructure contradicts the zero-licensing-cost constraint and the self-hostable posture. Deferred indefinitely. |
| Voice input | Requires audio I/O subsystem incompatible with terminal-first constraint. Deferred indefinitely. |
| Platform API integration (GitHub PR creation via REST API) | `vex pr-summary` (Gap 24) produces text for pipe to the operator's platform CLI; direct REST API calls require credential management for each platform and a dedicated ADR. Deferred indefinitely. |
| Built-in web search | Depends on MCP (Gap 5). Implementing web search before MCP exists would permanently couple it to the core runtime |
| IDE extensions | Deferred to a post-first-milestone ADR per ADR-022 amendment Decision item 11. File-based editor extensions must use `vex exec` (Gap 2). Native GUI surfaces (IDE panels with live streaming, macOS native client) must use the `LocalApiServer` path reserved in Phase I |
| Conversation compaction / context-window management | Long-running sessions that approach the model's context limit have no managed strategy for pruning or summarising old turns. `ConversationCheckpoint` in `TaskState` records a `message_count` and `summary` string but neither is populated nor acted upon by the runtime today. Implementing compaction requires a dedicated ADR: the summarisation prompt, the trigger threshold, and whether the summary is injected as a system message or a synthetic turn all affect model behaviour and must be decided deliberately. Deferred until the edit loop and BatchMode are stable — compaction adds the most value for long `vex exec` runs, and those require BatchMode to exist first. **Command-surface note:** reference CLIs expose active context management commands (`/compact`, `/usage`). ADR-023 `EL-12` introduces `/context` for read-only token-estimate display. `/compact` (trigger summarisation) and a richer `/usage` (per-tool token attribution) are part of this deferred gap and must not be implemented without the dedicated compaction ADR. This gap is a formal deferral gate: do not implement conversation pruning or summarisation without a dedicated ADR. A per-session turn-token counter (Gap 28) is separable from this gate: it reads token counts reported by the API response and requires no summarisation strategy. Gap 28 must not be blocked by this deferral. |

---

## Sequencing guard

**Phases G and H (distribution and macOS packaging) are post-first-milestone** and must not block milestone-1 correctness work (ADR-022 phases 1–8 and ADR-023 edit loop). They may not begin until the edit loop, approval system, and task state persistence are validated end-to-end. Any dispatcher that begins Phase G or H work before those milestones are green must be considered out of scope.

**Phase I (local API server) is post-Phase-H** and requires a dedicated ADR specifying wire protocol, local socket authentication, and streaming response format before any dispatcher begins work.

---

## Decision

This ADR locks decisions for gaps 1–11, gaps 13–32, and gap 35. Gap 12 is formally deferred with rationale recorded.

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

Introduce `McpRegistry` loaded from the **user config file only** (`~/.config/vex/config.toml`) under a `[[mcp_servers]]` table. STDIO servers are launched as managed processes at session start and terminated at session end. HTTP servers are connected by URL, with optional authentication headers (see Gap 31). Tools advertised by MCP servers are merged into the tool dispatch table with `mcp.<server_name>.<tool_name>` namespace prefixing to prevent collisions with built-in tools. A new `Capability::McpTool` variant is added with a default approval scope of `once`.

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

The example package `@modelcontextprotocol/server-filesystem` is MIT-licensed. This is noted for documentation completeness only; the operator licensing obligation in the dependency constraint section above applies to all configured MCP packages regardless.

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

**Windows target note:** `x86_64-pc-windows-msvc` requires a Windows CI runner and the MSVC toolchain. `x86_64-pc-windows-gnu` (mingw) is cross-compilable from Linux via `cross` with no Windows runner required. Use `gnu` as the default Windows target. A future ADR may add an `msvc` build on a Windows runner if installer tooling requires it. See the dependency licensing constraint section for the mingw runtime library exception applicable to static builds.

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

**OS-vendor API licensing note (Phase H):** `Security.framework` (keychain access) and `xcrun notarytool` (notarisation) are Apple proprietary APIs available under Apple's macOS SDK terms. Their use in the Phase H packaging layer imposes no additional licensing obligation on the Rust binary itself — the binary's MIT license is unaffected. Phase H is macOS-exclusive by design; the Apple SDK terms are accepted by operators at OS installation time, not imposed by `vexcoder`'s distribution.

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

**`/clear` — clear conversation history without changing task:**

```
/clear
    Clears the conversation history in RuntimeContext without changing TaskState,
    TaskId, or active_grants. The task remains active; only the message window
    is reset. Emits "[cleared: conversation history reset; task <task-id> continues]".
    No model turn.
    Use: conversation window is growing large and the operator wants a fresh
    exchange within the same task without discarding grants or file-change
    context. Distinct from /new, which saves the task and creates a fresh TaskId.
    active_edit_loop on TuiMode must be cleared; a running loop cannot continue
    after the conversation history it was operating on is discarded.
```

**`/fork` — branch current session to a new task-id:**

```
/fork [<label>]
    Saves the current TaskState to VEX_STATE_DIR under its existing TaskId
    (preserving the parent). Creates a new TaskState with a fresh TaskId
    (format: "task-<utc-ms>-fork" or "task-<utc-ms>-<label>" if <label> given)
    that copies active_grants, changed_files, and TaskStatus from the parent.
    The current session continues on the fork; the parent is preserved and
    resumable via /resume. Emits "[fork: <new-task-id> branched from
    <parent-task-id>]". No model turn.
    Conversation history is NOT copied to the fork — the fork begins with an
    empty conversation window and the inherited grants and file-change context.
    If TaskState::save fails for the parent, the fork is aborted with an error;
    the session continues unchanged.
```

**Constraints:**

- `/new` must call `TaskState::save` before resetting. A new session must not begin if the save fails; emit the error and abort.
- `/resume` must not attempt to restore conversation history. `ConversationCheckpoint.message_count` may be displayed informationally; the content is not stored.
- `/resume` without argument must not start a model turn. The selection overlay must use the existing `PendingApproval` input path.
- Both commands must clear `active_edit_loop` on `TuiMode` to prevent stale loop state. `[source: task_state.rs — TaskState::state_dir() for VEX_STATE_DIR resolution]`
- `/clear` must clear `active_edit_loop` on `TuiMode`. A running edit loop cannot continue after its conversation history is discarded.
- `/fork` must call `TaskState::save` for the parent before creating the fork. Fork must be aborted if the save fails.
- `/fork` must not copy conversation history to the fork. The fork begins with an empty conversation window.

**Anchor tests:** `test_tui_new_saves_current_state_before_reset`; `test_tui_new_creates_fresh_task_id`; `test_tui_resume_restores_active_grants`; `test_tui_resume_does_not_restore_conversation`; `test_tui_resume_unknown_id_emits_error`; `test_tui_new_clears_active_edit_loop`; `test_tui_clear_resets_conversation_history`; `test_tui_clear_preserves_task_id_and_grants`; `test_tui_clear_clears_active_edit_loop`; `test_tui_fork_saves_parent_before_branching`; `test_tui_fork_creates_new_task_id`; `test_tui_fork_does_not_copy_conversation`; `test_tui_fork_aborts_on_save_failure`.

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

### Gap 16 — User Persistent Notes (`/memory`)

Reference agents expose a user-level notes surface that persists across sessions and is injected into every conversation. This is distinct from project instructions (Gap 4 / `AGENTS.md`): project instructions are project-scoped and committed to the repo; user notes are operator-scoped, stored in the user config layer, and never committed.

**Storage:** `~/.config/vex/memory.md` (XDG path) or `~/.vex/memory.md` as fallback. Plain UTF-8 Markdown. The file is created on first `/memory add` if it does not exist.

**Session injection:** At session start, after project instructions are loaded, the notes file is read and appended to the system prompt using the same labelled-delimiter pattern as Gap 4. Token budget: `VEX_MAX_MEMORY_TOKENS` (default: 2 048). If the notes file exceeds the budget, it is not injected and a startup warning is emitted. The budget is checked independently of `VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS`; both may be active simultaneously and their token counts do not sum toward a shared limit.

**Commands added to `try_handle_slash_command`:**

```
/memory
    Renders the current contents of the notes file to transcript via
    push_history_line. No model turn.
    If the notes file does not exist or is empty: "[memory] no notes".

/memory add <note>
    Appends <note> as a new line to the notes file.
    Creates the file if it does not exist.
    Emits "[memory: note added]" on success.
    No model turn.

/memory clear
    Clears all notes from the file after an in-TUI confirmation prompt
    (rendered via the existing overlay input path: "clear all notes? [y/N]").
    Emits "[memory: cleared]" on confirmation; "[memory: cancelled]" otherwise.
    No model turn.
```

**Constraints:**

- `/memory` commands must never start a model turn. All output is via `push_history_line`.
- The notes file path is resolved from the user config layer (priority 3 in the Gap 3 layered chain). It must not be overrideable via repo-local config — notes are operator-personal and must not be settable per-project.
- The notes file is never committed to source control. `vex init` (Gap 17) must write `~/.config/vex/memory.md` to a global `.gitignore_global` recommendation in `docs/src/migration.md`, not to the repo `.gitignore`.
- `/memory clear` requires the confirmation overlay. Non-interactive (`BatchMode`) invocation must treat `/memory clear` as an error unless `--auto-approve` is passed.
- Token budget overflow is a warning, not an error. A session without notes injection is still a valid session.

**Gating:** Gap 16 depends on Gap 3 (layered config) for the notes file path resolution. PJ-01 must not begin until PA-01 (layered config) is green.

**Anchor tests:** `test_tui_memory_renders_empty_notes`; `test_tui_memory_add_appends_to_file`; `test_tui_memory_clear_requires_confirmation`; `test_tui_memory_clear_cancellable`; `test_tui_memory_does_not_call_start_turn`; `test_memory_injection_within_budget`; `test_memory_injection_over_budget_emits_warning`.

---

### Gap 17 — Project Bootstrapping (`vex init`)

Operators starting a new project must currently create the `.vex/` directory and config files manually. `vex init` is a one-shot CLI sub-command (not a TUI slash command) that scaffolds the standard project structure.

```bash
vex init [--dir <path>]
```

**Actions (non-destructive — skips files that already exist):**

1. Create `.vex/` directory in the current working directory (or `--dir`).
2. Write `.vex/config.toml` with all keys present but commented out, matching the canonical key names from the ADR-024 normative additions table.
3. Write `AGENTS.md` at the repo root with a minimal template instructing the operator to fill in project-specific guidance.
4. Write `.vex/validate.toml` with an empty `[[commands]]` table and comments explaining the format.
5. Print a summary of created and skipped files to stdout.

**It must not:**

- Start the agent loop.
- Overwrite existing files.
- Modify `~/.config/vex/` or any user-layer config path.
- Require network access.

**Relationship to `vex install-hooks` (Gap 7):** `vex init` does not install git hooks. The operator runs `vex install-hooks` separately after `vex init`. This preserves the opt-in nature of hook installation.

**Constraints:**

- `vex init` must exit 0 on success and on the "file already exists, skipping" case.
- `vex init` must exit non-zero if the target directory is not writeable.
- The generated `.vex/config.toml` must be kept in sync with the ADR-024 normative TOML table. Any new config key added to this ADR must also appear commented-out in the generated template; this is enforced by a test that parses the generated file and compares keys to the normative list.

**Anchor tests:** `test_vex_init_creates_vex_dir`; `test_vex_init_writes_config_toml_skeleton`; `test_vex_init_writes_agents_md_template`; `test_vex_init_skips_existing_files`; `test_vex_init_config_keys_match_normative_list`; `test_vex_init_does_not_start_agent_loop`.

---

### Gap 18 — Graceful Exit and Session Metadata Display (`/quit`, `/exit`, `/about`)

**`/quit` and `/exit`:** Both reference CLIs expose an explicit exit command. Currently the only TUI exit path is Ctrl+C or Ctrl+D; nothing in the command directory tells an operator how to exit. Both `/quit` and `/exit` must be registered in the dispatch table and therefore appear in `/commands` output. They must trigger a clean shutdown: save any live `TaskState`, flush the TUI, and exit with code 0. A running `EditLoop` must be cancelled via its `CancellationToken` before shutdown proceeds.

**`/about`:** Zero-turn display of build metadata to transcript:

```
/about
    Renders session metadata to transcript via push_history_line. No model turn.
    Output format (normative):
      [about]
        version   : <cargo package version from env!("CARGO_PKG_VERSION")>
        build     : <BUILD_DATE env var or "unknown">
        commit    : <GIT_COMMIT_SHORT env var or "unknown">
        model     : <active model name>
        backend   : <ModelBackendKind>
        sandbox   : <SandboxKind>
```

`BUILD_DATE` and `GIT_COMMIT_SHORT` are injected at compile time via `build.rs` using `env!()` macros. Neither is required to be present; "unknown" is the fallback.

**Constraints:** `/quit` and `/exit` must call `TaskState::save` before exiting; if save fails, emit the error and prompt the operator to confirm exit without save. A running `EditLoop` must be cancelled before shutdown — never force-exit while `active_edit_loop` is `Some`.

**Anchor tests:** `test_tui_quit_saves_task_state`; `test_tui_quit_cancels_active_edit_loop`; `test_tui_about_renders_without_model_turn`; `test_tui_exit_is_alias_for_quit`.

---

### Gap 19 — `@<path>` Inline File Injection

Operators frequently need to include a specific file's content in a free-form turn without using a slash command. The `@<path>` prefix is a pre-send prompt transformation: before the turn reaches `ctx.start_turn`, any `@<path>` tokens in the input are resolved and replaced with the file content as an inline fenced block.

```
@src/foo.rs what does the parse_header function do?
```

is transformed to:

```
[file: src/foo.rs]
<content of src/foo.rs>

what does the parse_header function do?
```

**Resolution rules:**
- Path resolution uses `ToolOperator`'s workspace-root confinement guards. Any `@<path>` that resolves outside the workspace root is rejected with an inline error annotation and the turn proceeds without that substitution.
- If the file does not exist: annotate `[file: <path> — not found]` inline; do not abort the turn.
- If the file exceeds `max_file_bytes` (same limit as `ContextAssembler`): annotate `[file: <path> — truncated at <n> bytes]`.
- Multiple `@<path>` tokens in a single input are all resolved in order.
- `@<dir>` resolves to a compact file listing (paths only, no content) bounded to `max_related` entries.
- `@` expansion is applied in `TuiMode::on_user_input` before the slash-command check and before `ctx.start_turn`. It must not be applied inside slash-command arguments — `/explain @src/foo.rs` already assembles context via `ContextAssembler`.

**Anchor tests:** `test_at_prefix_injects_file_content`; `test_at_prefix_outside_workspace_is_rejected`; `test_at_prefix_missing_file_annotates_inline`; `test_at_prefix_multiple_tokens_resolved_in_order`; `test_at_prefix_not_expanded_inside_slash_command_args`.

---

### Gap 20 — `!<command>` Inline Shell Passthrough

The `!` prefix runs a shell command from the TUI input and renders its output to the transcript. Distinct from `/run` (which invokes `ValidationSuite` and is model-adjacent) and `/test` (which runs the full inferred suite). `!<cmd>` is a zero-model-turn, operator-driven shell call for quick inspection.

```
!git log --oneline -10
!cat src/foo.rs | grep "fn "
```

**Behaviour:**
- The command is passed to `CommandRunner::run_one_shot` via `SandboxDriver::wrap` — the same approval and containment path as all other subprocess calls. `Capability::RunCommand` approval is required; if not in `active_grants`, the standard approval overlay is shown.
- Output (stdout + stderr, capped at `VALIDATION_TAIL_BYTES`) is rendered to transcript via `push_history_line`. Exit code is shown: `[exit: <n>]`.
- No model turn is started. Output is not automatically injected into the next turn's context. If the operator wants the output as model context, they can reference it in their next free-form prompt.
- `!` expansion is applied in `TuiMode::on_user_input` before the slash-command check, after `@` expansion.

**Constraints:** `!<cmd>` must route through `SandboxDriver::wrap` and `ApprovalPolicy`. It must never bypass the approval gate. It must never start a model turn.

**Anchor tests:** `test_bang_prefix_routes_through_sandbox`; `test_bang_prefix_requires_run_command_approval`; `test_bang_prefix_renders_output_to_transcript`; `test_bang_prefix_does_not_start_model_turn`.

---

### Gap 21 — User-Defined Slash Commands

Operators need reusable prompt templates that invoke the agent with consistent instructions without modifying runtime code. User-defined commands are TOML files stored in `~/.config/vex/commands/` (user-scoped) or `.vex/commands/` (project-scoped). Project-scoped commands take precedence over user-scoped commands of the same name.

```toml
# .vex/commands/standup.toml
name        = "standup"
description = "Summarise changed files as a standup update"
template    = "Summarise the changes in {{context}} as a concise standup update. List each changed file and one sentence of what changed."
```

**Invocation:** `/standup` — resolved and rendered via `edit_template.txt`'s `{{instruction}}` site with `ContextAssembler` providing `{{context}}`. Starts a single `ctx.start_turn`; does not invoke `EditLoop`.

**Rules:**
- Built-in commands (all ADR-023 and ADR-024 slash commands) take precedence. A user-defined command may not shadow a built-in name.
- The `name` field must match `[a-z0-9-]+`. Names beginning with `vex-` are reserved for future built-ins.
- The `template` field supports `{{context}}` and `{{input}}` substitution sites. `{{context}}` is populated by `ContextAssembler`; `{{input}}` is the remainder of the operator's command invocation (`/standup last week` → `{{input}}` = `"last week"`).
- User-defined commands are loaded at session start and appear in `/commands` output in a separate `[custom commands]` section.
- Project-scoped command files in `.vex/commands/` must not be considered user-config-only; they are intentionally project-committed. This is an exception to the general principle that operator-personal config lives in the user layer. The rationale: shared team workflows are a legitimate use case for project-committed commands.

**Anchor tests:** `test_custom_command_invokes_single_turn`; `test_custom_command_cannot_shadow_builtin`; `test_custom_command_input_substitution`; `test_custom_command_context_substitution`; `test_custom_command_appears_in_commands_list`; `test_custom_command_project_scoped_takes_precedence`.

---

### Gap 22 — `/tools` Active Tool Enumeration

Operators need to inspect what tools the agent can invoke in the current session, especially after MCP servers load. `/tools` is a zero-turn command that renders all registered tools to transcript.

```
/tools [desc]
    Renders all registered tools from the live dispatch table.
    No model turn. Output format:
      [tools]
        read_file
        write_file
        apply_patch
        run_command
        mcp.my-server.read_file
        mcp.my-server.write_file
    /tools desc — includes one-line description per tool from the tool schema.
    If McpRegistry is not yet loaded: "[tools] MCP registry not yet available; built-in tools only".
```

**Constraints:** `/tools` and `/tools desc` must never start a model turn. Tool list must be read from the live dispatch table, not a hardcoded list. MCP-namespaced tools use the same `mcp.<server>.<tool>` format as `/mcp show`.

**Anchor tests:** `test_tui_tools_renders_builtin_tools`; `test_tui_tools_includes_mcp_tools`; `test_tui_tools_desc_includes_descriptions`; `test_tui_tools_does_not_start_model_turn`.

---

### Gap 23 — `/diff` Zero-Turn Working-Tree Diff Display

`/context` surfaces a one-line git status summary. Operators frequently need to see the full working-tree diff without starting a model turn or using `/review` (which costs a model turn). `/diff` is the zero-turn complement to `/review`.

```
/diff [--staged]
    Runs `git diff HEAD` (or `git diff --cached` with --staged) via the same
    spawn_blocking + timeout pattern as ContextAssembler. Renders output to
    transcript via push_history_line. No model turn.
    If not a git repo: "[diff] not a git repository".
    If diff is empty: "[diff] working tree is clean".
    If diff exceeds max_diff_lines (ContextAssembler default: 200 lines):
    renders the first max_diff_lines lines with a "[diff truncated — showing
    first <n> lines]" annotation.
```

**Constraints:** `/diff` must use the same `spawn_blocking` + `tokio::time::timeout` + `child.kill()` pattern specified for `ContextAssembler` git calls. It must not start a model turn. Output must never be silently truncated.

**Anchor tests:** `test_tui_diff_renders_working_tree_diff`; `test_tui_diff_staged_flag`; `test_tui_diff_non_git_repo`; `test_tui_diff_clean_working_tree`; `test_tui_diff_truncates_at_max_lines`; `test_tui_diff_does_not_start_model_turn`.

---

### Gap 24 — Git Workflow Integration Beyond Commit Attribution

Gap 7 adds `vex install-hooks` for commit trailers. Reference CLI agents additionally support higher-level git workflow operations as CLI sub-commands: branch creation, PR preparation, and integration with the system `git` binary for common pre-commit/pre-PR workflows.

```bash
vex branch <name>
    Creates a new git branch from HEAD. Equivalent to `git checkout -b <name>`.
    Records the branch name in the active TaskState.
    Does not start the agent loop.

vex pr-summary
    Assembles a diff from the current branch vs the merge-base of the default
    branch (detected from `git symbolic-ref refs/remotes/origin/HEAD`) and
    starts a single model turn using a pr_summary_template.txt prompt to
    generate a PR title and description. Output is rendered to stdout (not
    TUI transcript) for easy pipe to `gh pr create --body-file -` or similar.
    This is a CLI sub-command, not a TUI slash command.
```

**Scope boundary:** These are thin wrappers over `git` binary calls and `ContextAssembler`-style diff assembly. They do not integrate with any specific hosting platform API (GitHub, GitLab, Gitea). Platform API integration (creating PRs via REST API) is explicitly out of scope and requires a dedicated ADR. `vex pr-summary` produces a text artifact the operator pipes to whatever CLI tool manages their remote.

**New prompt template:** `src/prompts/pr_summary_template.txt` — added to EL-06 scope.

**Anchor tests:** `test_vex_branch_creates_git_branch`; `test_vex_branch_records_in_task_state`; `test_vex_pr_summary_assembles_merge_base_diff`; `test_vex_pr_summary_outputs_to_stdout`; `test_vex_pr_summary_does_not_start_tui`.

---

### Gap 25 — Test Generation Semantic Command (`/generate-tests`)

`ValidationSuite` (ADR-023) runs existing tests. Reference CLI agents additionally support generating new tests for existing code as a distinct semantic workflow. This is different from `/edit` (general code editing) because it uses a specialised prompt template that instructs the model to output test code matching the project's test framework, with no patch for non-test files.

```
/generate-tests [path] [--framework <name>]
    Assembles context for the named path (or most recently accessed file)
    using ContextAssembler. Starts a single ctx.start_turn using
    generate_tests_template.txt. EditLoop is not invoked. Any PendingPatch
    targeting non-test files is silently dropped — /generate-tests must only
    apply patches to files matching test naming conventions
    (*_test.rs, *.test.ts, test_*.py, *_spec.rb, etc.) or directories
    named test/ or tests/. Patches to other paths require explicit /edit.
    --framework <name>: injects the framework name into the template
    (e.g. "jest", "pytest", "cargo-test"). Defaults to inferred framework
    from ValidationSuite::infer_from_repo.
```

**New prompt template:** `src/prompts/generate_tests_template.txt` — added to EL-06 scope.

**Constraints:** `/generate-tests` must never apply patches to non-test files. The test-file path filter must be applied before the `PendingApproval` gate, not after — the operator should never be asked to approve a patch to a source file from this command.

**Anchor tests:** `test_tui_generate_tests_assembles_context`; `test_tui_generate_tests_drops_non_test_patches`; `test_tui_generate_tests_infers_framework`; `test_tui_generate_tests_framework_flag_overrides_inference`.

---

### Gap 26 — Pre/Post-Tool-Call Hooks

Reference implementations (Codex, Claude Code) support a `hooks` configuration table that fires operator-defined shell commands before or after specific tool calls — for example, running a formatter after every `write_file`, or a linter before any `apply_patch`. Gap 7 adds git commit hooks only; a general-purpose hooks system addressing the same feature present in reference CLIs is a separate and higher-value capability.

**Configuration** (user config only — `~/.config/vex/config.toml`; repo-local hooks carry the same supply-chain risk as `[[mcp_servers]]` and are rejected at config load time):

```toml
# ~/.config/vex/config.toml — user config layer only

[[hooks]]
event   = "post_tool"
tool    = "apply_patch"
command = "cargo"
args    = ["fmt"]
on_fail = "warn"   # "warn" | "abort" | "ignore"

[[hooks]]
event   = "post_tool"
tool    = "write_file"
command = "prettier"
args    = ["--write", "{{path}}"]
on_fail = "warn"
```

**Events:** `pre_tool` (fires before `SandboxDriver::wrap`) and `post_tool` (fires after the tool result is recorded in evidence). The `tool` field matches tool names as they appear in the dispatch table: built-in names (e.g. `apply_patch`, `write_file`) or MCP-namespaced names (e.g. `mcp.my-server.write_file`).

**Execution:** Hook commands run via `CommandRunner::run_one_shot` wrapped in `SandboxDriver::wrap`. `Capability::RunCommand` approval is required; a hook without approval in `active_grants` is skipped with a warning and the turn continues — hooks must never silently block a turn. `on_fail = "abort"` aborts the pending tool result and surfaces the error to the operator; the agent turn is interrupted at that point but the process continues running. `on_fail = "warn"` records the error to transcript and continues. `on_fail = "ignore"` suppresses the error entirely.

**Template substitution:** `{{path}}` in `args` is substituted with the primary file path of the tool invocation where available. `{{tool}}` is substituted with the tool name. No other substitution sites are supported in this ADR.

**Gating:** Gap 26 depends on Gap 3 (layered config) for the `[[hooks]]` table resolution. PL-01 must not begin until PA-01 (layered config) is green.

**Anchor tests:** `test_hook_post_apply_patch_runs_command`; `test_hook_pre_tool_runs_before_dispatch`; `test_hook_on_fail_abort_interrupts_turn`; `test_hook_on_fail_warn_continues`; `test_hook_requires_run_command_approval`; `test_hook_skipped_without_approval_emits_warning`; `test_hook_repo_local_config_rejected_at_load`.

---

### Gap 27 — Environment Health Check (`vex doctor`)

Operators starting a new deployment need a way to verify that their environment is configured correctly before running a task. `vex doctor` is a read-only CLI sub-command (not a TUI slash command) that probes each runtime dependency in sequence and reports a pass/warn/fail result per check. This is the free/open equivalent of the environment verification present in reference CLI agents.

```bash
vex doctor [--json]
```

**Checks performed (in order):**

| Check | Pass condition | Fail behaviour |
| :--- | :--- | :--- |
| Config load | Layered config resolves without error | Fail with file path and key reference |
| `VEX_MODEL_URL` set | Non-empty, well-formed URL | Fail |
| Model endpoint reachable | HTTP GET to `<VEX_MODEL_URL>` returns any response within 5 s | Warn (endpoint may require auth) |
| `VEX_MODEL_TOKEN` present | Non-empty (value not inspected) | Warn for non-local endpoints only |
| Sandbox probe | `SandboxDriver::probe()` returns ok | Warn with fallback note if `sandbox_require = false`; Fail if `sandbox_require = true` |
| MCP server connectivity | STDIO: binary resolvable on PATH; HTTP: URL reachable within 5 s | Warn per failing server; does not start servers |
| State directory writable | `VEX_STATE_DIR` (or default `.vex/state`) exists and is writable | Warn |
| Policy file parseable | `.vex/policy.toml` parses without error if present | Fail |

Output is rendered to stdout as a human-readable list. `--json` emits a JSON array of `{"check": "...", "status": "pass|warn|fail", "message": "..."}` objects for machine consumption and CI integration. Exit code: 0 if all checks pass or warn; non-zero if any check fails.

`vex doctor` must not start the agent loop, modify any state, or write any files. It is safe to run before `vex init` has been completed.

**Anchor tests:** `test_vex_doctor_passes_with_valid_config`; `test_vex_doctor_fails_on_missing_model_url`; `test_vex_doctor_warns_on_unreachable_endpoint`; `test_vex_doctor_json_output_structure`; `test_vex_doctor_does_not_start_agent_loop`; `test_vex_doctor_sandbox_probe_warns_on_fallback`.

---

### Gap 28 — Session-Level Token Counter

Reference implementations (Codex, Claude Code) display per-turn and cumulative token usage from API responses. This gap is explicitly separable from the compaction deferral gate: counting tokens reported in API responses requires no summarisation strategy and must not be blocked by the compaction ADR requirement.

**Source:** token counts are read from the `usage` field in model API responses (`input_tokens`, `output_tokens`). For local runtimes that do not return usage fields, counts are estimated at `chars ÷ 4` with an `(estimated)` annotation displayed wherever the count appears.

**Session accumulator:** `RuntimeContext` maintains a `SessionTokens { input: u64, output: u64, last_input: u64, last_output: u64, estimated: bool }` field incremented after each completed turn. The accumulator is reset on `/new` and `/clear` — conversation history is discarded at those points, making the running total meaningless.

**`/usage` command** (added to `try_handle_slash_command`):

```
/usage
    Renders token usage for the current session to transcript via
    push_history_line. No model turn.
    Output format:
      [usage]
        this turn   : <last_input> in / <last_output> out
        session     : <total_input> in / <total_output> out  (estimated)
    If no turns have completed: "[usage] no turns completed this session".
    The "(estimated)" annotation is appended when estimated = true.
```

**`BatchMode` JSONL:** each turn evidence block includes a `tokens` object: `{"input": N, "output": N, "estimated": false}`.

**Gating:** the `/usage` TUI command does not depend on Gap 2 (`BatchMode`) and may be implemented independently. The `BatchMode` JSONL `tokens` field extension requires Gap 2 to exist first.

**Anchor tests:** `test_session_token_accumulator_increments_per_turn`; `test_session_token_accumulator_resets_on_new`; `test_session_token_accumulator_resets_on_clear`; `test_tui_usage_renders_last_and_session_totals`; `test_tui_usage_empty_session`; `test_tui_usage_does_not_call_start_turn`; `test_batch_mode_jsonl_includes_tokens_field`.

---

### Gap 29 — Conversation and Task Export (`vex export`)

Reference implementations allow operators to export conversation and task artifacts for archiving, audit, or sharing. `vex export` is a read-only CLI sub-command that reads a saved `TaskState` from `VEX_STATE_DIR` and writes a structured export artifact to stdout or a named file.

```bash
vex export <task-id> [--format jsonl|markdown] [--output <path>] [--force]
```

**Formats:**

| Format | Content |
| :--- | :--- |
| `jsonl` (default) | One JSON object per line: task metadata, changed files, command history, turn evidence. Schema is identical to `BatchMode` JSONL output so tooling built for `vex exec` works for `vex export` without modification. |
| `markdown` | Human-readable document: task metadata header, changed files table, command history, and per-turn summaries (tool names and outcomes only — not full model response text). |

**Rules:**

- `vex export` is read-only. It must not modify `TaskState`, change file content, or write any state.
- Output defaults to stdout. `--output <path>` writes to a file; fails with a non-zero exit and a diagnostic if the file already exists unless `--force` is passed.
- Unknown or unreadable task-id produces a non-zero exit with a clear diagnostic.
- The Markdown format must not reproduce full model response text — turn summaries contain tool invocations and outcomes only. Full model response content is available only in JSONL.
- `vex export` does not require a running agent session; it operates entirely from persisted `TaskState` on disk.

**Anchor tests:** `test_vex_export_jsonl_matches_batch_schema`; `test_vex_export_markdown_omits_model_response_text`; `test_vex_export_unknown_task_id_exits_nonzero`; `test_vex_export_does_not_modify_state`; `test_vex_export_output_path_flag`; `test_vex_export_force_flag_overwrites`.

---

### Gap 30 — `--resume` CLI Startup Flag

Gap 14 adds `/resume` as a TUI slash command for resuming a saved task from within a running session. This gap is distinct: `--resume [<task-id>]` is a CLI startup flag that loads a `TaskState` before `TuiMode` initialises, so operators who exited entirely can resume their last task without first landing in a blank session.

```bash
vex --resume                 # loads the most recently modified TaskState in VEX_STATE_DIR
vex --resume <task-id>       # loads the named TaskState
```

**Behaviour:** The flag is handled in `src/bin/vex.rs` before `TuiMode` starts. `TaskState::load` is called for the specified or most-recent task-id. On success, `TuiMode` is initialised with `active_grants` and `changed_files` restored from the saved state, and an informational line `[resumed: <task-id> status=<status>]` is prepended to the transcript. On failure (task-id not found, state file unreadable), the process exits non-zero with a clear diagnostic before any TUI initialisation occurs. Conversation history is not restored — this matches the behaviour of `/resume` (Gap 14): `TaskState` does not persist message content.

**Implementation scope:** `src/bin/vex.rs` only. No changes to `src/runtime/`, `src/state/`, or `src/tools/`. `TaskState::load` already exists; this is routing only.

**Relationship to `-p`/`--print` (Gap 32):** `--resume` and `--print` are independent startup flags. `--resume --print "continue"` is a valid combination: load the saved task context, run one turn non-interactively, and print the result to stdout.

**Anchor tests:** `test_cli_resume_flag_loads_task_state`; `test_cli_resume_flag_restores_active_grants`; `test_cli_resume_flag_unknown_task_id_exits_nonzero`; `test_cli_resume_flag_most_recent_when_no_id_given`.

---

### Gap 31 — MCP HTTP Server Authentication Headers (extends Gap 5)

Gap 5 specifies HTTP-transport MCP servers by URL only. Self-hosted or LAN-hosted MCP servers commonly require an authentication header (e.g. `Authorization: Bearer <token>`). Without a header configuration field, any HTTP MCP server that requires authentication cannot be used with `vexcoder`.

**Configuration** (user config only — same layer restriction as `[[mcp_servers]]`):

```toml
# ~/.config/vex/config.toml — user config layer only

[[mcp_servers]]
name      = "private-search"
transport = "http"
url       = "https://mcp.example.internal/mcp"

[mcp_servers.headers]
Authorization = "${MCP_PRIVATE_SEARCH_TOKEN}"   # env-var reference — value is never stored in config
X-Client-Id   = "vexcoder"
```

**Secret handling:** Header values support `${ENV_VAR_NAME}` substitution. The substitution is resolved from the environment at session start, not stored in the config file. A header value that contains a literal `${}` reference to an unset environment variable is a hard startup failure with a diagnostic naming the missing variable and the server. A header value with no `${}` syntax is used verbatim — this permits non-secret headers such as `X-Client-Id`. Secrets must never be written to any config file layer; header values containing sensitive tokens must always use env-var references.

**STDIO transport:** The `headers` field is not applicable to STDIO servers and must be rejected with a diagnostic at config load time if present on a STDIO entry.

**Anchor tests:** `test_mcp_http_header_injected_on_request`; `test_mcp_http_header_env_var_substituted`; `test_mcp_http_header_unset_env_var_is_hard_failure`; `test_mcp_http_header_literal_value_used_verbatim`; `test_mcp_stdio_headers_field_rejected`.

---

### Gap 32 — `-p`/`--print` One-Shot Plain-Text Flag

Reference CLI agents expose a `-p`/`--print` flag for pipe-friendly one-shot queries: the agent runs a single turn, prints the plain assistant response to stdout, and exits. This is distinct from `vex exec` (`BatchMode`): `BatchMode` is designed for multi-turn automation with full JSONL evidence output; `--print` is designed for scripting that needs only a plain text answer. Both are headless; their output formats and use cases do not overlap.

```bash
# pipe input
git diff HEAD | vex -p "summarise these changes in one paragraph"
cat src/foo.rs | vex -p "identify any error-handling issues"

# direct task
vex --print "what does the Config::validate function do?"

# combined with --resume
vex --resume <task-id> --print "what files did you change?"
```

**Behaviour:** `-p`/`--print` is a `BatchMode` invocation with the following fixed parameters: `--max-turns 1`, `--format text`, no JSONL evidence output, no changed-file tracking appended to `TaskState`. Stdin is read and prepended to the prompt if stdin is not a TTY (pipe mode). Output is the assistant's final response text only, written to stdout. Exit code: 0 on a completed turn; non-zero on model error or approval denial.

**Implementation:** `-p`/`--print <prompt>` is a `clap` flag pair (short `-p`, long `--print`) in `src/bin/vex.rs` that routes to `BatchMode` with the parameters above. It does not introduce a new runtime mode. `BatchMode` must already exist (Gap 2) for this flag to be implemented; Gap 32 is therefore gated on Gap 2 completion.

**No `ratatui` or `crossterm` in the execution path.** The existing `BatchMode` CI check covers this.

**Anchor tests:** `test_print_flag_runs_single_turn`; `test_print_flag_reads_stdin_pipe`; `test_print_flag_outputs_plain_text`; `test_print_flag_exits_nonzero_on_error`; `test_print_flag_routes_to_batch_mode`.

---

### Gap 35 — Model-Callable Workspace Exploration Tools

Reference CLI agents expose a set of read-only, model-callable tools for autonomous
workspace exploration: file-pattern search (`search_files`), directory listing
(`list_dir`), and glob-based path enumeration (`glob_files`). These are distinct
from every existing mechanism in the ADR chain:

- `ContextAssembler` (ADR-023) runs automatically before each turn and provides a
  pre-assembled snapshot — it is not model-callable.
- `@<path>` expansion (Gap 19) is an operator-driven input transformation — the
  model cannot invoke it.
- `!<command>` passthrough (Gap 20) routes through `SandboxDriver::wrap` and
  requires `Capability::RunCommand` approval — appropriate for arbitrary shell
  commands, not for read-only file enumeration.
- Gap 12 (code indexing) is a semantic/embedding-based search capability —
  formally deferred. The tools in this gap are literal-string-matching and
  directory-listing primitives that require no index, no external service, and
  no crate beyond the Rust standard library.

Without these tools, the model cannot autonomously discover where a symbol is used,
what files exist in a directory, or which paths match a pattern in an unfamiliar
codebase. It must rely on the operator to supply context via `@<path>` or `!<cmd>`
for every exploration step.

**Tools introduced:**

```
search_files(pattern: &str, path: Option<&str>) -> SearchResult
    Searches file content for lines containing <pattern> as a literal string
    (case-sensitive, fixed-string match via `str::contains`). No regex support.
    <path> is relative to the workspace root; defaults to workspace root if omitted.
    Returns up to MAX_SEARCH_RESULTS (default: 50) matching lines, each annotated
    with relative path and line number. Workspace-root confinement applies;
    out-of-workspace paths return a structured error annotation.

list_dir(path: &str) -> DirListing
    Lists the immediate contents of <path> relative to the workspace root.
    Returns file names, directory names (suffixed /), and sizes. Does not recurse.
    Bounded to MAX_DIR_ENTRIES (default: 200) entries.

glob_files(pattern: &str) -> GlobResult
    Returns all workspace-relative paths matching <pattern> (standard glob syntax:
    *, **, ?). Bounded to MAX_GLOB_RESULTS (default: 100) paths. Workspace-root
    confinement applies.
```

**Capability tier:** These tools are read-only and require no subprocess execution.
They are gated under `Capability::ReadFile` (the existing capability for
`read_file`). A future ADR may introduce a distinct `Capability::ReadWorkspace` if
finer-grained control is needed; this ADR does not require it.

**Ignore rules:** All three tools must skip paths excluded by `.gitignore` and any
workspace ignore mechanism active at the time of implementation. PP-01 must not
begin until a workspace ignore ruleset is available in the codebase; the exact
mechanism must be confirmed green before PP-01 starts.

**Implementation scope:** `src/tools/workspace_explore.rs` — new file. Three tool
handler functions registered in the existing dispatch table alongside `read_file`,
`write_file`, and `apply_patch`. No new `RuntimeMode`, no new `Capability` variant,
no subprocess calls.

**Constraints:**
- All three tools must use `ToolOperator`'s workspace-root confinement guards
  (ADR-002). Out-of-workspace paths return a structured error, not an abort.
- `search_files` must not use `std::process::Command`. Pattern matching is
  implemented in-process using `str::contains` from the Rust standard library.
  No regex support; no crate dependency beyond `std`.
- None of the three tools start a model turn or modify any file.
- Results must be bounded. Truncated results include an annotation:
  `[results truncated — showing first <n> of <total> matches]`.

**Anchor tests:** `test_search_files_returns_matching_lines`;
`test_search_files_respects_workspace_root`;
`test_search_files_skips_gitignore_excluded_paths`;
`test_search_files_literal_match_no_partial_regex_interpretation`;
`test_list_dir_returns_immediate_contents`;
`test_list_dir_does_not_recurse`;
`test_glob_files_returns_matching_paths`;
`test_glob_files_bounded_results`;
`test_workspace_tools_do_not_start_model_turn`;
`test_workspace_tools_out_of_workspace_path_returns_error`.

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
| `VEX_MAX_MEMORY_TOKENS` | Token budget for user notes injection | `2048` |
| `VEX_AT_INJECT_MAX_BYTES` | Max bytes per `@<path>` inline file injection | `32768` (shared with `ContextAssembler::max_file_bytes`) |

### New prompt templates (additions to ADR-023 `src/prompts/`)

These files are added to the `src/prompts/` directory under ADR-023 EL-06 scope:

```
src/prompts/pr_summary_template.txt   — Gap 24: branch diff → PR title + body
src/prompts/generate_tests_template.txt — Gap 25: source file → test file
```

Both files must pass `scripts/check_forbidden_names.sh`. Both are loaded via `include_str!` at compile time.

---

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
| **PJ-01** | `/clear` — clears conversation history; preserves task and grants; clears `active_edit_loop` | [ ] |
| **PJ-02** | `/fork [<label>]` — saves parent; creates new task-id; copies grants; does not copy conversation | [ ] |
| **PJ-03** | `/memory`, `/memory add`, `/memory clear` — notes file; session injection; token budget | [ ] |
| **PJ-04** | `vex init` — scaffolds `.vex/config.toml`, `AGENTS.md`, `.vex/validate.toml`; non-destructive | [ ] |
| **PK-01** | `/quit`, `/exit` — graceful shutdown with TaskState::save and EditLoop cancel | [ ] |
| **PK-02** | `/about` — build metadata display; `build.rs` compile-time injection | [ ] |
| **PK-03** | `@<path>` inline injection — workspace-confined; truncation annotation; multi-token | [ ] |
| **PK-04** | `!<command>` passthrough — SandboxDriver + ApprovalPolicy; no model turn | [ ] |
| **PK-05** | User-defined commands — TOML loader; project + user scopes; `/commands` integration | [ ] |
| **PK-06** | `/tools [desc]` — live dispatch table enumeration; MCP-namespaced tools | [ ] |
| **PK-07** | `/diff [--staged]` — spawn_blocking git diff; truncation; no model turn | [ ] |
| **PK-08** | `vex branch` and `vex pr-summary` — thin git wrappers; stdout output; no platform API | [ ] |
| **PK-09** | `/generate-tests` — generate_tests_template.txt; non-test patch filter; framework flag | [ ] |
| **PL-01** | Pre/post-tool-call hooks — `[[hooks]]` config; `Capability`-triggered; `SandboxDriver`-wrapped; user-layer only | [ ] |
| **PL-02** | `vex doctor` — config probe, endpoint reachability, sandbox probe, MCP connectivity, `--json` output | [ ] |
| **PL-03** | Session token counter — turn accumulator; `/usage` command; `BatchMode` JSONL `tokens` field | [ ] |
| **PL-04** | `vex export <task-id>` — JSONL and Markdown formats; read-only; `--output`/`--force` flags | [ ] |
| **PM-01** | `--resume [<task-id>]` startup flag — `TaskState::load` before TUI init; non-zero exit on failure | [ ] |
| **PM-02** | MCP HTTP `[mcp_servers.headers]` — env-var substitution; STDIO rejection; startup failure on unset var | [ ] |
| **PM-03** | `-p`/`--print` flag — `BatchMode` single-turn; stdin pipe; plain-text stdout; gated on Gap 2 | [ ] |
| **PP-01** | `search_files`, `list_dir`, `glob_files` — workspace-confined; `.gitignore`-aware; bounded results; registered in dispatch table; gated on workspace ignore mechanism being available | [ ] |

## Dispatcher reporting contract (mandatory per checklist item)

When checking a box above, append an evidence block under this section:

```markdown
### [PA-01 … PM-03] - <short title>
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
| Do not use `std::process::Command` in `src/tools/workspace_explore.rs` | `search_files` must use `str::contains` from the Rust standard library; no subprocess permitted; no `regex` crate or external pattern-matching dependency |
| `search_files`, `list_dir`, and `glob_files` must skip `.gitignore`-excluded paths | Apply at minimum `.gitignore` rules before returning results; extend to any workspace ignore mechanism once available |
| PP-01 must not begin until a workspace ignore ruleset is available in the codebase | Ignore integration is a correctness requirement, not an enhancement |
| Do not implement conversation compaction, turn pruning, or `ConversationCheckpoint` summarisation without a dedicated ADR | Formal deferral gate; `/compact` and richer `/usage` are part of this gate |
| Do not implement `/undo` without a dedicated ADR specifying rollback strategy | Gap 14 formal deferral gate |
| `/allow` and `/deny` must derive capability names from the `Capability` enum at compile time | No hardcoded string list permitted; drift between enum and command surface must be a compile error |
| `/allow session` must not call `TaskState::save` | Session grants are in-memory only; persistence belongs to config layering (Gap 3) |
| `/mcp add` and `/mcp remove` must not be implemented under this ADR | Runtime MCP lifecycle management requires a dedicated ADR |
| `/new` must call `TaskState::save` before resetting; abort if save fails | Data loss prevention — never discard a live task state without a successful save |
| `/clear` must clear `active_edit_loop` on `TuiMode` | A running edit loop cannot continue after its conversation history is discarded |
| `/fork` must call `TaskState::save` for the parent before creating the fork; abort fork if save fails | Data loss prevention — never branch without preserving the parent |
| `/fork` must not copy conversation history to the fork | The fork begins with an empty conversation window and inherited grants only |
| `/memory` notes file must be resolved from the user config layer only | Notes are operator-personal; repo-local config must not be able to set the notes file path |
| `/memory clear` must require a confirmation prompt in `TuiMode`; must treat confirmation as denied in `BatchMode` unless `--auto-approve` is passed | Non-interactive clear without explicit operator confirmation is prohibited |
| `vex init` must not overwrite existing files | Non-destructive scaffolding only; skip and report any file that already exists |
| `vex init` generated `.vex/config.toml` must contain all normative config keys from this ADR, commented out | Enforced by `test_vex_init_config_keys_match_normative_list` |
| `/quit` and `/exit` must call `TaskState::save` before exiting; cancel any active `EditLoop` via `CancellationToken` | Never force-exit while `active_edit_loop` is `Some` |
| `@<path>` expansion must use `ToolOperator`'s workspace-root confinement | Reject out-of-workspace paths with inline annotation; do not abort the turn |
| `@<path>` expansion must not be applied inside slash-command arguments | Only applied to free-form input before the slash-command check |
| `!<command>` must route through `SandboxDriver::wrap` and require `Capability::RunCommand` approval | Never bypass the approval gate for shell passthrough |
| User-defined commands must not shadow built-in command names | Built-in names take precedence at dispatch; user commands that conflict are silently skipped with a startup warning |
| User-defined command `name` field must match `[a-z0-9-]+`; names beginning `vex-` are reserved | Enforced at load time; invalid names cause a startup warning and the command is skipped |
| `/generate-tests` must never apply patches to non-test files | Test-file path filter applied before `PendingApproval` gate; non-test patches silently dropped |
| `pr_summary_template.txt` and `generate_tests_template.txt` must pass `check_forbidden_names.sh` | Added to EL-06 CI scope |
| `[[hooks]]` must not be permitted in repo-local `.vex/config.toml` | Same supply-chain rationale as `[[mcp_servers]]`; reject with a diagnostic at config load time |
| `[[hooks]]` commands must route through `SandboxDriver::wrap` and require `Capability::RunCommand` approval | A hook skipped for missing approval emits a warning and continues the turn; it must never silently block |
| `[[hooks]]` `on_fail = "abort"` must abort the pending tool result and surface the error to the operator; it must not terminate the process | A hook failure is a tool-level event, not a session-level event |
| Do not begin Phases G or H before milestone-1 correctness work is validated | Sequencing guard |
| Do not add a dependency licensed under a commercial, copyleft, or conditionally-paid license | All direct dependencies must carry MIT, Apache 2.0, or dual MIT/Apache 2.0 licensing; exceptions require a dedicated ADR with explicit legal basis |
| Do not use provider-branded names or proprietary product references in runtime code, config keys, or default values | Documentation may reference external tools by name for operator clarity; runtime behaviour must remain neutral. **Migration tooling exception (Gap 11):** `vex migrate config` is the sole permitted context in which pre-ADR-022 branded variable values (e.g. `VEX_API_PROTOCOL=anthropic`) may be read at runtime — exclusively to map them to neutral equivalents. No other code path may read or emit branded values. |
| MCP HTTP header values containing secrets must use `${ENV_VAR_NAME}` substitution; literal secrets must never appear in config files | Enforced at config load time: values without `${}` syntax are used verbatim and are assumed non-secret; values with `${}` are resolved from environment only |
| `-p`/`--print` must not be implemented before Gap 2 (`BatchMode`) is complete | `--print` is a routing flag over `BatchMode`; implementing it without `BatchMode` requires duplicating runtime logic, which is prohibited |

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
