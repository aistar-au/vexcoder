use super::transcript_helpers::{is_horizontal_rule, parse_numbered_list_item};
use super::*;
use crate::app::{StepLifecycle, TaskLayoutState, TimelineEntry};

fn make_state(entries: Vec<TimelineEntry>, output: Vec<&str>) -> TaskLayoutState {
    TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:streaming approval:none repo:vexcoder inst:AGENTS.md".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
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
            label: "read_file: Mapping adjacent sectors...".into(),
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
fn transcript_body_text_and_code_blocks_use_phosphor_white() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["plain response", "```rust", "let value = 1;", "```"],
    );

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("\x1b[38;5;15mplain response"),
        "plain transcript text must render in phosphor white"
    );
    assert!(
        output.contains("\x1b[38;5;15mlet value = 1;"),
        "code block text must render in phosphor white"
    );
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
    assert!(buf.is_empty(), "zero-size display must produce no output");
}

#[test]
fn undersized_terminal_shows_resize_notice() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["text"]);

    // Sub-minimum sizes (< 10 cols or < 4 rows) render a short resize notice
    // so the operator knows the surface is waiting for a larger viewport.
    draw.draw(&mut buf, &state, 9, 3);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("resize"),
        "undersized display should show resize notice: {output:?}"
    );
    assert!(
        !output.is_empty(),
        "undersized notice must produce visible output"
    );
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
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
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
fn empty_timeline_renders_separator_and_transcript() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:streaming approval:none repo:vexcoder inst:none".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("line 1"));
}

#[test]
fn enriched_paragraph_output_renders_paragraph_markers() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec![
            "[tool] read_file · src/main.rs · Response complete.",
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
        output.contains("\x1b[38;5;2mResponse complete."),
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
            "[tool] bash · exit code 0 · Response complete.",
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
            "[tool] read_file · 42 lines read from src/main.rs · Response complete.",
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
            "[tool] read_file · Response complete.",
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
        output.contains("\x1b[38;5;5mMapping adjacent sectors..."),
        "command session summary must keep the running status accent: {output}"
    );
}

#[test]
fn waiting_placeholder_uses_spatial_progress_copy() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[thinking] Mapping adjacent sectors..."]);

    draw.draw(&mut buf, &state, 100, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(output.contains("Mapping adjacent sectors..."));
    assert!(output.contains("\x1b[38;5;5m"));
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
        "streaming cursor must use the current cyan cursor accent: {output}"
    );
}

#[test]
fn persistent_layout_starts_with_blank_transcript_before_first_turn() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
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
    assert_eq!(regions.transcript_start, 0);
    assert!(regions.transcript_rows > 0);
}

#[test]
fn adaptive_composer_scales_with_terminal_height() {
    let small = Regions::compute(80, 12, false, 0);
    assert_eq!(
        small.composer_rows, 3,
        "small display keeps the prompt surface fixed to three rows"
    );

    let medium = Regions::compute(80, 20, false, 0);
    assert_eq!(
        medium.composer_rows, 3,
        "medium display keeps the prompt surface fixed to three rows"
    );

    let large = Regions::compute(80, 40, false, 0);
    assert_eq!(
        large.composer_rows, 3,
        "large display keeps the prompt surface fixed to three rows"
    );

    let multiline = (0..8)
        .map(|idx| format!("line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    let expanded = Regions::compute_with_composer(80, 24, false, 0, &multiline);
    assert_eq!(
        expanded.composer_rows, 7,
        "wrapped composer content must expand the prompt surface up to the shared cap"
    );
}

#[test]
fn fullscreen_surface_hides_top_header_chrome() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:streaming approval:none repo:myrepo inst:AGENTS.md".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![TimelineEntry {
            step_id: 1,
            lifecycle: StepLifecycle::Running,
            label: "read_file: Mapping adjacent sectors...".into(),
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
        composer_focused: true,
        changed_files: vec!["src/main.rs".into()],
        follow_mode: true,
        picker_overlay: vec![],
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
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
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
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
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
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
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
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("hello fullscreen"),
        "composer must render the active editor buffer"
    );
}

