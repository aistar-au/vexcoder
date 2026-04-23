use super::*;

use crate::ui::tui::{Terminal, backend::TestBackend};

#[test]
fn all_modals_use_unified_renderer() {
    let backend = TestBackend::new(100, 30);
    let mut tui = Terminal::new(backend).expect("test backend");

    let modals = [
        OverlayModal::PatchApprove {
            patch_preview: "diff --git a/src/app/mod.rs b/src/app/mod.rs",
            scroll_offset: 0,
        },
        OverlayModal::ToolPermission {
            tool_name: "exec_command",
            input_preview: "echo hi",
            auto_approve_enabled: false,
        },
        OverlayModal::MemoryClear,
    ];

    for modal in modals {
        tui.draw(|frame| render_overlay_modal(frame, modal))
            .expect("renderer should support every modal class");
    }
}

#[test]
fn patch_modal_uses_body_viewport_for_visible_range() {
    let (_, _, body, _) = modal_content(
        OverlayModal::PatchApprove {
            patch_preview: "a\nb\nc\nd\ne\nf",
            scroll_offset: 0,
        },
        5,
    );

    assert!(
        body.iter()
            .any(|line| line.to_string().contains("showing 1-1 of 6"))
    );
}

#[test]
fn memory_clear_modal_shows_matching_shortcuts() {
    let (_, _, _, shortcuts) = modal_content(OverlayModal::MemoryClear, 5);

    assert_eq!(shortcuts, "y/yes confirm   n/esc cancel");
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

    assert_eq!(
        diff_line_color(DiffLineKind::Other, Color::White),
        Color::White
    );
    assert_eq!(
        diff_line_color(DiffLineKind::Other, Color::Gray),
        Color::Gray
    );

    assert_eq!(history_row_style("+add").fg, Some(Color::Green));
    assert_eq!(history_row_style("plain").fg, Some(Color::White));
    assert_eq!(styled_diff_line("+add").style.fg, Some(Color::Green));
    assert_eq!(styled_diff_line(" ctx").style.fg, Some(Color::White));
}

#[test]
fn test_changed_files_and_live_approval_prompt_render() {
    let backend = TestBackend::new(80, 24);
    let mut tui = Terminal::new(backend).unwrap();
    let state = crate::app::TaskViewProjection {
        status_line: "AwaitingApproval".into(),
        output_rows: vec![crate::app::TranscriptRow::Plain(
            "[approval] ApplyPatch: src/main.rs · awaiting approval".to_string(),
        )],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        picker_overlay: vec![],
    };

    tui.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = tui.backend().buffer().clone();
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
}

#[test]
fn task_layout_keeps_output_surface_primary_when_steps_are_pending() {
    let backend = TestBackend::new(80, 24);
    let mut tui = Terminal::new(backend).unwrap();
    let state = crate::app::TaskViewProjection {
        status_line: "Running".into(),
        output_rows: vec![crate::app::TranscriptRow::Plain(
            "streamed output".to_string(),
        )],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Top,
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        picker_overlay: vec![],
    };

    tui.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = tui.backend().buffer().clone();
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
fn task_layout_renders_status_row_on_primary_surface() {
    let backend = TestBackend::new(80, 24);
    let mut tui = Terminal::new(backend).unwrap();
    let state = crate::app::TaskViewProjection {
        status_line: "mode: task | approval: auto | branch: work/pr347".into(),
        output_rows: vec![crate::app::TranscriptRow::Plain(
            "status row stays visible".to_string(),
        )],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        picker_overlay: vec![],
    };

    tui.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = tui.backend().buffer().clone();
    let flat = rendered
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        flat.contains("mode: task | approval: auto | branch: work/pr347"),
        "task surface must keep the status row visible"
    );
}

