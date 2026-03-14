# Task PE-01: Non-interactive Execution Mode (BatchMode) [COMPLETE]

**Target files:**
- `Makefile` — extend `check-boundary` coverage to `src/batch_mode.rs`
- `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` — PE-01 / PE-02 closeout evidence and tracked deferral notes
- `src/batch_mode.rs` — new file
- `src/bin/vex.rs` — add `vex exec` subcommand
- `src/lib.rs` — expose `batch_mode` module
- `src/runtime/task_state.rs` — add `MaxTurnsReached` batch status surface
- `tests/integration_test.rs` — anchor tests

**ADR:** ADR-024 Gap 2

**Parity items:** PE-01, PE-02

**Depends on:** PA-01 (merged)

---

## Issue

There is no non-interactive execution mode. Automation pipelines, editor
integrations, and CI jobs have no way to drive the agent headlessly. Reference
implementations expose a `vex exec`-equivalent subcommand that reads a task
prompt, runs the agent loop to completion, and writes structured JSONL or plain
text to stdout. This capability is gated on PA-01 being green because
`BatchMode` reads `--format`, `--max-turns`, and `--auto-approve` from the
command line, but config-layer resolution must already be stable for the
`ApprovalPolicy` defaults to behave correctly.

---

## Decision

1. Introduce `BatchMode` in `src/batch_mode.rs` implementing `RuntimeMode` and
   `BatchFrontend` implementing `FrontendAdapter<BatchMode>`. No `ratatui` or
   `crossterm` imports anywhere in the file or any module reachable only through
   `vex exec`.

2. Add a `vex exec` subcommand to `src/bin/vex.rs` with the following flags:
   - `--task <TEXT>` — task prompt (mutually exclusive with `--task-file`)
   - `--task-file <PATH>` — read task prompt from file
   - `--auto-approve <SCOPE>` — optional; accepted values: `once`, `task`
   - `--max-turns <N>` — stop after N turns; default: unlimited
   - `--output <PATH>` — write output to file instead of stdout
   - `--format <FORMAT>` — output format: `jsonl` (default) or `text`

3. Approval policy in `BatchMode`:
   - Without `--auto-approve`: interactive approval prompts return `deny`.
   - `--auto-approve once`: grants each capability at `ApprovalScope::Once`
     for the duration of the run.
   - `--auto-approve task`: grants each capability at `ApprovalScope::Session`
     for the duration of the run.
   - Policy file is still read; `--auto-approve` overrides the interactive
     denial path only.

4. Exit codes:
   - `0`: `TaskStatus::Completed` only.
   - Non-zero: `TaskStatus::Failed`, approval denied, or `MaxTurnsReached`.
     `MaxTurnsReached` is non-zero because the task was not completed; a CI
     pipeline must not treat it as success.

5. JSONL output (one object per turn, written after each turn completes):

   ```json
   {
     "turn": 1,
     "input": "...",
     "response": "...",
     "changed_files": ["src/foo.rs"],
     "command_history": [{"program": "cargo test", "exit_code": 0, "interrupted": false}]
   }
   ```

   A final summary object is appended after the loop exits:

   ```json
   {
     "summary": true,
     "status": "Completed",
     "task_id": "...",
     "total_turns": 2,
     "changed_files": ["src/foo.rs"]
   }
   ```

   Per ADR-024 Gap 28, a future `PL-03` extension adds a `tokens` object to
   each JSONL turn record. That field is not part of `PE-01`/`PE-02`
   completion.

6. Text output (`--format text`): plain assistant response text only, one turn
   concatenated after the next, separated by a blank line. No JSONL envelope.

7. The `check-boundary` Makefile target must be extended (or a separate check
   added) to assert `src/batch_mode.rs` does not import `ratatui` or
   `crossterm`. The existing check covers only `src/runtime/`; `BatchMode` lives
   outside that directory and must be covered explicitly.

---

## Definition of Done

- `cargo test --all-targets` is green.
- `make gate-fast` is green including `check-boundary` for `src/batch_mode.rs`.
- `vex exec --task "list Rust source files" --format jsonl` produces JSONL to
  stdout without starting a TUI.
- `grep -r ratatui src/batch_mode.rs` returns no matches.
- `grep -r crossterm src/batch_mode.rs` returns no matches.
- All anchor tests below pass.

---

## Anchor tests

