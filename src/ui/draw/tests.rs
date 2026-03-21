use super::transcript::{is_horizontal_rule, parse_numbered_list_item};
use super::*;
use crate::app::{StepLifecycle, TaskLayoutState, TimelineEntry};

fn make_state(entries: Vec<TimelineEntry>, output: Vec<&str>) -> TaskLayoutState {
    TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:streaming approval:none repo:vexcoder inst:AGENTS.md".into(),
        activity_rows: vec![],
        total_steps: entries.len(),
        timeline_entries: entries,
        selected_step: 0,
        output_title: "Transcript".into(),
        output_rows: output.into_iter().map(|s| s.to_string()).collect(),
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nsubmit: / commands  @ files  ! shell".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        changed_files: vec![],
        follow_mode: true,
    }
}

#[test]
fn first_draw_writes_full_screen() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![TimelineEntry {
            step_id: 1,
            lifecycle: StepLifecycle::Running,
            label: "read_file: running...".into(),
            detail: String::new(),
            session_id: None,
        }],
        vec!["line 1", "line 2"],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("\x1b["), "output must contain ANSI escapes");
    // Must contain output rows.
    assert!(output.contains("line 1"), "output row 1 must be drawn");
    assert!(output.contains("line 2"), "output row 2 must be drawn");
    assert!(output.contains("Prompt"), "composer label must be drawn");
    assert!(
        !output.contains("vexcoder") && !output.contains("AGENTS.md"),
        "fullscreen transcript should not reintroduce top header chrome"
    );
}

#[test]
fn incremental_draw_skips_unchanged_regions() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![TimelineEntry {
            step_id: 1,
            lifecycle: StepLifecycle::Completed,
            label: "read_file: ok".into(),
            detail: String::new(),
            session_id: None,
        }],
        vec!["output line"],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let first_len = buf.len();

    buf.clear();
    draw.draw(&mut buf, &state, 80, 24);
    let second_len = buf.len();

    assert!(
        second_len < first_len,
        "incremental draw ({second_len} bytes) must be smaller than full draw ({first_len} bytes)"
    );
}

#[test]
fn append_only_output_draws_new_lines() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();

    let state1 = make_state(vec![], vec!["line 1"]);
    draw.draw(&mut buf, &state1, 80, 24);

    buf.clear();
    let state2 = make_state(vec![], vec!["line 1", "line 2", "line 3"]);
    draw.draw(&mut buf, &state2, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("line 2"), "new line 2 must be drawn");
    assert!(output.contains("line 3"), "new line 3 must be drawn");
}

#[test]
fn transcript_scroll_offset_renders_older_rows_from_prompt_edge() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let mut state = make_state(vec![], vec![]);
    state.output_rows = (0..20).map(|i| format!("line-{i}")).collect();
    state.output_scroll_offset = 2;

    draw.draw(&mut buf, &state, 80, 12);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("line-15"),
        "older transcript rows must stay visible"
    );
    assert!(
        !output.contains("line-19"),
        "bottom-most rows must move out of view when scrolled upward"
    );
}

#[test]
fn zero_terminal_size_is_noop() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["text"]);
    draw.draw(&mut buf, &state, 0, 0);
    assert!(buf.is_empty(), "zero-size terminal must produce no output");
}

#[test]
fn resize_triggers_full_repaint() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["text"]);

    draw.draw(&mut buf, &state, 80, 24);
    let first_len = buf.len();

    buf.clear();
    draw.draw(&mut buf, &state, 120, 30);
    let resize_len = buf.len();

    assert!(
        resize_len >= first_len / 2,
        "resize must trigger a substantial repaint"
    );
}

#[test]
fn truncate_to_width_handles_multibyte() {
    let text = "hello";
    assert_eq!(truncate_to_width(text, 3), "hel");
    assert_eq!(truncate_to_width(text, 100), "hello");

    let wide = "界界a";
    assert_eq!(truncate_to_width(wide, 4), "界界");
    assert_eq!(truncate_to_width(wide, 5), "界界a");
}

