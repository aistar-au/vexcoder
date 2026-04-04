use super::*;

use ratatui::{backend::TestBackend, Terminal};

#[test]
fn all_modals_use_unified_renderer() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    let modals = [
        OverlayModal::PatchApprove {
            patch_preview: "diff --git a/src/app/mod.rs b/src/app/mod.rs",
            scroll_offset: 0,
            viewport_rows: 8,
        },
        OverlayModal::ToolPermission {
            tool_name: "exec_command",
            input_preview: "echo hi",
            auto_approve_enabled: false,
        },
    ];

    for modal in modals {
        terminal
            .draw(|frame| render_overlay_modal(frame, modal))
            .expect("renderer should support every modal class");
    }
}

#[test]
fn visual_window_start_scrolls_once_cursor_exceeds_visible_rows() {
    assert_eq!(visual_window_start(0, 4), 0);
    assert_eq!(visual_window_start(3, 4), 0);
    assert_eq!(visual_window_start(4, 4), 1);
    assert_eq!(visual_window_start(7, 4), 4);
}

#[test]
fn diff_line_semantics_are_styled_by_prefix() {
    let add = styled_diff_line("+added");
    let del = styled_diff_line("-removed");
    let hunk = styled_diff_line("@@ -1 +1 @@");
    let ctx = styled_diff_line(" context");

    assert_eq!(add.style.fg, Some(Color::Green));
    assert_eq!(del.style.fg, Some(Color::Red));
    assert_eq!(hunk.style.fg, Some(Color::Cyan));
    assert_eq!(ctx.style.fg, Some(Color::White));
}

#[test]
fn numbered_edit_diff_lines_keep_added_and_removed_colors() {
    let add = styled_diff_line("7 + let new_value = 2;");
    let del = styled_diff_line("7 - let old_value = 1;");

    assert_eq!(add.style.fg, Some(Color::Green));
    assert_eq!(del.style.fg, Some(Color::Red));
}

#[test]
fn history_visual_line_count_tracks_embedded_newlines() {
    let messages = vec![
        "first".to_string(),
        "line-a\nline-b".to_string(),
        String::new(),
    ];
    assert_eq!(history_visual_line_count(&messages, 80), 4);
}

#[test]
fn history_visual_line_count_tracks_wrapped_rows() {
    let messages = vec!["123456".to_string()];
    assert_eq!(history_visual_line_count(&messages, 3), 2);
}

#[test]
fn history_row_style_marks_diff_rows() {
    assert_eq!(history_row_style("+add").fg, Some(Color::Green));
    assert_eq!(history_row_style("-del").fg, Some(Color::Red));
    assert_eq!(history_row_style("@@ -1 +1 @@").fg, Some(Color::Cyan));
    assert_eq!(history_row_style("plain text").fg, Some(Color::White));
}

#[test]
fn classify_diff_line_keeps_header_markers_consistent() {
    assert_eq!(classify_diff_line("diff --git a b"), DiffLineKind::Header);
    assert_eq!(classify_diff_line("index 123..456"), DiffLineKind::Header);
    assert_eq!(classify_diff_line("@@ -1 +1 @@"), DiffLineKind::Header);
    assert_eq!(
        classify_diff_line("12 + inserted line"),
        DiffLineKind::Added
    );
    assert_eq!(
        classify_diff_line("12 - removed line"),
        DiffLineKind::Removed
    );
}