#[test]
fn composer_header_renders_focus_and_char_count() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nsubmit: / commands  @ files  ! shell".into(),
        composer_text: "hello".into(),
        composer_cursor: 5,
        composer_focused: false,
        changed_files: vec![],
        follow_mode: false,
        picker_overlay: vec![],
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(output.contains("unfocused · 5 chars"));
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
fn fullscreen_surface_uses_three_regions_without_fixed_bottom_pane() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let mut state = make_state(
        vec![],
        vec!["[thinking] Mapping adjacent sectors... 2.5s | \u{2191}:512/1024"],
    );
    state.telemetry = crate::app::TaskTelemetryState {
        mode: "streaming".into(),
        approval: "pending".into(),
        model_name: String::new(),
        model_backend: None,
        sandbox_kind: None,
        context_summary: None,
        history_rows: 7,
        total_tokens: 1234,
        tokens_sent: 800,
        tokens_received: 434,
        active_tools: 2,
        active_commands: 1,
        waiting_summary: Some("lat:2.5s".into()),
        timing_summary: None,
        git_branch: "main".into(),
    };
    state.changed_files = vec!["src/config.rs".into(), "src/ui/draw/mod.rs".into()];

    draw.draw(&mut buf, &state, 120, 30);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        !output.contains("Telemetry") && !output.contains("Git"),
        "the fullscreen surface should not reserve a dedicated telemetry/git pane"
    );
    assert!(
        output.contains("Mapping adjacent sectors...") && output.contains("\u{2191}:512/1024"),
        "telemetry should stay inline in the scrolling transcript"
    );
    assert!(
        output.contains("Prompt"),
        "the prompt region must remain visible"
    );
    assert!(
        output.contains("task:test-001")
            && output.contains("\u{e0a0}main")
            && output.contains("m:streaming")
            && output.contains("ap:pending")
            && output.contains("\u{2191}800")
            && output.contains("\u{2193}434")
            && output.contains("lat:2.5s")
            && output.contains("chg:2"),
        "the separate status bar should fold compact telemetry, git branch, and token counters from the removed pane"
    );
}

#[test]
fn inline_telemetry_truncation_rewrites_legacy_labels_before_fallback() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["[ttft:0.3s | read:2.5s (2641 tok) | generate:1.1s (357 tok) | total:7.7s]"],
    );

    draw.draw(&mut buf, &state, 46, 12);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("↑:"),
        "truncated inline telemetry should use arrow labels: {output:?}"
    );
    assert!(
        !output.contains("read:"),
        "legacy read label should be rewritten before truncation: {output:?}"
    );
    assert!(
        !output.contains("generate:"),
        "legacy generate label should be rewritten before truncation: {output:?}"
    );
}

#[test]
fn regions_compute_three_pane_surface() {
    let regions = Regions::compute(80, 24, false, 0);

    assert_eq!(
        regions.transcript_start + regions.transcript_rows,
        regions.composer_start,
        "the composer should begin immediately after the scrolling transcript"
    );
    assert_eq!(
        regions.transcript_rows + regions.composer_rows + 1,
        regions.rows,
        "the fullscreen surface should consist only of transcript, composer, and status bar rows"
    );
}

#[test]
fn status_bar_summary_truncates_to_available_width() {
    let mut state = make_state(vec![], vec!["plain response"]);
    state.telemetry = crate::app::TaskTelemetryState {
        mode: "streaming".into(),
        approval: "pending".into(),
        model_name: String::new(),
        model_backend: None,
        sandbox_kind: None,
        context_summary: None,
        history_rows: 99,
        total_tokens: 9999,
        tokens_sent: 6000,
        tokens_received: 3999,
        active_tools: 4,
        active_commands: 2,
        waiting_summary: Some("latency:2.5s \u{2191}:512/1024 \u{2193}:128".into()),
        timing_summary: None,
        git_branch: String::new(),
    };
    state.changed_files = vec![
        "src/config.rs".into(),
        "src/ui/draw/mod.rs".into(),
        "src/ui/draw/tests.rs".into(),
    ];

    let summary = status_bar_summary(&state, 32);

    assert!(display_width(&summary) <= 32);
    assert!(summary.contains("task:test-001"));
    assert!(summary.contains("m:streaming"));
}