#[test]
fn lifecycle_prefixes_have_no_trailing_spaces() {
    let lifecycles = [
        StepLifecycle::Completed,
        StepLifecycle::Failed,
        StepLifecycle::Running,
        StepLifecycle::AwaitingApproval,
        StepLifecycle::Approved,
        StepLifecycle::UserInput,
        StepLifecycle::CommandSession,
    ];
    for lc in &lifecycles {
        let prefix = lifecycle_prefix(lc);
        assert!(
            !prefix.ends_with(' '),
            "prefix for {lc:?} must not end with a space: got {prefix:?}"
        );
    }
}

#[test]
fn lifecycle_only_changes_redraw_activity() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();

    let running = make_state(
        vec![TimelineEntry {
            step_id: 1,
            lifecycle: StepLifecycle::Running,
            label: "tool: status".into(),
            detail: String::new(),
            session_id: None,
        }],
        vec![],
    );
    draw.draw(&mut buf, &running, 80, 24);

    buf.clear();
    let completed = make_state(
        vec![TimelineEntry {
            step_id: 1,
            lifecycle: StepLifecycle::Completed,
            label: "tool: status".into(),
            detail: String::new(),
            session_id: None,
        }],
        vec![],
    );
    draw.draw(&mut buf, &completed, 80, 24);

    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("task:test-001"),
        "lifecycle changes should still redraw the bottom status bar"
    );
    assert!(
        !output.contains("1 step done"),
        "lifecycle changes must not reintroduce the removed header summary"
    );
}

#[test]
fn changing_selected_inspector_entry_redraws_output() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();

    let first = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:streaming approval:none repo:vexcoder inst:none".into(),
        activity_rows: vec![],
        timeline_entries: vec![
            TimelineEntry {
                step_id: 1,
                lifecycle: StepLifecycle::Completed,
                label: "read_file: ok".into(),
                detail: "Tool: read_file\nOutcome: ok".into(),
                session_id: None,
            },
            TimelineEntry {
                step_id: 2,
                lifecycle: StepLifecycle::Completed,
                label: "check: ok".into(),
                detail: "Tool: check\nOutcome: ok".into(),
                session_id: None,
            },
        ],
        selected_step: 0,
        total_steps: 2,
        output_title: "Inspector".into(),
        output_rows: vec!["Tool: read_file".into(), "Outcome: ok".into()],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Top,
        pending_approval: None,
        input_hint: "Prompt\nUse submit-time `/` commands, submit-time `@path` inlining, paste large blocks, and Shift+Enter for a newline.".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        changed_files: vec![],
        follow_mode: true,
    };
    draw.draw(&mut buf, &first, 80, 24);

    buf.clear();
    let second = TaskLayoutState {
        selected_step: 1,
        output_rows: vec!["Tool: check".into(), "Outcome: ok".into()],
        ..first
    };
    draw.draw(&mut buf, &second, 80, 24);

    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("Tool: check"),
        "selection changes must redraw transcript"
    );
}

#[test]
fn activity_rows_do_not_render_in_fullscreen_transcript_surface() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:streaming approval:none repo:vexcoder inst:none".into(),
        activity_rows: vec!["[->] validate: running...".into(), "> ship it".into()],
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec!["line 1".into()],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nUse submit-time `/` commands, submit-time `@path` inlining, paste large blocks, and Shift+Enter for a newline.".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        changed_files: vec![],
        follow_mode: true,
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("line 1"));
    assert!(!output.contains("validate: running"));
    assert!(!output.contains("ship it"));
}

