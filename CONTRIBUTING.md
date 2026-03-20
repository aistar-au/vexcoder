# Contributing to vexcoder

> **Version:** This workflow applies from `v0.1.0-alpha2` onward.
> **Architecture decisions** live in [`adr/`](adr/ADR-README.md).
> The ADRs explain *why* the project is structured this way. Read them before opening a PR.
>
> **Agent bootstrap:** repo-local product guidance stays here, but the active
> dispatcher skills now live in the internal private repo
> `../vexdraft/.agents/skills/`.
> Read [`AGENTS.md`](AGENTS.md) first for the dependency map and required load
> order before using the private skill tree against this repo.

---

## The Agentic Workflow (Test-Driven Manifest)

`vexcoder` uses the **Test-Driven Manifest (TDM)** strategy for all bug fixes, features, and refactors. The full rationale is in [ADR-001](adr/completed/ADR-001-tdm-agentic-manifest-strategy.md). The short version:

1. **Identify task** — Check `adr/` for open architecture decisions.
2. **Anchor test** — Every task has exactly one failing Rust test before work begins. No anchor, no dispatch.
3. **Module isolation** — Work is confined to the `Target File` named in the task manifest (± one helper file).
4. **Verification** — Success is `cargo test <anchor_name>` passing, plus `cargo test --all-targets` showing no regressions.

Runtime mode additions and naming-policy changes require explicit confirmation before implementation or documentation. See ADR-007.
Canonical production dispatch is runtime-core only: `Runtime<M>::run` → `RuntimeMode::on_user_input` → `RuntimeContext::start_turn`.
Alternate app-owned dispatch channels are forbidden in production paths.
Runtime-core ratatui TUI behavior must conform to ADR-009, ADR-010, and ADR-011 before merge.
Runtime-core TUI deployment is gated by ADR-012; no deploy if any ADR-012 item is unmet.
Architecture gates enforcing ADR-007 must remain green:
`bash scripts/check_no_alternate_routing.sh`
`bash scripts/check_forbidden_imports.sh`
Tests that mutate process environment variables must hold `crate::test_support::ENV_LOCK`; `cargo test --all-targets` must pass without `--test-threads=1`.

---

## Planning and Audit-Only Requests

Planning-only and audit-only requests are strictly no-touch by default:
no file create, edit, rename, move, or delete is allowed during a planning/audit-only pass.

If the user later asks to implement changes in the same session, switch to edit mode only
after explicit user confirmation.

Use the same explicit-confirmation standard already required for runtime mode additions and
naming-policy changes.

---

## Task Naming Convention

| Prefix | Type | Example |
| :--- | :--- | :--- |
| `CRIT-XX` | Critical bug | `CRIT-02-serde-fix.md` |
| `FEAT-XX` | Feature | `FEAT-01-streaming-ui.md` |
| `REF-XX` | Refactor | `REF-02-runtime-contract.md` |
| `SEC-XX` | Security | `SEC-01-path-security.md` |
| `CORE-XX` | Core infrastructure | `CORE-01-sse-parser.md` |
| `DOC-XX` | Documentation | `DOC-01-api-docs.md` |

---

## Rust Module File Naming (Rust 2018+)

Use path-based module entry files across `src/`.

| Situation | Required path |
| :--- | :--- |
| Top-level module entry | `src/<module>.rs` |
| Nested module | `src/<module>/<submodule>.rs` |

Do not introduce new `src/*/mod.rs` files unless an external tool or macro
requires that layout.

---

## Runtime-core Status

REF-08 full cutover is complete and merged (2026-02-19).
Canonical dispatch and layering rules are now governed by ADR-007 and ADR-008.

---

## Quick Start

```bash
# 1. Install Rust (stable toolchain required)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 2. Verify the environment
cargo test --all-targets

# 3. Read the relevant ADR in adr/, identify the anchor test

# 4. Implement, then verify
cargo test test_crit_XX_anchor_name -- --nocapture

# 5. Confirm no regressions
cargo test --all-targets
bash scripts/check_no_alternate_routing.sh
bash scripts/check_forbidden_imports.sh
```