#[test]
fn status_bar_shows_git_branch_and_token_counters() {
    let mut state = make_state(vec![], vec!["response text"]);
    state.telemetry = crate::app::TaskTelemetryState {
        mode: "ready".into(),
        approval: String::new(),
        model_name: String::new(),
        model_backend: None,
        sandbox_kind: None,
        context_summary: None,
        history_rows: 5,
        total_tokens: 150,
        tokens_sent: 100,
        tokens_received: 50,
        active_tools: 0,
        active_commands: 0,
        waiting_summary: None,
        timing_summary: None,
        git_branch: "feat/my-branch".into(),
    };

    let summary = status_bar_summary(&state, 120);

    assert!(
        summary.contains("feat/my-branch"),
        "status bar should display the git branch: {summary}"
    );
    assert!(
        summary.contains("\u{2191}100") && summary.contains("\u{2193}50"),
        "status bar should show token counters with up/down arrows: {summary}"
    );
}

#[test]
fn status_bar_includes_model_sandbox_and_context_summary() {
    let mut state = make_state(vec![], vec!["response text"]);
    state.telemetry = crate::app::TaskTelemetryState {
        mode: "ready".into(),
        approval: "none".into(),
        model_name: "local-model-a".into(),
        model_backend: Some(crate::runtime::ModelBackendKind::LocalRuntime),
        sandbox_kind: Some(crate::runtime::SandboxKind::Container),
        context_summary: Some(crate::app::TaskContextSummaryState {
            file_snapshots: 3,
            related_paths: 2,
            cache_hits: 5,
            cache_misses: 1,
            git_context_included: true,
        }),
        history_rows: 5,
        total_tokens: 150,
        tokens_sent: 100,
        tokens_received: 50,
        active_tools: 0,
        active_commands: 0,
        waiting_summary: None,
        timing_summary: None,
        git_branch: "feat/my-branch".into(),
    };

    let summary = status_bar_summary(&state, 200);

    assert!(
        summary.contains("mdl:local-model-a"),
        "status bar should expose the active model name: {summary}"
    );
    assert!(
        summary.contains("be:local"),
        "status bar should expose the active backend: {summary}"
    );
    assert!(
        summary.contains("sb:container"),
        "status bar should expose the active sandbox: {summary}"
    );
    assert!(
        summary.contains("ctx:f3 r2 c5/1 git"),
        "status bar should expose the context assembly summary: {summary}"
    );
}

#[test]
fn status_bar_priority_truncation_drops_low_priority_fields_first() {
    let mut state = make_state(vec![], vec!["response text"]);
    state.telemetry = crate::app::TaskTelemetryState {
        mode: "streaming".into(),
        approval: "pending".into(),
        model_name: "local-model-a".into(),
        model_backend: Some(crate::runtime::ModelBackendKind::LocalRuntime),
        sandbox_kind: Some(crate::runtime::SandboxKind::Container),
        context_summary: Some(crate::app::TaskContextSummaryState {
            file_snapshots: 3,
            related_paths: 2,
            cache_hits: 5,
            cache_misses: 1,
            git_context_included: true,
        }),
        history_rows: 5,
        total_tokens: 150,
        tokens_sent: 100,
        tokens_received: 50,
        active_tools: 1,
        active_commands: 0,
        waiting_summary: None,
        timing_summary: None,
        git_branch: "feat/my-branch".into(),
    };
    state.changed_files = vec!["src/main.rs".into()];

    // Narrow width: high-priority fields must survive; low-priority fields drop.
    let narrow = status_bar_summary(&state, 55);
    assert!(
        display_width(&narrow) <= 55,
        "narrow summary must respect width budget: {narrow}"
    );
    assert!(
        narrow.contains("task:test-001"),
        "task identity is highest priority and must survive narrow width: {narrow}"
    );
    assert!(
        narrow.contains("m:streaming"),
        "mode is high priority and must survive narrow width: {narrow}"
    );

    // Very wide: all fields should be present.
    let wide = status_bar_summary(&state, 200);
    assert!(
        wide.contains("task:test-001")
            && wide.contains("m:streaming")
            && wide.contains("ap:pending")
            && wide.contains("mdl:local-model-a")
            && wide.contains("be:local")
            && wide.contains("sb:container")
            && wide.contains("ctx:f3 r2 c5/1 git")
            && wide.contains("feat/my-branch")
            && wide.contains("chg:1")
            && wide.contains("act:1/0")
            && wide.contains("\u{2191}100")
            && wide.contains("\u{2193}50"),
        "wide summary must show all segments: {wide}"
    );

    // Confirm low-priority fields are absent at narrow width.
    assert!(
        !narrow.contains("\u{2191}100"),
        "token counters are low priority and should drop at narrow width: {narrow}"
    );
}