#[test]
fn enriched_paragraph_output_renders_paragraph_markers() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec![
            "[tool] read_file · src/main.rs · completed",
            "[detail] Scope: Read file content",
            "[detail] Command: read_file",
            "[detail] Result: 42 lines read from src/main.rs",
            "[evidence] Outcome: 42 lines read from src/main.rs",
            "",
            "[tool] write_file · src/lib.rs · failed",
            "[detail] Scope: Write file content",
            "[detail] Command: write_file",
            "[detail] Result: permission denied writing to src/lib.rs",
            "[evidence] Outcome: permission denied writing to src/lib.rs",
            "",
            "The file was read successfully.",
        ],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("\u{2726}"),
        "tool paragraph marker must be drawn in transcript"
    );
    assert!(
        output.contains("read_file"),
        "tool name must appear in output"
    );
    assert!(
        output.contains("src/main.rs"),
        "summary target must appear on the status line"
    );
    assert!(
        output.contains("Scope:") && output.contains("Read file content"),
        "detail label and value must appear in output"
    );
    assert!(
        output.contains("\x1b[38;5;2mcompleted"),
        "completed status should use the success accent color"
    );
    assert!(
        output.contains("\x1b[38;5;1mfailed"),
        "failed status should use the error accent color"
    );
}

#[test]
fn six_space_evidence_renders_dimmer_than_four_space() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec![
            "[tool] bash · exit code 0 · completed",
            "[detail] Scope: Tool invocation recorded in the completed turn.",
            "[detail] Command: bash",
            "[detail] Result: exit code 0",
            "[evidence] Outcome: exit code 0",
            "[evidence] stdout line 1",
        ],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("bash"), "tool name must appear in summary");
    assert!(
        output.contains("Result:") && output.contains("exit code 0"),
        "4-space phase detail must be drawn"
    );
    assert!(
        output.contains("stdout line 1"),
        "6-space evidence must be drawn"
    );
    // The 6-space evidence uses DIM_GRAY (240) rather than GRAY (245).
    // Both set_dim + set_fg(DIM_GRAY) for evidence vs set_dim + set_fg(GRAY) for detail.
    // Count the DIM_GRAY (240) color codes — evidence lines add extra instances.
    let dim_gray_count = output.matches("\x1b[38;5;240m").count();
    assert!(
        dim_gray_count >= 2,
        "6-space evidence must use DIM_GRAY color: found {dim_gray_count} instances"
    );
}

#[test]
fn paragraph_tree_summary_prefers_target_hint() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec![
            "[tool] read_file · 42 lines read from src/main.rs · completed",
            "[detail] Scope: Read file content",
            "[detail] Command: read_file",
            "[detail] Result: 42 lines read from src/main.rs",
            "[evidence] Outcome: 42 lines read from src/main.rs",
            "[evidence] evidence detail line",
        ],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("read_file"),
        "tool name must appear in summary"
    );
    assert!(
        output.contains("src/main.rs"),
        "target hint must appear in summary"
    );
    assert!(
        output.contains("Result:") && output.contains("42 lines read from src/main.rs"),
        "phase detail must appear"
    );
    assert!(
        output.contains("evidence detail"),
        "evidence text must appear"
    );
}

#[test]
fn paragraph_block_uses_four_to_six_lines_per_tool() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec![
            "[tool] read_file · ok · completed",
            "[detail] Scope: Read file content",
            "[detail] Command: read_file",
            "[detail] Result: ok",
            "[evidence] Outcome: ok",
        ],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("\u{2726}"),
        "tool summary marker must render"
    );
    assert!(output.contains("Scope"), "detail rows must render");
    assert!(output.contains("\u{2727}"), "evidence marker must render");
}

#[test]
fn tool_summary_styles_awaiting_approval_status() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[tool] read_file · awaiting approval"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("read_file"), "tool name must render");
    assert!(
        output.contains("\x1b[38;5;3mawaiting approval"),
        "awaiting approval status must use yellow accent: {output}"
    );
}

#[test]
fn labeled_diff_evidence_keeps_field_prefix_and_styles_diff() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[evidence] Outcome: +fn main() {"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("Outcome: "), "field prefix must render");
    assert!(
        output.contains("\x1b[38;5;2m+fn main() {"),
        "labeled diff evidence must keep green addition styling: {output}"
    );
}

