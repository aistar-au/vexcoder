# Task PL-01: Pre/Post-Tool-Call Hooks

**Target Files:** `src/config.rs`, `src/runtime/command_runner.rs`,
`src/tools/operator.rs`, `tests/integration_test.rs`

**ADR:** ADR-024 Gap 26

**Depends on:** PA-01 (layered config resolution chain — green at PR #48)

---

## Issue

No mechanism exists for operators to fire shell commands before or after
specific tool calls. ADR-024 Gap 26 defines a `[[hooks]]` configuration table
in the user config layer that wraps each qualifying tool invocation with
operator-defined pre/post commands.

---

## Decision

### Configuration surface

`[[hooks]]` tables are permitted in the user config layer only
(`~/.config/vex/config.toml`). Repo-local `.vex/config.toml` specifying
`[[hooks]]` is a hard startup failure with a diagnostic naming the file
(same supply-chain rationale as `[[mcp_servers]]`).

```toml
# ~/.config/vex/config.toml — user config layer only

[[hooks]]
event   = "post_tool"     # "pre_tool" | "post_tool"
tool    = "apply_patch"   # tool name as registered in dispatch table
command = "cargo"
args    = ["fmt"]
on_fail = "warn"          # "warn" | "abort" | "ignore"

[[hooks]]
event   = "post_tool"
tool    = "write_file"
command = "prettier"
args    = ["--write", "{{path}}"]
on_fail = "warn"
```

Template substitution in `args`:
- `{{path}}` — primary file path of the tool invocation where available
- `{{tool}}` — tool name
- No other substitution sites are supported.

### Execution contract

- All hook commands route through `SandboxDriver::wrap`.
- Hooks require `Capability::RunCommand` approval. A hook skipped for missing
  approval must emit a warning and allow the turn to continue. It must never
  silently block the tool call.
- `on_fail = "warn"`: log warning, continue.
- `on_fail = "abort"`: abort the pending tool result and surface the error to
  the operator. Must not terminate the process — hook failure is a tool-level
  event, not a session-level event.
- `on_fail = "ignore"`: swallow the exit code silently.
- `pre_tool` fires before the tool call is dispatched.
- `post_tool` fires after the tool call returns its result.

---

## Constraints

- `[[hooks]]` must not be permitted in repo-local `.vex/config.toml`.
  Reject with a diagnostic at config load time. This is the same constraint
  as `[[mcp_servers]]` — both carry supply-chain risk.
- Hook commands must route through `SandboxDriver::wrap` and require
  `Capability::RunCommand` approval.
- `on_fail = "abort"` must not terminate the process.
- Do not implement hooks in `src/runtime/` boundary modules. Hook dispatch
  belongs in the tool execution path, not in the runtime orchestration layer.
- Do not implement `vex doctor` (PL-02), the session token counter (PL-03),
  or `vex export` (PL-04) in this task.

---

## Definition of Done

1. `[[hooks]]` in user config layer parses without error.
2. `[[hooks]]` in repo-local config is a hard startup failure.
3. `post_tool` hook fires after apply_patch returns its result.
4. `pre_tool` hook fires before tool dispatch.
5. `on_fail = "abort"` surfaces error without terminating the process.
6. `on_fail = "warn"` logs warning and continues.
7. Hook without `Capability::RunCommand` approval emits warning and skips.
8. `{{path}}` and `{{tool}}` substituted correctly in `args`.
9. `cargo test --all-targets` is green.

---

## Anchor Tests

`test_hook_post_apply_patch_runs_command`
`test_hook_pre_tool_runs_before_dispatch`
`test_hook_on_fail_abort_interrupts_turn`
`test_hook_on_fail_warn_continues`
`test_hook_requires_run_command_approval`
`test_hook_skipped_without_approval_emits_warning`
`test_hook_repo_local_config_rejected_at_load`

Primary verification anchor:

```rust
#[test]
fn test_hook_repo_local_config_rejected_at_load() {
    // Given a repo-local .vex/config.toml that contains [[hooks]],
    // Config::load_for_tests must return Err with a diagnostic
    // naming the offending file path.
}
```

---

## Dispatch Verification (dispatch only — implementation not yet landed)

### [PL-01] - Pre/post-tool-call hooks

- Dispatcher: `dispatcher/adr-024-batch-b`
- Commit: (fill in at dispatch)
- Files changed:
  - `TASKS/PL-01-pre-post-tool-hooks.md` (this file)
  - `TASKS/TASKS-DISPATCH-MAP.md` (Batch B added)
  - `TASKS/completed/REPO-RAW-URL-MAP.md` (map update)
- Validation:
  - `cargo test --all-targets` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Notes:
  - This branch stages the PL-01 dispatch manifest and map updates only.
  - Do not mark PL-01 green until the implementation branch lands and all
    anchor tests pass.

---

## Completion Verification (fill in when implementation lands)

### [PL-01] - Pre/post-tool-call hooks

- Dispatcher: `<branch-name>`
- Commit: `<sha>`
- Files changed:
  - `src/config.rs` (+`<n>` -`<n>`)
  - `src/runtime/command_runner.rs` (+`<n>` -`<n>`)
  - `src/tools/operator.rs` (+`<n>` -`<n>`)
  - `tests/integration_test.rs` (+`<n>` -`<n>`)
- Validation:
  - `cargo test test_hook_repo_local_config_rejected_at_load --all-targets` : pass
  - `cargo test test_hook_post_apply_patch_runs_command --all-targets` : pass
  - `cargo test --all-targets` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Notes:
  - Hooks system implemented per ADR-024 Gap 26.
  - Repo-local [[hooks]] rejected at config load time.
  - All hook commands routed through SandboxDriver and Capability gate.