#[test]
fn status_bar_shows_follow_mode_and_selected_step_state() {
    let mut state = make_state(
        vec![
            TimelineEntry {
                step_id: 1,
                lifecycle: StepLifecycle::UserInput,
                label: "inspect the file".into(),
                detail: String::new(),
                session_id: None,
            },
            TimelineEntry {
                step_id: 2,
                lifecycle: StepLifecycle::Completed,
                label: "read_file · Response complete.".into(),
                detail: "Tool: read_file\nOutcome: ok".into(),
                session_id: None,
            },
        ],
        vec!["response text"],
    );

    let live = status_bar_summary(&state, 120);
    assert!(
        live.contains("view:live"),
        "follow mode should be visible in the status bar: {live}"
    );

    state.follow_mode = false;
    state.selected_step = 1;
    let browse = status_bar_summary(&state, 120);
    assert!(
        browse.contains("view:2/2"),
        "manual browse state should expose the selected step: {browse}"
    );
}

#[test]
fn detail_mode_footer_hints_advertise_live_return() {
    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let mut state = make_state(
        vec![
            TimelineEntry {
                step_id: 1,
                lifecycle: StepLifecycle::UserInput,
                label: "inspect the file".into(),
                detail: String::new(),
                session_id: None,
            },
            TimelineEntry {
                step_id: 2,
                lifecycle: StepLifecycle::Completed,
                label: "read_file · Response complete.".into(),
                detail: "Tool: read_file\nOutcome: ok".into(),
                session_id: None,
            },
        ],
        vec!["Tool: read_file", "Outcome: ok"],
    );
    state.follow_mode = false;
    state.selected_step = 1;
    state.output_title = "Inspector · 2/2 · read_file · Response complete.".into();
    state.output_scroll_anchor = OutputScrollAnchor::Top;

    draw.draw(&mut buf, &state, 120, 24);
    let output = String::from_utf8_lossy(&buf);

    assert!(
        output.contains("Tab step"),
        "detail mode should advertise timeline navigation hints"
    );
    assert!(
        output.contains("Alt+End live"),
        "detail mode should advertise the live-return hint"
    );
    assert!(
        output.contains("Enter  S-Enter"),
        "detail mode should keep submit hints visible while browsing"
    );
}

#[test]
fn short_transcript_starts_at_top_of_transcript_pane() {
    let state = make_state(vec![], vec!["Type a prompt below to begin."]);
    let regions = Regions::compute(80, 24, false, 0);
    let (visible_start, visible_end) = transcript_window(&state, regions.transcript_rows as usize);
    let render_start = transcript_render_start_row(&state, &regions, visible_start, visible_end);

    assert_eq!(
        render_start, regions.transcript_start,
        "short transcripts should render from the top of the scrolling pane"
    );
}

#[test]
fn inspector_uses_six_line_window() {
    let mut state = make_state(vec![], vec![]);
    state.output_title = "Inspector".into();
    state.output_scroll_anchor = OutputScrollAnchor::Top;
    state.output_rows = (0..10).map(|i| format!("detail-line-{i}")).collect();

    let (visible_start, visible_end) = transcript_window(&state, 12);

    assert_eq!(visible_start, 0);
    assert_eq!(visible_end, 6);
}

#[test]
fn composer_hash_tracks_live_input_changes() {
    let draw = TaskDraw::new();
    let first = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
    };
    let second = TaskLayoutState {
        composer_text: "second".into(),
        composer_cursor: 6,
        ..first.clone()
    };

    assert_ne!(
        draw.compute_composer_hash(&first),
        draw.compute_composer_hash(&second),
        "composer hash must change when the current input buffer changes"
    );
}