#[test]
fn labeled_json_evidence_preserves_json_number_styling() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["[evidence] Outcome: {\"path\":\"src/main.rs\",\"count\":2}"],
    );

    draw.draw(&mut buf, &state, 100, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("Outcome: "), "field prefix must render");
    assert!(
        output.contains("\x1b[38;5;3m2"),
        "labeled json evidence must keep numeric styling: {output}"
    );
}

#[test]
fn command_session_start_includes_running_status_and_pid() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[command session started pid=42] cargo test"]);

    draw.draw(&mut buf, &state, 100, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("command session"),
        "session summary must render"
    );
    assert!(output.contains("pid 42"), "pid detail must render");
    assert!(
        output.contains("\x1b[38;5;6mrunning"),
        "command session summary must keep the running status accent: {output}"
    );
}

#[test]
fn error_header_uses_error_body_color() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[error] permission denied writing src/lib.rs"]);

    draw.draw(&mut buf, &state, 100, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("\x1b[38;5;1mpermission denied writing src/lib.rs"),
        "error header body must use the error color: {output}"
    );
}

#[test]
fn streaming_cursor_uses_live_cursor_accent() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["streaming line▌"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("streaming line"),
        "streaming text must render"
    );
    assert!(
        output.contains("\x1b[1m\x1b[38;5;6m\u{258c}"),
        "streaming cursor must use the live cyan cursor accent: {output}"
    );
}

#[test]
fn persistent_layout_starts_with_blank_transcript_before_first_turn() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        activity_rows: vec![],
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nsubmit: / commands  @ files  ! shell".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        changed_files: vec![],
        follow_mode: true,
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("Prompt"),
        "composer label must be drawn on first frame"
    );
    assert!(
        !output.contains("Type a prompt"),
        "fullscreen transcript should start blank before the first turn"
    );
}

#[test]
fn adaptive_layout_assigns_full_body_to_transcript() {
    let entries: Vec<TimelineEntry> = (0..20)
        .map(|i| TimelineEntry {
            step_id: i as u64,
            lifecycle: StepLifecycle::Completed,
            label: format!("step_{i}: done"),
            detail: String::new(),
            session_id: None,
        })
        .collect();

    let regions = Regions::compute(80, 40, false, entries.len());
    assert_eq!(regions.timeline_rows, 0);
    assert_eq!(regions.transcript_start, 0);
    assert!(regions.transcript_rows > 0);
}

#[test]
fn adaptive_composer_scales_with_terminal_height() {
    let small = Regions::compute(80, 12, false, 0);
    assert_eq!(
        small.composer_rows, 3,
        "small terminal keeps the prompt surface fixed to three rows"
    );

    let medium = Regions::compute(80, 20, false, 0);
    assert_eq!(
        medium.composer_rows, 3,
        "medium terminal keeps the prompt surface fixed to three rows"
    );

    let large = Regions::compute(80, 40, false, 0);
    assert_eq!(
        large.composer_rows, 3,
        "large terminal keeps the prompt surface fixed to three rows"
    );
}

#[test]
fn fullscreen_surface_hides_top_header_chrome() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:streaming approval:none repo:myrepo inst:AGENTS.md".into(),
        activity_rows: vec![],
        timeline_entries: vec![TimelineEntry {
            step_id: 1,
            lifecycle: StepLifecycle::Running,
            label: "read_file: running".into(),
            detail: String::new(),
            session_id: None,
        }],
        selected_step: 0,
        total_steps: 1,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nsubmit: / commands  @ files  ! shell".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        changed_files: vec!["src/main.rs".into()],
        follow_mode: true,
    };

    draw.draw(&mut buf, &state, 100, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        !output.contains("myrepo") && !output.contains("AGENTS.md"),
        "repo and instruction labels must not appear above the transcript"
    );
    assert!(output.contains("Prompt"), "composer should remain visible");
}