#[test]
fn test_diff_line_color_maps_markers_consistently() {
    // Verify the shared helper maps Added/Removed/Header consistently,
    // regardless of which other_color is passed as the fallback.
    assert_eq!(
        diff_line_color(DiffLineKind::Added, Color::White),
        Color::Green
    );
    assert_eq!(
        diff_line_color(DiffLineKind::Added, Color::Gray),
        Color::Green
    );
    assert_eq!(
        diff_line_color(DiffLineKind::Removed, Color::White),
        Color::Red
    );
    assert_eq!(
        diff_line_color(DiffLineKind::Removed, Color::Gray),
        Color::Red
    );
    assert_eq!(
        diff_line_color(DiffLineKind::Header, Color::White),
        Color::Cyan
    );
    assert_eq!(
        diff_line_color(DiffLineKind::Header, Color::Gray),
        Color::Cyan
    );
    // Other respects the caller's choice of fallback color.
    assert_eq!(
        diff_line_color(DiffLineKind::Other, Color::White),
        Color::White
    );
    assert_eq!(
        diff_line_color(DiffLineKind::Other, Color::Gray),
        Color::Gray
    );
    // Both plain history rows and diff context lines keep the default white.
    assert_eq!(history_row_style("+add").fg, Some(Color::Green));
    assert_eq!(history_row_style("plain").fg, Some(Color::White));
    assert_eq!(styled_diff_line("+add").style.fg, Some(Color::Green));
    assert_eq!(styled_diff_line(" ctx").style.fg, Some(Color::White));
}

#[test]
fn render_timeline_entry_normalizes_prefix_spacing() {
    let approval = render_timeline_entry(
        &crate::app::TimelineEntry {
            step_id: 1,
            lifecycle: crate::app::StepLifecycle::AwaitingApproval,
            label: "ApplyPatch: src/main.rs".into(),
            detail: String::new(),
            session_id: None,
        },
        true,
    );
    let user_input = render_timeline_entry(
        &crate::app::TimelineEntry {
            step_id: 0,
            lifecycle: crate::app::StepLifecycle::UserInput,
            label: "ship it".into(),
            detail: String::new(),
            session_id: None,
        },
        true,
    );

    let approval_text: String = approval
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    let user_input_text: String = user_input
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert_eq!(approval_text, "> [?] ApplyPatch: src/main.rs");
    assert_eq!(user_input_text, "> > ship it");
}

#[test]
fn render_timeline_entry_gives_approved_a_distinct_prefix() {
    let approved = render_timeline_entry(
        &crate::app::TimelineEntry {
            step_id: 1,
            lifecycle: crate::app::StepLifecycle::Approved,
            label: "ApplyPatch: approved".into(),
            detail: String::new(),
            session_id: None,
        },
        false,
    );
    let completed = render_timeline_entry(
        &crate::app::TimelineEntry {
            step_id: 2,
            lifecycle: crate::app::StepLifecycle::Completed,
            label: "ApplyPatch: done".into(),
            detail: String::new(),
            session_id: None,
        },
        false,
    );

    let approved_text: String = approved
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    let completed_text: String = completed
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert!(approved_text.starts_with("  [v]"));
    assert!(completed_text.starts_with("  [ok]"));
}

#[test]
fn test_changed_files_and_live_approval_prompt_render() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = crate::app::TaskLayoutState {
        task_id: "task-001".into(),
        status_line: "AwaitingApproval".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![crate::app::TimelineEntry {
            step_id: 1,
            lifecycle: crate::app::StepLifecycle::AwaitingApproval,
            label: "ApplyPatch: src/main.rs".into(),
            detail: "Tool: ApplyPatch\nFile: src/main.rs".into(),
            session_id: None,
        }],
        selected_step: 0,
        total_steps: 1,
        output_title: "Transcript".into(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        changed_files: vec!["src/main.rs".into()],
        pending_approval: Some("ApplyPatch: src/main.rs".into()),
        input_hint: "ApplyPatch: src/main.rs\n[y/n/s] ".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        follow_mode: true,
        picker_overlay: vec![],
        working_dir: String::new(),
        model_url: String::new(),
    };

    terminal.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = terminal.backend().buffer().clone();
    let flat = rendered
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        flat.contains("src/main.rs"),
        "changed file must appear in rendered output"
    );
    assert!(
        flat.contains("ApplyPatch"),
        "approval prompt must appear in rendered output"
    );
    assert!(
        flat.contains("[y/n/s]"),
        "approval choices must appear in rendered output"
    );
}