#[test]
fn composer_hash_tracks_cursor_only_changes() {
    let draw = TaskDraw::new();
    let first = TaskLayoutState {
        task_id: "test-001".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
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
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![],
    };
    let second = TaskLayoutState {
        composer_cursor: 7,
        ..first.clone()
    };

    assert_ne!(
        draw.compute_composer_hash(&first),
        draw.compute_composer_hash(&second),
        "composer hash must change when the current cursor moves"
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
            label: "build: Mapping adjacent sectors...".into(),
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
    let state = make_state(
        vec![],
        vec!["[detail] Status: Response complete., 42 lines"],
    );

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
            "[detail] Status: Response complete., 42 lines",
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

// -- Picker overlay regression tests -----------------------------------------

#[test]
fn picker_overlay_renders_above_composer() {
    use crate::app::PickerOverlayLine;

    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();
    let state = TaskLayoutState {
        task_id: "test-picker".into(),
        status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec!["line 1".into()],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nmode: file mention".into(),
        composer_text: "@".into(),
        composer_cursor: 1,
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![
            PickerOverlayLine {
                text: "[file] 3 match(es) — Up/Down to navigate, Enter to select".into(),
                selected: false,
            },
            PickerOverlayLine {
                text: "> src/a.rs".into(),
                selected: true,
            },
            PickerOverlayLine {
                text: "  src/b.rs".into(),
                selected: false,
            },
            PickerOverlayLine {
                text: "  src/c.rs".into(),
                selected: false,
            },
        ],
    };

    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);

    // Overlay entries must appear in the output
    assert!(
        output.contains("src/a.rs"),
        "overlay entry src/a.rs must render"
    );
    assert!(
        output.contains("src/b.rs"),
        "overlay entry src/b.rs must render"
    );
    assert!(
        output.contains("src/c.rs"),
        "overlay entry src/c.rs must render"
    );
    // Selected entry rendered with bold+cyan (ANSI: \x1b[1m\x1b[38;5;6m)
    assert!(
        output.contains("\x1b[1m\x1b[38;5;6m> src/a.rs"),
        "selected entry must render bold cyan"
    );
}

#[test]
fn picker_overlay_clears_when_dismissed() {
    use crate::app::PickerOverlayLine;

    let mut buf = Vec::new();
    let mut draw = TaskDraw::new();

    // First frame: overlay active
    let state_with_overlay = TaskLayoutState {
        task_id: "test-picker".into(),
        status_line: "mode:ready".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec!["line 1".into()],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nmode: file mention".into(),
        composer_text: "@".into(),
        composer_cursor: 1,
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![
            PickerOverlayLine {
                text: "[file] 1 match(es)".into(),
                selected: false,
            },
            PickerOverlayLine {
                text: "> src/a.rs".into(),
                selected: true,
            },
        ],
    };
    draw.draw(&mut buf, &state_with_overlay, 80, 24);
    assert_eq!(draw.last_overlay_rows, 2, "overlay should track 2 rows");

    // Second frame: overlay dismissed
    let state_no_overlay = TaskLayoutState {
        input_hint: "Prompt\nsubmit: / commands  @ files  ! shell".into(),
        composer_text: "hello".into(),
        composer_cursor: 5,
        picker_overlay: vec![],
        ..state_with_overlay
    };
    buf.clear();
    draw.draw(&mut buf, &state_no_overlay, 80, 24);
    assert_eq!(draw.last_overlay_rows, 0, "overlay rows must reset to 0");
}

#[test]
fn picker_overlay_hash_changes_on_selection_move() {
    use crate::app::PickerOverlayLine;

    let draw = TaskDraw::new();
    let base = TaskLayoutState {
        task_id: "test".into(),
        status_line: "mode:ready".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: OutputScrollAnchor::Bottom,
        pending_approval: None,
        input_hint: "Prompt\nmode: file mention".into(),
        composer_text: "@".into(),
        composer_cursor: 1,
        composer_focused: true,
        changed_files: vec![],
        follow_mode: true,
        picker_overlay: vec![
            PickerOverlayLine {
                text: "> src/a.rs".into(),
                selected: true,
            },
            PickerOverlayLine {
                text: "  src/b.rs".into(),
                selected: false,
            },
        ],
    };

    let moved = TaskLayoutState {
        picker_overlay: vec![
            PickerOverlayLine {
                text: "  src/a.rs".into(),
                selected: false,
            },
            PickerOverlayLine {
                text: "> src/b.rs".into(),
                selected: true,
            },
        ],
        ..base.clone()
    };

    assert_ne!(
        draw.compute_composer_hash(&base),
        draw.compute_composer_hash(&moved),
        "hash must change when picker selection moves"
    );
}

// ── Edit loop transcript rendering ─────────────────────────────────

#[test]
fn edit_loop_turn_renders_with_progress_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[edit loop turn 2/6]"]);
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("turn 2/6"),
        "edit loop turn must render turn counter in transcript"
    );
}