#[test]
fn inline_approval_renders_in_composer() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:overlay approval:pending repo:vexcoder inst:none".into(),
        activity_rows: vec![],
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: Some("write_file: src/main.rs".into()),
        input_hint: "Approval\n[y/n/s]".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        changed_files: vec![],
        follow_mode: true,
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("write_file"),
        "approval context must show in composer"
    );
    assert!(
        output.contains("approve") || output.contains("deny"),
        "approval choices must be visible"
    );
}

#[test]
fn status_parts_parsing() {
    let parts = parse_status_parts(
        "mode:streaming approval:none history:11 repo:vexcoder inst:AGENTS.md tokens:0",
    );
    assert_eq!(parts.mode, "streaming");
    assert_eq!(parts.repo, "vexcoder");
    assert_eq!(parts.inst, "AGENTS.md");
    assert_eq!(parts.tokens, 0);
}

#[test]
fn status_parts_parsing_with_tokens() {
    let parts =
        parse_status_parts("mode:ready approval:none history:3 repo:myrepo inst:none tokens:4800");
    assert_eq!(parts.tokens, 4800);
    assert_eq!(parts.repo, "myrepo");
    assert_eq!(parts.mode, "ready");
}

#[test]
fn fullscreen_surface_hides_token_indicator_when_tokens_recorded() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        // tokens:2500 — just over 2k so the label rounds to "~2.5k ctx"
        status_line: "mode:ready approval:none history:2 repo:vexcoder inst:none tokens:2500"
            .into(),
        activity_rows: vec![],
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nsubmit: / commands  @ files  ! shell".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        changed_files: vec![],
        follow_mode: true,
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        !output.contains("ctx"),
        "top header chrome should stay hidden: {output:?}"
    );
    assert!(
        !output.contains("2.5"),
        "top header chrome should stay hidden: {output:?}"
    );
}

#[test]
fn header_hides_token_indicator_when_no_turns_completed() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        // tokens:0 — no turns completed yet
        status_line: "mode:ready approval:none history:0 repo:vexcoder inst:none tokens:0"
            .into(),
        activity_rows: vec![],
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nUse submit-time `/` commands, submit-time `@path` inlining, paste large blocks, and Shift+Enter for a newline.".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        changed_files: vec![],
        follow_mode: true,
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        !output.contains("ctx"),
        "header must not show token indicator before any turns: got {output:?}"
    );
}

#[test]
fn composer_renders_live_input_text() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        activity_rows: vec![],
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nUse submit-time `/` commands, submit-time `@path` inlining, paste large blocks, and Shift+Enter for a newline.".into(),
        composer_text: "hello fullscreen".into(),
        composer_cursor: "hello fullscreen".len(),
        changed_files: vec![],
        follow_mode: true,
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("hello fullscreen"),
        "composer must render the live editor buffer"
    );
}

#[test]
fn composer_hint_renders_once_without_repeating_down_the_prompt() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec![]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    let hint = "/ commands  @ files  ! shell";

    assert_eq!(
        output.matches(hint).count(),
        1,
        "composer helper hint must render once"
    );
}

#[test]
fn short_bottom_anchored_transcript_hugs_prompt_edge() {
    let state = make_state(vec![], vec!["Type a prompt below to begin."]);
    let regions = Regions::compute(80, 24, false, 0);
    let (visible_start, visible_end) = transcript_window(&state, regions.transcript_rows as usize);
    let render_start = transcript_render_start_row(&state, &regions, visible_start, visible_end);

    assert_eq!(
        render_start,
        regions.composer_start.saturating_sub(1),
        "short transcript should render against the prompt edge"
    );
}

#[test]
fn composer_hash_tracks_live_input_changes() {
    let draw = TaskDraw::new();
    let first = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        activity_rows: vec![],
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nUse submit-time `/` commands, submit-time `@path` inlining, paste large blocks, and Shift+Enter for a newline.".into(),
        composer_text: "first".into(),
        composer_cursor: 5,
        changed_files: vec![],
        follow_mode: true,
    };
    let second = TaskLayoutState {
        composer_text: "second".into(),
        composer_cursor: 6,
        ..first.clone()
    };

    assert_ne!(
        draw.compute_composer_hash(&first),
        draw.compute_composer_hash(&second),
        "composer hash must change when the live input buffer changes"
    );
}

