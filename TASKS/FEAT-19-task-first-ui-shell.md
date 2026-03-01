# Task FEAT-19: Task-First UI Shell

**Target File:** `src/ui/layout.rs`, `src/app.rs`, `src/ui/render.rs`

**ADR:** ADR-022 Phase 6, ADR-018

**Depends on:** ADR-018 streaming/managed TUI baseline must be green; CORE-17
(`test_task_state_survives_atomic_write_and_reload` must be green)

---

## Issue

The current TUI is conversation-history-first: the dominant pane is a scrolling message
log. ADR-022 requires the layout to shift to four persistent task-execution regions:
header/status, activity/audit trail, output pane, and input pane. This phase builds
the shell structure; evidence population is FEAT-20.

---

## Decision

1. Update `src/ui/layout.rs` to split the frame into four vertical regions in the
   order: header (fixed 1–2 rows), activity trail (proportional), output pane
   (proportional), input pane (fixed 3–4 rows).
2. Add a `TaskLayoutState` struct to `src/app.rs` holding `task_id`, `status_line`,
   `activity_rows: Vec<String>`, and `output_rows: Vec<String>`.
3. Route `src/ui/render.rs` to render all four regions from `TaskLayoutState` when
   a task is active. Conversation history is folded into the activity trail.
4. The existing chat-only render path is retained as a fallback when no task is active.
5. Approval prompts and status text are visible in header and input pane without
   requiring a mode switch.

---

## Definition of Done

1. The four-region layout renders without panic on an 80×24 terminal frame.
2. `TaskLayoutState` with non-empty `activity_rows` renders all rows in the activity
   region.
3. Header shows task id and status string.
4. Input pane renders a prompt line.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_task_layout_four_regions_render_without_panic`

```rust
#[test]
fn test_task_layout_four_regions_render_without_panic() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = TaskLayoutState {
        task_id: "task-001".into(),
        status_line: "Running".into(),
        activity_rows: vec!["[ok] ReadFile: README.md".into()],
        output_rows: vec!["$ cargo test".into()],
    };
    terminal.draw(|f| render_task_layout(f, &state)).unwrap();
    // must not panic; all four regions must fit within 24 rows
}
```

**What NOT to do:**
- Do not remove the existing conversation render path entirely.
- Do not add approval prompt handling to the TUI in this task — that is FEAT-20.
- Do not modify `src/runtime/`, `src/tools/`, or `src/state/`.
- Do not introduce new runtime dispatch paths.