---

## Release Packaging

Package release changes on a dispatcher branch first, verify them locally, and open the PR without waiting on a duplicate packaging workflow run.

```bash
git switch -c dispatcher/v0.1.0-alpha2-packaging
make gate
make release TARGET=x86_64-unknown-linux-gnu
git push -u origin dispatcher/v0.1.0-alpha2-packaging
```

On Windows PowerShell 7, use the native packaging script instead of `make release`:

```powershell
git switch -c dispatcher/v0.1.0-alpha2-packaging
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build --release --bin vex
.\scripts\release.ps1 -Target x86_64-pc-windows-msvc -RunGate
git push -u origin dispatcher/v0.1.0-alpha2-packaging
```

Windows packaging is currently an unsigned alpha path. Platform trust warnings are expected until code signing lands; evaluate a compatible signing service only when the packaging ADR set explicitly requires it.

The packaging scripts derive the archive tag from `Cargo.toml` and reject mismatched tag inputs. `.github/workflows/release.yml` now runs only for tag pushes and manual dispatch so dispatcher branches do not duplicate the main PR checks. After the branch gates are green and the local packaging smoke checks look correct, open the PR. Publish the prerelease only after the merge commit is on `main`:

```bash
git switch main
git pull --ff-only origin main
git tag v0.1.0-alpha2
git push origin v0.1.0-alpha2
```

Do not merge packaging work directly from a local debug session; keep the review and merge step explicit.

---

## Remote Agent Sessions

Repository-level background sessions can use the private skill tree from
`aistar-au/vexdraft` through the repository setup workflow and the
custom agent profiles under `.github/agents/`.

- The setup workflow syncs `vexdraft/.agents/skills/` into the background
  session home directory and reads the private-repo clone credential from the
  platform environment secret referenced by the setup workflow.
- Repository-wide background-session guidance now lives under
  `.github/instructions/`, while skill bootstrap stays in the setup workflow and
  the custom agent profiles.
- The setup workflow only affects background sessions after it lands on the
  default branch. Manual workflow dispatch is still useful for validating the
  bootstrap steps on a feature branch before merge.
- The repository-level agent profile follows the branch you target. Use a
  dispatcher branch as the `--base` argument when you want the remote session
  to see branch-local agent changes.
- New or renamed custom agent profile files are only selectable through
  `gh agent-task --custom-agent` after they exist on the default branch.
  Branch-local profiles remain useful for repository content and follow-up
  promotion work, but the remote agent catalog itself is resolved from the
  default-branch profile set before the session starts.

Available profiles:

- `vexcoder-ui-parity-orchestrator` for broad fullscreen UI, task-state, and
  stale-doc work.
- `vexcoder-ui-paragraph-renderer` for transcript drawing, paragraph rendering,
  and enriched tool-detail layout work.

Start a UI parity session from the GitHub CLI with:

```bash
gh agent-task create \
  --base <dispatcher-branch> \
  --custom-agent vexcoder-ui-parity-orchestrator \
  --follow \
  "Investigate the fullscreen UI, task-state control surface, scrolling, and stale docs."
```

Tail an existing session with:

```bash
gh agent-task view <session-id-or-pr> --log --follow
```

Start a paragraph-rendering session with:

```bash
gh agent-task create \
  --base <dispatcher-branch> \
  --custom-agent vexcoder-ui-paragraph-renderer \
  --follow \
  "Investigate paragraph-style tool rendering, transcript drawing, and stale docs."
```

When promoting remote-session changes, mirror or cherry-pick them onto a
repo-qualified dispatcher branch before commit-debug, push, and PR creation.

---

## Project Structure

```
~/git-repo/
├── vexcoder/               # This repo — product code and release CI only
│   ├── CONTRIBUTING.md
│   ├── README.md
│   ├── adr/           # Architecture Decision Records
│   ├── src/                # Rust crate source
│   └── tests/              # Integration tests
└── vexdraft/               # Adjacent devops repo — dispatcher, commit-debug, skills
    └── scripts/
        └── commit-debug.py # Multi-provider pre-push reviewer (called by dispatcher)
```