#[test]
fn composer_hash_tracks_cursor_only_changes() {
    let draw = TaskDraw::new();
    let first = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        activity_rows: vec![],
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nUse submit-time `/` commands, submit-time `@path` inlining, paste large blocks, and Shift+Enter for a newline.".into(),
        composer_text: "same text".into(),
        composer_cursor: 2,
        changed_files: vec![],
        follow_mode: true,
    };
    let second = TaskLayoutState {
        composer_cursor: 7,
        ..first.clone()
    };

    assert_ne!(
        draw.compute_composer_hash(&first),
        draw.compute_composer_hash(&second),
        "composer hash must change when the live cursor moves"
    );
}

#[test]
fn numbered_list_items_are_parsed() {
    let result = parse_numbered_list_item("1. First item");
    assert!(result.is_some(), "must parse '1. First item'");
    let (prefix, rest, _) = result.unwrap();
    assert_eq!(prefix, "1. ");
    assert_eq!(rest, "First item");

    let result = parse_numbered_list_item("42. Large number");
    assert!(result.is_some(), "must parse '42. Large number'");
    let (prefix, rest, _) = result.unwrap();
    assert_eq!(prefix, "42. ");
    assert_eq!(rest, "Large number");

    assert!(
        parse_numbered_list_item("not a list").is_none(),
        "must not parse plain text"
    );
    assert!(
        parse_numbered_list_item("1.no space").is_none(),
        "must not parse without space after dot"
    );
}

#[test]
fn horizontal_rules_are_detected() {
    assert!(is_horizontal_rule("---"), "three dashes");
    assert!(is_horizontal_rule("***"), "three asterisks");
    assert!(is_horizontal_rule("___"), "three underscores");
    assert!(is_horizontal_rule("- - -"), "spaced dashes");
    assert!(is_horizontal_rule("-----"), "many dashes");
    assert!(!is_horizontal_rule("--"), "too few");
    assert!(!is_horizontal_rule("abc"), "not a rule");
    assert!(!is_horizontal_rule("---a"), "mixed characters");
}

#[test]
fn checklist_items_render_with_markers() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec![
            "- [x] Completed task",
            "- [ ] Pending task",
            "- Regular bullet",
        ],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("\u{2611}"),
        "checked item must show ballot box with check: {output}"
    );
    assert!(
        output.contains("\u{2610}"),
        "unchecked item must show empty ballot box: {output}"
    );
    assert!(
        output.contains("Completed task"),
        "checked task text must appear"
    );
    assert!(
        output.contains("Pending task"),
        "unchecked task text must appear"
    );
}

#[test]
fn numbered_list_renders_in_transcript() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["1. First step", "2. Second step"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("1. "), "number prefix must be drawn");
    assert!(
        output.contains("First step"),
        "list item text must be drawn"
    );
}

#[test]
fn horizontal_rule_renders_in_transcript() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["Some text", "---", "More text"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    // Horizontal rule draws thin rule characters.
    assert!(
        output.contains("\u{2500}"),
        "horizontal rule must draw line characters"
    );
    assert!(output.contains("Some text"), "text before rule must appear");
    assert!(output.contains("More text"), "text after rule must appear");
}

#[test]
fn progress_indicator_shown_for_running_tasks() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![TimelineEntry {
            step_id: 1,
            lifecycle: StepLifecycle::Running,
            label: "build: running...".into(),
            detail: String::new(),
            session_id: None,
        }],
        vec![],
    );

    draw.draw(&mut buf, &state, 100, 24);
    let output = String::from_utf8_lossy(&buf);

    // Must contain at least one block-drawing character from the progress
    // indicator animation.
    let has_progress_char =
        output.contains('\u{2591}') || output.contains('\u{2593}') || output.contains('\u{2588}');
    assert!(
        !has_progress_char,
        "running tasks must not reintroduce the removed top progress banner"
    );
    assert!(
        !output.contains("1 active"),
        "running tasks must not reintroduce the removed top activity count"
    );
}