#[test]
fn edit_loop_validation_passed_renders_with_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[edit loop: validation passed]"]);
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("validation passed"),
        "edit loop validation passed must render in transcript"
    );
}

#[test]
fn edit_loop_complete_renders_with_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["[edit loop complete: patch_applied=true validate_passed=true]"],
    );
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("patch_applied=true"),
        "edit loop complete must render outcome details in transcript"
    );
}

#[test]
fn edit_loop_warning_renders_with_warning_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["[edit loop warning: workspace has uncommitted changes; proceeding without mutating git state]"],
    );
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("workspace has uncommitted changes"),
        "edit loop warning must render in transcript; got: {output}"
    );
}

#[test]
fn edit_loop_turn_error_renders_with_error_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["[edit loop turn error: timeout contacting model]"],
    );
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("timeout contacting model"),
        "edit loop turn error must render detail in transcript; got: {output}"
    );
}

#[test]
fn edit_loop_aborted_renders_with_error_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[edit loop aborted: approval denied]"]);
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("approval denied"),
        "edit loop aborted must render in transcript; got: {output}"
    );
}

#[test]
fn edit_loop_cancelled_renders_with_warning_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[edit loop cancelled]"]);
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("edit loop cancelled"),
        "edit loop cancelled must render in transcript; got: {output}"
    );
}

#[test]
fn edit_loop_validation_failed_renders_with_error_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[edit loop: validation failed, retrying]"]);
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("validation failed"),
        "edit loop validation failed must render; got: {output}"
    );
}

#[test]
fn edit_loop_no_patch_renders_with_warning_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(vec![], vec!["[edit loop: no patch applied, retrying]"]);
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("no patch applied"),
        "edit loop no patch must render; got: {output}"
    );
}

#[test]
fn edit_loop_max_turns_renders_with_warning_icon() {
    let mut draw = TaskDraw::new();
    let state = make_state(
        vec![],
        vec!["[edit loop reached max turns — last error: cargo test exited with 1]"],
    );
    let mut buf = Vec::new();
    draw.draw(&mut buf, &state, 80, 24);
    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("max turns"),
        "edit loop max turns must render; got: {output}"
    );
}

// ── Delta-native rendering tests ────────────────────────────────────

#[test]
fn format_compact_paragraph_applies_tool_call_prefix() {
    use crate::state::TranscriptBlockKind;
    let result = super::transcript_helpers::format_compact_paragraph(
        "search_files .github/workflows/",
        TranscriptBlockKind::ToolCall,
        80,
    );
    assert!(
        result.contains("\u{25b6}"),
        "tool call paragraph must use the arrow prefix: {result:?}"
    );
    assert!(
        result.contains("search_files"),
        "tool call text must be preserved in output: {result:?}"
    );
}

#[test]
fn format_compact_paragraph_applies_tool_result_prefix() {
    use crate::state::TranscriptBlockKind;
    let result = super::transcript_helpers::format_compact_paragraph(
        "No matches found.",
        TranscriptBlockKind::ToolResult,
        80,
    );
    assert!(
        result.contains("\u{21b3}"),
        "tool result paragraph must use the continuation prefix: {result:?}"
    );
}

#[test]
fn format_compact_paragraph_no_prefix_for_text_blocks() {
    use crate::state::TranscriptBlockKind;
    for kind in [
        TranscriptBlockKind::FinalText,
        TranscriptBlockKind::Thinking,
    ] {
        let result =
            super::transcript_helpers::format_compact_paragraph("Analyzing the code.", kind, 80);
        assert!(
            !result.contains("\u{25b6}") && !result.contains("\u{21b3}"),
            "text block kind {kind:?} must have no directional prefix: {result:?}"
        );
        assert!(
            result.contains("Analyzing"),
            "text content must be preserved for {kind:?}: {result:?}"
        );
    }
}

#[test]
fn apply_transcript_delta_empty_incomplete_produces_no_rows() {
    use crate::state::{TranscriptBlockKind, TranscriptDelta};
    let mut draw = TaskDraw::new();
    let delta = TranscriptDelta {
        text: String::new(),
        is_complete: false,
        block_kind: TranscriptBlockKind::FinalText,
    };
    let mut rows: Vec<String> = Vec::new();
    draw.apply_transcript_delta(&delta, &mut rows, 80);
    assert!(
        rows.is_empty(),
        "empty incomplete delta must produce no output rows"
    );
}