`vexdraft` must exist at `../vexdraft` relative to this repo for the dispatcher
loop and pre-push review to function. The internal layout is the assumed path contract.

```
vexcoder/ (standalone view)
├── CONTRIBUTING.md                # Workflow guide + source map
├── README.md                      # Runtime and quickstart
├── adr/                      # Architecture Decision Records (open + completed)
├── src/                           # Rust crate source
│   └── bin/vex.rs                 # Binary entrypoint
└── tests/                         # Integration tests
```

---

## Tracked Rust Source Map (`*.rs`)

| File | Short description (with raw URL) |
| :--- | :--- |
| `src/lib.rs` | Crate root exporting runtime/app/api/state/tools/ui modules. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/lib.rs> |
| `src/bin/vex.rs` | Production binary entrypoint and managed TUI startup loop. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/bin/vex.rs> |
| `src/api.rs` | API module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api.rs> |
| `src/api/client.rs` | HTTP client, protocol selection, request/stream setup, tool schemas. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/client.rs> |
| `src/api/logging.rs` | Shared API debug/error logger and env-based log path handling. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/logging.rs> |
| `src/api/mock_client.rs` | Mock streaming client used by tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/mock_client.rs> |
| `src/api/stream.rs` | Stream/SSE event parsing helpers used by API layer. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/stream.rs> |
| `src/app.rs` | Current interactive application module root: TUI mode state, input, overlays, history, and runtime-facing coordination. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app.rs> |
| `src/app/accessors.rs` | TuiMode status and read-only accessor methods extracted from app facade under ADR-028 phase 4. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/accessors.rs> |
| `src/app/commands.rs` | Slash-command handler methods extracted from app facade under ADR-028 phase 1. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/commands.rs> |
| `src/app/ctor.rs` | TuiMode construction methods extracted from app facade under ADR-028 phase 4. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/ctor.rs> |
| `src/app/errors.rs` | AppError wrapper type and AppResult type alias for error handling. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/errors.rs> |
| `src/app/facade.rs` | API client facade builder with project instructions and bootstrap configuration. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/facade.rs> |
| `src/app/inline.rs` | Inline `@`-token file-expansion methods extracted from app facade under ADR-028 phase 3. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/inline.rs> |
| `src/app/input.rs` | User-input and interrupt handler methods extracted from app facade under ADR-028 phase 5. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/input.rs> |
| `src/app/layout.rs` | Layout-state and command-routing helper methods extracted from app facade under ADR-028 phase 4. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/layout.rs> |
| `src/app/model_update.rs` | Model-update handler methods extracted from app facade under ADR-028 phase 5. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/model_update.rs> |
| `src/app/overlay.rs` | Overlay and approval handler methods extracted from app facade under ADR-028 phase 2. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/overlay.rs> |
| `src/app/runtime_build.rs` | Runtime-construction functions `build_runtime` and `build_runtime_with_resume` extracted from app facade under ADR-028 phase 6. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/runtime_build.rs> |
| `src/app/scroll.rs` | Viewport and history scroll methods extracted from app facade under ADR-028 phase 2. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/scroll.rs> |
| `src/app/shell.rs` | Bang-command approval and command-session spawn methods extracted from app facade under ADR-028 phase 3. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/shell.rs> |
| `src/app/tests.rs` | App-level unit and integration tests extracted from app facade under ADR-028 phase 1. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/tests.rs> |
| `src/app/turn.rs` | Turn-lifecycle and command-session tracking methods extracted from app facade under ADR-028 phase 2. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/turn.rs> |
| `src/app/turn_start.rs` | Turn-dispatch and context-assembly helper methods extracted from app facade under ADR-028 phase 3. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/turn_start.rs> |
| `src/app/util.rs` | Module-level helper functions extracted from app facade under ADR-028 phase 1. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/util.rs> |
| `src/batch_mode.rs` | Non-interactive batch runner for `vex exec`, including JSONL and text turn output. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/batch_mode.rs> |
| `src/config.rs` | Layered config loading and validation across environment, repo-local, user, and system sources. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/config.rs> |
| `src/custom_commands.rs` | Custom user-defined commands loaded from `.vex/commands` directory. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/custom_commands.rs> |
| `src/doctor.rs` | Diagnostic checks for system configuration and health status. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/doctor.rs> |
| `src/edit_diff.rs` | Edit preview diff/hunk formatting utilities. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/edit_diff.rs> |
| `src/export.rs` | Task execution record export in JSONL or Markdown format. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/export.rs> |
| `src/git_hooks.rs` | Git hook install/remove helpers and commit-trailer hook script. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/git_hooks.rs> |
| `src/local_api.rs` | HTTP server implementation for local API with axum web framework. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/local_api.rs> |
| `src/prompts.rs` | Prompt template loading and rendering for code generation tasks. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/prompts.rs> |
| `src/runtime.rs` | Runtime module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime.rs> |
| `src/runtime/approval.rs` | Capability-based approval policies for sandboxed operations. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/approval.rs> |
| `src/runtime/backend.rs` | Model backend types and protocol abstractions for LLM integration. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/backend.rs> |
| `src/runtime/command.rs` | Command execution: one-shot, streaming, PTY, and process group management. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/command.rs> |
| `src/runtime/context.rs` | Async turn execution context, edit-turn driver, and conversation update forwarding. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/context.rs> |
| `src/runtime/context_assembler.rs` | Context assembly for model turns (file snapshots and prompt construction). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/context_assembler.rs> |
| `src/runtime/frontend.rs` | Frontend adapter contracts and runtime-facing input event types. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/frontend.rs> |
| `src/runtime/json_handoff.rs` | ADR-025 canonical runtime JSON handoff types: `RuntimeEnvelope`, `RuntimeEvent`, `RuntimeRequest`, and related contracts. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/json_handoff.rs> |
| `src/runtime/loop.rs` | Runtime event loop orchestration between mode, frontend, and context. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/loop.rs> |
| `src/runtime/edit_loop.rs` | Task-completion edit loop: assemble→model→apply→validate→retry cycle. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/edit_loop.rs> |
| `src/runtime/mode.rs` | Runtime mode trait defining input/update hooks. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/mode.rs> |
| `src/runtime/policy.rs` | Output sanitization and tool-evidence policy helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/policy.rs> |
| `src/runtime/project_instructions.rs` | Project-level instructions loading from `.vex/AGENTS.md` or `AGENTS.md` with token budget enforcement. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/project_instructions.rs> |
| `src/runtime/sandbox.rs` | Command sandboxing trait and implementations with wrapper and probe methods. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/sandbox.rs> |
| `src/runtime/task_state.rs` | Task execution state management with status tracking and evidence collection. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/task_state.rs> |
| `src/runtime/text_util.rs` | UTF-8 aware text truncation utilities respecting byte boundaries. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/text_util.rs> |
| `src/runtime/update.rs` | `UiUpdate` message types emitted from runtime to frontend. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/update.rs> |
| `src/runtime/validation.rs` | Concurrent validation suite: command execution, retry formatting. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/validation.rs> |
| `src/state.rs` | State module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state.rs> |
| `src/state/conversation.rs` | Conversation module entrypoint and re-exports for split conversation submodules. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation.rs> |
| `src/state/conversation/core.rs` | Main conversation turn loop, streaming event processing, and model/tool round orchestration. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/core.rs> |
| `src/state/conversation/history.rs` | Message history pruning, truncation, and read-file result summarization helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/history.rs> |
| `src/state/conversation/state.rs` | Conversation state types and `ConversationManager` constructors/accessors. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/state.rs> |
| `src/state/conversation/streaming.rs` | Stream block lifecycle helpers, block promotion, and delta emission utilities. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/streaming.rs> |
| `src/state/conversation/tests.rs` | Conversation module tests covering protocol flow, loop guards, and regression anchors. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/tests.rs> |
| `src/state/conversation/tools.rs` | Tool execution dispatch, approval gating, input parsing, and tool-loop guard helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/tools.rs> |
| `src/state/stream_block.rs` | Structured stream block models and tool status enum. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/stream_block.rs> |
| `src/terminal.rs` | Terminal raw-mode lifecycle and panic-safe restore guard. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/terminal.rs> |
| `src/test_support.rs` | Shared test synchronization helpers (e.g., env lock). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/test_support.rs> |
| `src/tool_preview.rs` | Tool approval preview rendering and read-file snapshot summaries. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tool_preview.rs> |
| `src/tools.rs` | Tools module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tools.rs> |
| `src/skills.rs` | Skills registry load/list/install/remove helpers for `.agents/skills`. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/skills.rs> |
| `src/session_notes.rs` | Session notes loading and resolution with memory token budget management. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/session_notes.rs> |
| `src/tools/operator.rs` | Sandboxed file/git tool operator with path safety and literal search. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tools/operator.rs> |
| `src/turn_evidence.rs` | Turn evidence recording: input, response, file changes, and command history. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/turn_evidence.rs> |
| `src/types.rs` | Types module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/types.rs> |
| `src/types/api_types.rs` | API request/response content and streaming event structs/enums. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/types/api_types.rs> |
| `src/types/model_profile.rs` | ModelProfile configuration for LLM models with system prompt and parameters. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/types/model_profile.rs> |
| `src/ui.rs` | UI module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui.rs> |
| `src/ui/draw/mod.rs` | Adaptive ANSI draw engine for persistent full-screen operator workspace surface. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/draw/mod.rs> |
| `src/ui/draw/ansi.rs` | ANSI escape helpers, color palette, spinner and progress animation frames. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/draw/ansi.rs> |
| `src/ui/draw/regions.rs` | Adaptive layout region computation for the four-region workspace surface. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/draw/regions.rs> |
| `src/ui/draw/transcript.rs` | Transcript line rendering with markdown, checklists, numbered lists, and inline formatting. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/draw/transcript.rs> |
| `src/ui/draw/tests.rs` | Draw engine unit tests covering layout, rendering, and hash computation. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/draw/tests.rs> |
| `src/ui/editor.rs` | Text input editor with history, undo/redo stacks, and cursor management. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/editor.rs> |
| `src/ui/input_metrics.rs` | Input editor row/width metrics for viewport-safe rendering. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/input_metrics.rs> |
| `src/ui/layout.rs` | Ratatui pane layout splitting and geometry helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/layout.rs> |
| `src/ui/render.rs` | Ratatui render functions for status, history, input, and overlays. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/render.rs> |
| `src/util.rs` | Shared utility functions (bool/env parsing and endpoint helpers). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/util.rs> |
| `src/usage.rs` | Token usage tracking structures for turns and sessions. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/usage.rs> |
| `src/workspace.rs` | Repo-root and workspace-relative path helpers for repo-scoped state. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/workspace.rs> |
| `tests/integration_test.rs` | Integration tests for config validation behavior. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/integration_test.rs> |
| `tests/layout_underflow_tests.rs` | TUI layout constraint and underflow regression tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/layout_underflow_tests.rs> |
| `tests/signal_handling_tests.rs` | Command session cancellation and process group signal tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/signal_handling_tests.rs> |
| `tests/stream_parser_tests.rs` | Stream parser protocol and fragmentation tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/stream_parser_tests.rs> |
| `tests/tool_operator_tests.rs` | Tool operator behavior/security tests for file and git actions. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/tool_operator_tests.rs> |

---

## Reference

- [AGENTS.md](AGENTS.md) — bootstrap dependency map for the private dispatcher skill tree
- [ADR index](adr/ADR-README.md) — architectural decisions and their rationale