#[test]
fn task_layout_uses_full_body_for_transcript_on_large_display() {
    let backend = TestBackend::new(80, 40);
    let mut tui = Terminal::new(backend).unwrap();
    let state = crate::app::TaskViewProjection {
        status_line: "Running".into(),
        output_rows: vec![crate::app::TranscriptRow::Plain("output".to_string())],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Top,
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        picker_overlay: vec![],
    };

    tui.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = tui.backend().buffer().clone();
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
fn task_layout_without_changed_files_starts_short_transcript_below_status_row() {
    let backend = TestBackend::new(60, 16);
    let mut tui = Terminal::new(backend).unwrap();
    let state = crate::app::TaskViewProjection {
        status_line: "Running".into(),
        output_rows: vec![crate::app::TranscriptRow::Plain("body row".to_string())],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        picker_overlay: vec![],
    };

    tui.draw(|f| render_task_layout(f, &state)).unwrap();

    let rendered = tui.backend().buffer().clone();
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
    let body_row_pos = rows
        .iter()
        .position(|row| row.contains("body row"))
        .expect("body row must appear in rendered output");

    assert!(
        body_row_pos <= 2,
        "short transcript content should start near the top of the output pane; found at row {body_row_pos}"
    );
}

#[test]
fn task_output_window_uses_expanded_display_rows() {
    let state = crate::app::TaskViewProjection {
        status_line: "Running".into(),
        output_rows: vec![crate::app::TranscriptRow::Plain(
            "alpha beta gamma delta epsilon".to_string(),
        )],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        picker_overlay: vec![],
    };

    let expanded = expand_rows_for_display(&state.output_rows, 10);
    assert!(expanded.len() > 2, "fixture must wrap into multiple rows");

    let (start, end) = task_output_window(&state, 10, 2);
    assert_eq!((start, end), (expanded.len() - 2, expanded.len()));
}

#[test]
fn task_output_render_area_top_aligns_content() {
    let state = crate::app::TaskViewProjection {
        status_line: String::new(),
        output_rows: vec![],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        composer_text: String::new(),
        composer_cursor: 0,
        composer_focused: true,
        picker_overlay: vec![],
    };
    let area = crate::ui::tui::layout::Rect::new(0, 5, 80, 20);

    let render_area = task_output_render_area(&state, area, 5);
    assert_eq!(
        render_area.y, area.y,
        "content should be top-aligned: y = area.y"
    );
    assert_eq!(render_area.height, 5);

    let render_area_full = task_output_render_area(&state, area, 20);
    assert_eq!(render_area_full.y, area.y);
    assert_eq!(render_area_full.height, 20);
}

#[test]
fn numbered_list_items_are_parsed() {
    let result = parse_numbered_list_item("1. First item").expect("must parse numbered list");
    assert_eq!(result.0, "1. ");
    assert_eq!(result.1, "First item");

    let result = parse_numbered_list_item("42. Large number").expect("must parse long prefix");
    assert_eq!(result.0, "42. ");
    assert_eq!(result.1, "Large number");

    assert!(parse_numbered_list_item("not a list").is_none());
    assert!(parse_numbered_list_item("1.no space").is_none());
}

#[test]
fn horizontal_rules_are_detected() {
    assert!(is_horizontal_rule("---"));
    assert!(is_horizontal_rule("***"));
    assert!(is_horizontal_rule("___"));
    assert!(is_horizontal_rule("- - -"));
    assert!(is_horizontal_rule("-----"));
    assert!(!is_horizontal_rule("--"));
    assert!(!is_horizontal_rule("abc"));
    assert!(!is_horizontal_rule("---a"));
}

#[test]
fn expand_rows_for_display_wraps_long_plain_rows() {
    use crate::app::TranscriptRow;
    let rows = vec![TranscriptRow::Plain(
        "wordone123 wordtwo456 wrdthree7 wrd4four56 wordFive0".to_string(),
    )];
    let expanded = expand_rows_for_display(&rows, 20);

    assert!(expanded.len() > 1, "plain prose should wrap: {expanded:?}");
    for row in &expanded {
        assert!(
            display_width(row.as_display_str()) <= 20,
            "expanded row exceeds width: {row:?}"
        );
    }
}

#[test]
fn expand_rows_for_display_leaves_structural_rows_intact() {
    use crate::app::TranscriptRow;
    let rows = vec![
        TranscriptRow::Plain("[tool:thinking]".to_string()),
        TranscriptRow::Plain("    disclosure detail".to_string()),
        TranscriptRow::Plain("normal short line".to_string()),
    ];

    let expanded = expand_rows_for_display(&rows, 80);
    assert_eq!(expanded[0].as_display_str(), "[tool:thinking]");
    assert_eq!(expanded[1].as_display_str(), "    disclosure detail");
    assert!(
        expanded
            .iter()
            .any(|r| r.as_display_str() == "normal short line")
    );
}

#[test]
fn expand_rows_for_display_splits_embedded_newlines() {
    use crate::app::TranscriptRow;
    let rows = vec![TranscriptRow::Plain(
        "line one\nline two\nline three".to_string(),
    )];

    assert_eq!(
        expand_rows_for_display(&rows, 80),
        vec![
            TranscriptRow::Plain("line one".to_string()),
            TranscriptRow::Plain("line two".to_string()),
            TranscriptRow::Plain("line three".to_string()),
        ]
    );
}

#[test]
fn task_layout_renders_picker_overlay() {
    let backend = TestBackend::new(80, 24);
    let mut tui = Terminal::new(backend).unwrap();
    let state = crate::app::TaskViewProjection {
        status_line: "Running".into(),
        output_rows: vec![crate::app::TranscriptRow::Plain("line 1".to_string())],
        output_scroll_offset: 0,
        output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
        composer_text: "@".into(),
        composer_cursor: 1,
        composer_focused: true,
        picker_overlay: vec![
            crate::app::PickerOverlayLine {
                text: "[file] 3 match(es) — Up/Down to navigate, Enter to select".into(),
                selected: false,
            },
            crate::app::PickerOverlayLine {
                text: "> src/a.rs".into(),
                selected: true,
            },
            crate::app::PickerOverlayLine {
                text: "  src/b.rs".into(),
                selected: false,
            },
        ],
    };

    tui.draw(|frame| render_task_layout(frame, &state)).unwrap();

    let rendered = tui.backend().buffer().clone();
    let flat = rendered
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(flat.contains("src/a.rs"));
    assert!(flat.contains("src/b.rs"));
}

#[test]
fn transcript_output_line_styles_paragraph_markers() {
    use crate::app::TranscriptRow;
    let tool = transcript_output_line(&TranscriptRow::ToolHeader(
        "read_file · Response complete.".to_string(),
    ));
    let detail = transcript_output_line(&TranscriptRow::ToolDetail(
        "Scope: Read file content".to_string(),
    ));
    let evidence = transcript_output_line(&TranscriptRow::Evidence("Outcome: ok".to_string()));

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
    use crate::app::TranscriptRow;
    let waiting = transcript_output_line(&TranscriptRow::WaitingPlaceholder(
        "[thinking] Mapping adjacent sectors...".to_string(),
    ));

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
    use crate::app::TranscriptRow;
    let plain = transcript_output_line(&TranscriptRow::Plain("plain response".to_string()));

    assert_eq!(plain.spans.len(), 1);
    assert_eq!(plain.spans[0].content.as_ref(), "plain response");
    assert_eq!(plain.spans[0].style.fg, Some(Color::White));
}

#[test]
fn transcript_output_line_styles_single_line_markdown_without_dropping_rows() {
    use crate::app::TranscriptRow;
    let expected = crate::ui::render::markdown::markdown_to_inline_line("## Heading")
        .expect("single heading line");
    let heading = transcript_output_line(&TranscriptRow::AssistantText {
        text: "## Heading".to_string(),
        streaming: false,
    });
    let text: String = heading
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert_eq!(text, "Heading");
    assert_eq!(heading.spans[0].style.fg, expected.spans[0].style.fg);
}

#[test]
fn transcript_output_line_keeps_fenced_code_markers_literal_until_pre_expansion_rendering() {
    use crate::app::TranscriptRow;
    let fence = transcript_output_line(&TranscriptRow::AssistantText {
        text: "```rust".to_string(),
        streaming: false,
    });

    assert_eq!(fence.spans.len(), 1);
    assert_eq!(fence.spans[0].content.as_ref(), "```rust");
    assert_eq!(fence.spans[0].style.fg, Some(Color::White));
}

#[test]
fn transcript_output_line_keeps_command_session_progress_status_with_pid() {
    use crate::app::TranscriptRow;
    let command = transcript_output_line(&TranscriptRow::Plain(
        "[command session started pid=42] cargo test".to_string(),
    ));

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
    use crate::app::TranscriptRow;
    let tool = transcript_output_line(&TranscriptRow::ToolHeader(
        "write_file \u{00b7} src/lib.rs \u{00b7} failed".to_string(),
    ));

    assert_eq!(tool.spans[1].content.as_ref(), "write_file");
    assert_eq!(tool.spans[1].style.fg, Some(Color::White));
    assert_eq!(tool.spans[3].content.as_ref(), "src/lib.rs");
    assert_eq!(tool.spans[3].style.fg, Some(Color::Gray));
    assert_eq!(tool.spans[5].content.as_ref(), "failed");
    assert_eq!(tool.spans[5].style.fg, Some(Color::Red));
    assert!(tool.spans[5].style.add_modifier.contains(Modifier::BOLD));
}