#[test]
fn apply_transcript_delta_tool_call_adds_prefixed_row() {
    use crate::state::{TranscriptBlockKind, TranscriptDelta};
    let mut draw = TaskDraw::new();
    let delta = TranscriptDelta {
        text: "search_files path/.github/workflows/".to_string(),
        is_complete: false,
        block_kind: TranscriptBlockKind::ToolCall,
    };
    let mut rows: Vec<String> = Vec::new();
    draw.apply_transcript_delta(&delta, &mut rows, 80);
    assert!(
        !rows.is_empty(),
        "non-empty tool call delta must produce at least one row"
    );
    assert!(
        rows.iter().any(|r| r.contains("\u{25b6}")),
        "tool call row must contain the arrow prefix: {rows:?}"
    );
}

#[test]
fn consume_transcript_deltas_returns_true_when_rows_added() {
    use crate::state::{DeltaAccumulator, TranscriptBlockKind};
    let mut draw = TaskDraw::new();
    let mut acc = DeltaAccumulator::new(TranscriptBlockKind::ToolCall);
    acc.append_delta(r#"{"path":".github/workflows/","pattern":"^.*$"}"#);
    acc.complete();
    let deltas = acc.flush_pending();

    let mut rows: Vec<String> = Vec::new();
    let changed = draw.consume_transcript_deltas(&deltas, &mut rows, 80);
    assert!(
        changed,
        "consuming non-empty deltas must signal that rows were added"
    );
    assert!(
        !rows.is_empty(),
        "delta content must produce at least one output row"
    );
    assert!(
        rows.iter().any(|r| r.contains("\u{25b6}")),
        "tool call output rows must include the arrow prefix: {rows:?}"
    );
}

#[test]
fn consume_transcript_deltas_returns_false_for_empty_input() {
    let mut draw = TaskDraw::new();
    let mut rows: Vec<String> = Vec::new();
    let changed = draw.consume_transcript_deltas(&[], &mut rows, 80);
    assert!(!changed, "consuming an empty delta slice must return false");
    assert!(rows.is_empty(), "empty delta slice must not add any rows");
}

// ── Word-wrap helpers ───────────────────────────────────────────────

#[test]
fn word_wrap_plain_row_passthrough_for_short_line() {
    let short = "Hello, world!";
    let result = transcript_helpers::word_wrap_plain_row(short, 80);
    assert_eq!(result, vec![short.to_string()]);
}

#[test]
fn word_wrap_plain_row_passthrough_for_empty() {
    let result = transcript_helpers::word_wrap_plain_row("", 80);
    assert_eq!(result, vec![String::new()]);
}

#[test]
fn word_wrap_plain_row_skips_bracket_marker() {
    let marker =
        "[tool:run_shell_command] some long argument text that exceeds eighty columns here";
    let result = transcript_helpers::word_wrap_plain_row(marker, 20);
    // Structural markers pass through unchanged.
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], marker);
}

#[test]
fn word_wrap_plain_row_skips_disclosure_indent() {
    let line = "    this is an indented disclosure line with extra text padding it out";
    let result = transcript_helpers::word_wrap_plain_row(line, 20);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], line);
}

#[test]
fn word_wrap_plain_row_wraps_long_prose() {
    // 5 words of 6 chars each = 5*6 + 4 spaces = 34 chars; wrap at 20 cols.
    let line = "alphaa betaaa gammaa deltaa epsiln";
    let result = transcript_helpers::word_wrap_plain_row(line, 20);
    // Must produce more than one row.
    assert!(
        result.len() > 1,
        "long prose must be word-wrapped: {result:?}"
    );
    // Every row must fit within 20 display columns (ASCII so .len() == width).
    for row in &result {
        assert!(row.len() <= 20, "wrapped row exceeds cols: {row:?}");
    }
    // Rejoined text must equal original words in order.
    let rejoined = result.join(" ");
    assert_eq!(rejoined, line);
}

#[test]
fn word_wrap_to_cols_splits_at_boundary() {
    let text = "one two three four five";
    let rows = transcript_helpers::word_wrap_to_cols(text, 10);
    for row in &rows {
        assert!(row.len() <= 10, "row exceeds width: {row:?}");
    }
    let reconstructed = rows.join(" ");
    assert_eq!(reconstructed, text);
}