#[test]
fn task_layout_keeps_output_surface_primary_when_steps_are_pending() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = crate::app::TaskLayoutState {
        task_id: "task-002".into(),
        status_line: "Running".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![
            crate::app::TimelineEntry {
                step_id: 1,
                lifecycle: crate::app::StepLifecycle::Completed,
                label: "read_file: ok".into(),
                detail: "Tool: read_file\nOutcome: ok".into(),
                session_id: None,
            },
            crate::app::TimelineEntry {
                step_id: 2,
                lifecycle: crate::app::StepLifecycle::Running,
                label: "validate: Mapping adjacent sectors...".into(),
                detail: "Tool: validate\nInput: ...".into(),
                session_id: None,
            },
        ],
        selected_step: 1,
        total_steps: 2,
        output_title: "Inspector".into(),
        output_rows: vec!["streamed output".into()],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Top,
        changed_files: vec![],
        pending_approval: None,
        input_hint: "> ".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        follow_mode: true,
        picker_overlay: vec![],
        working_dir: String::new(),
        model_url: String::new(),
    };

    terminal.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = terminal.backend().buffer().clone();
    let flat = rendered
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        flat.contains("streamed output"),
        "the output pane should remain primary when steps are pending"
    );
    assert!(
        !flat.contains("Orchestrating") && !flat.contains("validate: Mapping adjacent sectors..."),
        "the fallback surface should not render a separate top activity pane"
    );
}

#[test]
fn task_layout_uses_full_body_for_transcript_on_tall_terminals() {
    let backend = TestBackend::new(80, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let timeline_entries = (0..12)
        .map(|index| crate::app::TimelineEntry {
            step_id: index as u64,
            lifecycle: crate::app::StepLifecycle::Completed,
            label: format!("step_{index} · Response complete."),
            detail: format!("Tool: step_{index}"),
            session_id: None,
        })
        .collect();
    let state = crate::app::TaskLayoutState {
        task_id: "task-003".into(),
        status_line: "Running".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries,
        selected_step: 8,
        total_steps: 12,
        output_title: "Inspector".into(),
        output_rows: vec!["output".into()],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Top,
        changed_files: vec![],
        pending_approval: None,
        input_hint: "> ".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        follow_mode: true,
        picker_overlay: vec![],
        working_dir: String::new(),
        model_url: String::new(),
    };

    terminal.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = terminal.backend().buffer().clone();
    let flat = rendered
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        flat.contains("output"),
        "the fallback transcript should use the available body space"
    );
}

#[test]
fn task_layout_without_changed_files_bottom_anchors_short_transcript() {
    let backend = TestBackend::new(60, 16);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = crate::app::TaskLayoutState {
        task_id: "task-004".into(),
        status_line: "Running".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![crate::app::TimelineEntry {
            step_id: 1,
            lifecycle: crate::app::StepLifecycle::Completed,
            label: "step_1 · Response complete.".into(),
            detail: "Tool: step_1".into(),
            session_id: None,
        }],
        selected_step: 0,
        total_steps: 1,
        output_title: "Transcript".into(),
        output_rows: vec!["body row".into()],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        changed_files: vec![],
        pending_approval: None,
        input_hint: "> ".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        follow_mode: true,
        picker_overlay: vec![],
        working_dir: String::new(),
        model_url: String::new(),
    };

    terminal.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = terminal.backend().buffer().clone();
    let rows: Vec<String> = rendered
        .content()
        .chunks(60)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<Vec<_>>()
                .join("")
        })
        .collect();
    let first_non_empty = rows
        .iter()
        .position(|row| row.contains("body row"))
        .expect("body row must appear in rendered output");
    assert!(
        first_non_empty > 0,
        "short transcript should hug the prompt edge instead of starting at the top row"
    );
}

