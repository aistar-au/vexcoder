# Task FEAT-20: Changed Files and Evidence Pane

**Target File:** `src/app.rs`, `src/ui/render.rs`

**ADR:** ADR-022 §Execution and Approval Model, §TUI Direction

**Depends on:** FEAT-19 (`test_task_layout_four_regions_render_without_panic` must be green);
CORE-16 (`test_approval_policy_read_file_auto_allows_without_prompt` must be green)

---

## Issue

The task-first UI shell from FEAT-19 renders placeholder rows and a static approval
prompt. ADR-022 requires that changed files remain persistently visible during an active
task, that command and tool evidence is rendered as structured rows with status markers
in the activity trail, and that the approval prompt in the input pane reflects the live
`ApprovalRequest` sourced from `ApprovalPolicy` (CORE-16). This task wires those live
data sources into the layout introduced by FEAT-19.

---

## Decision

1. Extend `TaskLayoutState` with `changed_files: Vec<String>` rendered in the header
   or a sub-row below the status line.
2. Extend `activity_rows` population to include structured evidence entries with status
   markers: `[ok]` for completed, `[?]` for awaiting approval, `[->]` for running,
   `[!]` for failed.
3. Separate live command output (streaming stdout/stderr) from diff previews in the
   output pane: command output scrolls; diff preview is a fixed block above the input.
4. `changed_files` list is sourced from `TaskState::changed_files` loaded via CORE-17.
5. Wire the live `ApprovalRequest` from `ApprovalPolicy` (CORE-16) into
   `TaskLayoutState::pending_approval` so the approval prompt introduced in FEAT-19
   reflects real capability requests rather than static placeholder text.

---

## Definition of Done

1. `changed_files` from `TaskState` are visible in the rendered header region.
2. Activity rows with each status marker render without overlap or truncation on 80×24.
3. Output pane distinguishes a command output row from a diff preview row.
4. A live `ApprovalRequest` from `ApprovalPolicy` populates `pending_approval` and
   appears in the input pane with the correct capability description.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_changed_files_and_live_approval_prompt_render`

```rust
#[test]
fn test_changed_files_and_live_approval_prompt_render() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = TaskLayoutState {
        task_id: "task-001".into(),
        status_line: "AwaitingApproval".into(),
        activity_rows: vec!["[?] ApplyPatch: src/main.rs".into()],
        output_rows: vec![],
        changed_files: vec!["src/main.rs".into()],
        pending_approval: Some("ApplyPatch: src/main.rs".into()),
    };
    terminal.draw(|f| render_task_layout(f, &state)).unwrap();
    let rendered = terminal.backend().buffer().clone();
    let flat: String = rendered.content().iter().map(|c| c.symbol()).collect();
    assert!(flat.contains("src/main.rs"), "changed file must appear in rendered output");
    assert!(flat.contains("ApplyPatch"), "approval prompt must appear in rendered output");
    assert!(flat.contains("[y/n/s]"), "approval choices must appear in rendered output");
}
```

**What NOT to do:**
- Do not modify `src/runtime/`, `src/tools/`, or `src/state/`.
- Do not add new `UiUpdate` variants in this task.
- Do not remove the output pane scroll behavior introduced in FEAT-19.