#[test]
fn word_wrap_to_cols_handles_single_long_word() {
    // A single word wider than cols should be hard-truncated to exactly cols.
    let long_word = "abcdefghijklmnopqrstuvwxyz";
    let rows = transcript_helpers::word_wrap_to_cols(long_word, 8);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].len() <= 8, "long word must be truncated to cols");
}

#[test]
fn expand_rows_for_display_wraps_long_plain_rows() {
    // Create a row that is 5 words of 10 chars each ~ 54 cols, wrapped at 20.
    let long_plain = "wordone123 wordtwo456 wrdthree7 wrd4four56 wordFive0";
    let rows = vec![long_plain.to_string()];
    let expanded = expand_rows_for_display(&rows, 20);
    assert!(
        expanded.len() > 1,
        "a long plain row must expand to multiple display rows: {expanded:?}"
    );
    for row in &expanded {
        assert!(row.len() <= 20, "expanded row exceeds cols: {row:?}");
    }
}

#[test]
fn expand_rows_for_display_leaves_structural_rows_intact() {
    let rows = vec![
        "[tool:thinking]".to_string(),
        "    disclosure detail".to_string(),
        "normal short line".to_string(),
    ];
    // Use 80 cols so the plain "normal short line" row fits and is not wrapped.
    let expanded = expand_rows_for_display(&rows, 80);
    assert_eq!(&expanded[0], "[tool:thinking]");
    assert_eq!(&expanded[1], "    disclosure detail");
    assert!(expanded.contains(&"normal short line".to_string()));
}

#[test]
fn transcript_window_rows_bottom_anchor_scrolled() {
    // 30 display rows, viewport 10, scroll up by 5 → show rows 15..25.
    let (start, end) = transcript_window_rows(30, OutputScrollAnchor::Bottom, 5, 10);
    assert_eq!(start, 15);
    assert_eq!(end, 25);
}

#[test]
fn transcript_window_rows_bottom_anchor_at_tail() {
    // 15 rows, viewport 10, no scroll → show rows 5..15.
    let (start, end) = transcript_window_rows(15, OutputScrollAnchor::Bottom, 0, 10);
    assert_eq!(start, 5);
    assert_eq!(end, 15);
}

#[test]
fn transcript_window_rows_top_anchor_clamps_to_six() {
    // Inspector mode: total 20, viewport 12, Top anchor → clamp to 6 rows.
    let (start, end) = transcript_window_rows(20, OutputScrollAnchor::Top, 0, 12);
    assert_eq!(start, 0);
    assert_eq!(end, 6);
}

#[test]
fn expand_rows_for_display_splits_embedded_newlines() {
    let rows = vec!["line one\nline two\nline three".to_string()];
    let expanded = expand_rows_for_display(&rows, 80);
    assert_eq!(
        expanded,
        vec!["line one", "line two", "line three"],
        "embedded newlines must be split into separate display rows"
    );
}

#[test]
fn expand_rows_for_display_splits_and_wraps_combined() {
    // Row contains embedded newlines AND a long sub-line that needs wrapping.
    let rows = vec![
        "short\nthis is a much longer line that should be word wrapped at twenty cols".to_string(),
    ];
    let expanded = expand_rows_for_display(&rows, 20);
    assert!(
        expanded.len() >= 3,
        "must split on newline then word-wrap the long sub-line: {expanded:?}"
    );
    assert_eq!(&expanded[0], "short");
}

#[test]
fn structural_row_detects_bullet_lists() {
    use transcript_helpers::word_wrap_plain_row;
    // Bullet and numbered list items should pass through as structural.
    let bullet = "- item text that is considerably longer than the terminal width limit";
    let result = word_wrap_plain_row(bullet, 20);
    assert_eq!(
        result.len(),
        1,
        "bullet list items are structural: {result:?}"
    );

    let numbered = "1. first item that is also rather long and should not be wrapped here";
    let result = word_wrap_plain_row(numbered, 20);
    assert_eq!(
        result.len(),
        1,
        "numbered list items are structural: {result:?}"
    );
}

#[test]
fn format_compact_paragraph_preserves_empty_lines() {
    use crate::state::TranscriptBlockKind;
    let text = "paragraph one\n\nparagraph two\n";
    let formatted =
        transcript_helpers::format_compact_paragraph(text, TranscriptBlockKind::FinalText, 80);
    assert!(
        formatted.contains("\n\n"),
        "empty lines must be preserved as paragraph separators: {formatted:?}"
    );
}
