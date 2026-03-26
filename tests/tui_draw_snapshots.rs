//! Snapshot tests for the ANSI draw engine.
//!
//! These tests capture the raw ANSI byte output of `TaskDraw::draw()` as
//! `insta` snapshots.  Any unintended visual regression — truncated menus,
//! broken borders, missing widgets — will surface as a snapshot diff in CI.
//!
//! Run `cargo insta review` after adding or updating these tests.

use insta::assert_snapshot;
use vexcoder::app::{OutputScrollAnchor, StepLifecycle, TaskLayoutState, TimelineEntry};
use vexcoder::ui::draw::TaskDraw;

// ── Helpers ────────────────────────────────────────────────────────

fn make_state(entries: Vec<TimelineEntry>, output: Vec<&str>) -> TaskLayoutState {
    TaskLayoutState {
        task_id: "snap-001".to_string(),
        status_line: "mode:streaming approval:none repo:vexcoder inst:AGENTS.md".to_string(),
        timeline_entries: entries,
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".to_string(),
        output_rows: output.into_iter().map(String::from).collect(),
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nsubmit: / commands  @ files  ! shell".to_string(),
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
    }
}

fn entry(label: &str, lifecycle: StepLifecycle) -> TimelineEntry {
    TimelineEntry {
        step_id: 0,
        lifecycle,
        label: label.to_string(),
        detail: String::new(),
        session_id: None,
    }
}

/// Render and strip ANSI control sequences, returning a plain-text
/// representation suitable for deterministic snapshot comparison.
fn render_plain(state: &TaskLayoutState, cols: u16, rows: u16) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut draw = TaskDraw::new();
    draw.draw(&mut buf, state, cols, rows);
    let stripped = strip_ansi_escapes::strip(&buf);
    String::from_utf8_lossy(&stripped).into_owned()
}

// ── Snapshot tests ─────────────────────────────────────────────────

#[test]
fn snapshot_empty_state_80x24() {
    let state = make_state(vec![], vec![]);
    let rendered = render_plain(&state, 80, 24);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_single_running_tool_80x24() {
    let state = make_state(
        vec![entry("read_file", StepLifecycle::Running)],
        vec!["> hello", "[waiting for response...]"],
    );
    let rendered = render_plain(&state, 80, 24);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_multiple_steps_80x24() {
    let state = make_state(
        vec![
            entry("read_file", StepLifecycle::Completed),
            entry("apply_patch", StepLifecycle::AwaitingApproval),
            entry("run_command", StepLifecycle::Running),
        ],
        vec![
            "> Fix the config loader",
            "Reading src/config.rs...",
            "Found 3 issues.",
            "Generating patch...",
        ],
    );
    let rendered = render_plain(&state, 80, 24);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_small_terminal_40x12() {
    let state = make_state(
        vec![entry("read_file", StepLifecycle::Completed)],
        vec!["> hello", "Done."],
    );
    let rendered = render_plain(&state, 40, 12);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_very_small_terminal_20x5() {
    let state = make_state(
        vec![entry("read_file", StepLifecycle::Running)],
        vec!["> hi"],
    );
    let rendered = render_plain(&state, 20, 5);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_with_changed_files_80x24() {
    let mut state = make_state(
        vec![entry("apply_patch", StepLifecycle::Completed)],
        vec!["> edit config", "Patched successfully."],
    );
    state.changed_files = vec!["src/config.rs".to_string(), "src/agents.rs".to_string()];
    let rendered = render_plain(&state, 80, 24);
    assert_snapshot!(rendered);
}

#[test]
fn snapshot_pending_approval_80x24() {
    let mut state = make_state(
        vec![entry("write_file", StepLifecycle::AwaitingApproval)],
        vec!["> create new module", "Preparing write..."],
    );
    state.pending_approval = Some("write_file → src/agents.rs".to_string());
    let rendered = render_plain(&state, 80, 24);
    assert_snapshot!(rendered);
}