#[test]
fn scroll_indicator_drawn_when_content_exceeds_viewport() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let mut state = make_state(vec![], vec![]);
    // Create more output rows than will fit in the viewport.
    state.output_rows = (0..50).map(|i| format!("long-output-{i}")).collect();

    draw.draw(&mut buf, &state, 80, 16);
    let output = String::from_utf8_lossy(&buf);

    // Scroll indicator uses ░ (track) and █ (thumb) on the right edge.
    let has_track = output.contains('\u{2591}');
    let has_thumb = output.contains('\u{2588}');
    assert!(
        has_track || has_thumb,
        "scroll indicator must appear when content exceeds viewport"
    );
}

#[test]
fn inline_italic_renders_with_italic_escape() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["This has *italic* text"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    // Italic text uses CSI 3m.
    assert!(
        output.contains("\x1b[3m"),
        "italic text must use ANSI italic escape: {output}"
    );
    assert!(output.contains("italic"), "italic text must appear");
}

#[test]
fn inline_strikethrough_renders_as_dim() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["This has ~~struck~~ text"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("struck"), "struck text must appear");
}

// ── Paragraph-style disclosure tests ────────────────────────────

#[test]
fn tool_paragraph_header_renders_with_cosmic_marker() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[tool] read_file src/main.rs"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("\u{2726}"),
        "tool paragraph header must show ✦ cosmic marker: {output}"
    );
    assert!(
        output.contains("read_file"),
        "tool name must appear in paragraph header"
    );
}

#[test]
fn tool_detail_renders_at_four_space_indent() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[detail] Status: completed, 42 lines"]);

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("Status"),
        "detail line must render: {output}"
    );
}

#[test]
fn tool_evidence_renders_at_six_space_indent_with_accent() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["[evidence] fn main() { println!(\"hello\"); }"],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("\u{2727}"),
        "evidence line must show ✧ accent marker: {output}"
    );
    assert!(output.contains("fn main"), "evidence content must appear");
}

#[test]
fn paragraph_block_disclosure_levels_render_as_tree() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec![
            "[tool] read_file src/main.rs",
            "[detail] Status: completed, 42 lines",
            "[detail] Path: src/main.rs",
            "[evidence] fn main() {",
            "[evidence]     println!(\"hello\");",
            "[evidence] }",
            "",
            "[ok] read_file completed",
        ],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    // All three disclosure levels must be present.
    assert!(
        output.contains("\u{2726}"),
        "2-space tool header must have ✦ marker"
    );
    assert!(
        output.contains("Status"),
        "4-space detail lines must be present"
    );
    assert!(
        output.contains("\u{2727}"),
        "6-space evidence must have ✧ marker"
    );
    // Completion marker must also render.
    assert!(
        output.contains("\u{2605}"),
        "completed tool must show ★ star marker"
    );
}

#[test]
fn six_space_raw_indent_differentiates_from_four_space() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["    four-space detail", "      six-space evidence"],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("four-space detail"),
        "4-space indent must render"
    );
    assert!(
        output.contains("six-space evidence"),
        "6-space indent must render"
    );
    assert!(
        output.contains("\x1b[2m\x1b[38;5;245m    four-space detail"),
        "4-space detail must render dimmed in GRAY"
    );
    assert!(
        output.contains("\x1b[2m\x1b[38;5;240m      six-space evidence"),
        "6-space raw indent must render dimmed in DIM_GRAY"
    );
    let four_idx = output.find("four-space").unwrap();
    let six_idx = output.find("six-space").unwrap();
    // 6-space evidence text must appear at a different position confirming
    // it was rendered through a separate code path.
    assert_ne!(
        four_idx, six_idx,
        "indent levels must be rendered separately"
    );
}