#[test]
fn task_output_window_uses_expanded_display_rows() {
    let state = crate::app::TaskLayoutState {
        task_id: "task-wrap".into(),
        status_line: "Running".into(),
        telemetry: crate::app::TaskTelemetryState::default(),
        timeline_entries: vec![],
        selected_step: 0,
        total_steps: 0,
        output_title: "Transcript".into(),
        output_rows: vec!["alpha beta gamma delta epsilon".into()],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        changed_files: vec![],
        pending_approval: None,
        input_hint: "> ".into(),
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        follow_mode: true,
        picker_overlay: vec![],
        working_dir: String::new(),
        model_url: String::new(),
    };

    let expanded = crate::ui::draw::expand_rows_for_display(&state.output_rows, 10);
    assert!(expanded.len() > 2, "fixture must wrap into multiple rows");

    let (start, end) = task_output_window(&state, 10, 2);
    assert_eq!((start, end), (expanded.len() - 2, expanded.len()));
}

#[test]
fn transcript_output_line_styles_paragraph_markers() {
    let tool = transcript_output_line("[tool] read_file · Response complete.");
    let detail = transcript_output_line("[detail] Scope: Read file content");
    let evidence = transcript_output_line("[evidence] Outcome: ok");

    let tool_text: String = tool
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    let detail_text: String = detail
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    let evidence_text: String = evidence
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert!(tool_text.starts_with("  \u{2726} "));
    assert_eq!(detail_text, "    Scope: Read file content");
    assert!(evidence_text.starts_with("      \u{2727} "));
}

#[test]
fn transcript_output_line_styles_waiting_placeholder_as_progress() {
    let waiting = transcript_output_line("[thinking] Mapping adjacent sectors...");

    let waiting_text: String = waiting
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert_eq!(waiting_text, "  ⋯ Mapping adjacent sectors...");
    assert_eq!(waiting.spans[1].style.fg, Some(Color::Magenta));
    assert!(waiting.spans[1].style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn transcript_output_line_defaults_plain_text_to_white() {
    let plain = transcript_output_line("plain response");

    assert_eq!(plain.spans.len(), 1);
    assert_eq!(plain.spans[0].content.as_ref(), "plain response");
    assert_eq!(plain.spans[0].style.fg, Some(Color::White));
}

#[test]
fn transcript_output_line_styles_single_line_markdown_without_dropping_rows() {
    let heading = transcript_output_line("## Heading");
    let text: String = heading
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert_eq!(text, "Heading");
    assert_eq!(heading.spans[0].style.fg, Some(Color::Cyan));
}

#[test]
fn transcript_output_line_keeps_fenced_code_markers_literal_until_pre_expansion_rendering() {
    let fence = transcript_output_line("```rust");

    assert_eq!(fence.spans.len(), 1);
    assert_eq!(fence.spans[0].content.as_ref(), "```rust");
    assert_eq!(fence.spans[0].style.fg, Some(Color::White));
}

#[test]
fn transcript_output_line_keeps_command_session_progress_status_with_pid() {
    let command = transcript_output_line("[command session started pid=42] cargo test");

    let command_text: String = command
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert!(command_text.contains("pid 42"));
    assert!(command_text.contains("Mapping adjacent sectors..."));
    assert_eq!(
        command.spans.last().map(|span| span.style.fg),
        Some(Some(Color::Magenta))
    );
}

#[test]
fn transcript_output_line_highlights_tool_target_and_failed_status() {
    let tool = transcript_output_line("[tool] write_file · src/lib.rs · failed");

    assert_eq!(tool.spans[1].content.as_ref(), "write_file");
    assert_eq!(tool.spans[1].style.fg, Some(Color::White));
    assert_eq!(tool.spans[3].content.as_ref(), "src/lib.rs");
    assert_eq!(tool.spans[3].style.fg, Some(Color::Gray));
    assert_eq!(tool.spans[5].content.as_ref(), "failed");
    assert_eq!(tool.spans[5].style.fg, Some(Color::Red));
    assert!(tool.spans[5].style.add_modifier.contains(Modifier::BOLD));
}