```rust
// tests/integration_test.rs

#[tokio::test]
async fn test_batch_mode_exits_zero_on_completion() {
    let result = run_batch_mode("echo hello", 3).await.unwrap();
    assert_eq!(result.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_batch_mode_completes_on_final_allowed_turn() {
    let result = run_batch_mode_with_opts("keep going", BatchRunOpts { max_turns: Some(1), ..Default::default() }).await.unwrap();
    assert_eq!(result.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_batch_mode_marks_second_turn_attempt_as_max_turns_reached() {
    // max_turns = 1 allows the first turn and rejects the second turn attempt.
    let mut mode = BatchMode::new("test-task".to_string(), BatchRunOpts { max_turns: Some(1), ..Default::default() }, None, None);
    mode.current_turn = 1;
    mode.on_user_input("second turn".to_string(), &mut ctx);
    assert_eq!(mode.status, TaskStatus::MaxTurnsReached);
}

#[tokio::test]
async fn test_batch_mode_interactive_approval_denied_by_default() {
    // A turn that requires RunCommand approval with no --auto-approve must
    // receive a deny decision and record it without panicking.
    let result = run_batch_mode_with_opts(
        "run: ls",
        BatchRunOpts { auto_approve: None, ..Default::default() },
    )
    .await
    .unwrap();
    // The task may complete or fail; the important invariant is that no
    // interactive prompt was issued (no panic, no hang).
    assert!(matches!(
        result.status,
        TaskStatus::Completed | TaskStatus::Failed
    ));
}

#[tokio::test]
async fn test_batch_mode_auto_approve_once_grants_single_turn() {
    let result = run_batch_mode_with_opts(
        "run: echo approved",
        BatchRunOpts {
            auto_approve: Some(ApprovalScope::Once),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_batch_mode_jsonl_output_includes_required_fields() {
    let output = capture_batch_jsonl("echo hello", 3).await.unwrap();
    // Each turn line must have at minimum: turn, input, response, changed_files,
    // command_history.
    let first_turn: serde_json::Value =
        serde_json::from_str(output.lines().next().unwrap()).unwrap();
    assert!(first_turn.get("turn").is_some());
    assert!(first_turn.get("input").is_some());
    assert!(first_turn.get("response").is_some());
    assert!(first_turn.get("changed_files").is_some());
    assert!(first_turn.get("command_history").is_some());
}

#[tokio::test]
async fn test_batch_mode_text_format_outputs_plain_response() {
    let output = capture_batch_text("echo hello", 3).await.unwrap();
    // Text format must not contain JSON envelope characters at line start.
    assert!(!output.trim_start().starts_with('{'));
}
```

**What NOT to do:**

- Do not import `ratatui` or `crossterm` in `src/batch_mode.rs` or any path
  reachable only through `vex exec`.
- Do not modify `src/runtime/mode.rs`, `src/runtime/frontend.rs`, or
  `src/runtime/loop.rs` — these are the shared runtime contracts; `BatchMode`
  must implement them, not change them.
- Do not modify `TuiMode` or `app.rs` for this task.
- Do not add a new `RuntimeMode` or `FrontendAdapter` variant to the trait
  definitions; `BatchMode` is a new implementor of the existing traits.
- Do not treat `MaxTurnsReached` as a successful exit code; it is non-zero.
- Do not write `ratatui`/`crossterm` to `Cargo.toml` as unconditional
  dependencies for the `vex exec` path.

---

## Completion Verification

### [PE-01 / PE-02] - BatchMode + vex exec
- Dispatcher: `dispatcher/vexcoder-adr-024-pe-01-batch-mode`
- Commit: `d6e508b0e54d2d1c2411825e651e44611b389244`
- Files changed:
  - `Makefile` (+5 -3)
  - `src/batch_mode.rs` (+81 -5)
  - `tests/integration_test.rs` (+63 -0)
- Validation:
  - `cargo test test_batch_mode_jsonl_output_includes_required_fields --all-targets` : pass
  - `cargo test test_batch_mode_jsonl_output_includes_input_field --all-targets` : pass
  - `cargo test test_batch_mode_memory_clear_jsonl_records_input --all-targets` : pass
  - `cargo test --all-targets` : pass
  - `make gate-fast` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Notes:
  - PR `#54` delivered the base `BatchMode` and `vex exec` surface.
  - This closeout commit records submitted input in JSONL turn evidence,
    including locally handled batch-mode turns such as `/memory clear`.
  - `check-boundary` now covers `src/batch_mode.rs`, so the no-TUI dependency
    rule is enforced by the repo gate.
  - `test_batch_mode_jsonl_output_includes_required_fields` now requires an
    actual turn record and asserts the manifest fields `turn`, `input`,
    `response`, `changed_files`, and `command_history`, plus the final summary.
  - `AutoApproveScope::Once` and `Task` remain distinct CLI/API variants, but
    the current `ToolApprovalRequest` response path is still boolean. Scope-
    specific once-vs-task enforcement remains a tracked follow-up rather than a
    closed `PE-01` behavior claim.
  - `tokens` remain deferred to `PL-03` per ADR-024 Gap 28.
