use super::*;
use crate::ui::tui::{Terminal, backend::TestBackend};

#[test]
fn all_modals_render_without_panic() {
    let backend = TestBackend::new(100, 30);
    let mut tui = Terminal::new(backend).expect("test backend");
    for modal in [
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
    ] {
        tui.draw(|frame| render_overlay_modal(frame, modal))
            .expect("render");
    }
}

#[test]
fn diff_line_styling_maps_prefixes_to_colors() {
    assert_eq!(styled_diff_line("+added").style.fg, Some(Color::Green));
    assert_eq!(styled_diff_line("-removed").style.fg, Some(Color::Red));
    assert_eq!(styled_diff_line("@@ -1 +1 @@").style.fg, Some(Color::Cyan));
    assert_eq!(styled_diff_line(" context").style.fg, Some(Color::White));
}

#[test]
fn history_visual_line_count_accounts_for_newlines_and_wrapping() {
    assert_eq!(
        history_visual_line_count(
            &["first".to_string(), "a\nb".to_string(), String::new()],
            80
        ),
        4
    );
    assert_eq!(history_visual_line_count(&["123456".to_string()], 3), 2);
}

#[test]
fn visual_window_start_scrolls_after_cursor_exceeds_rows() {
    assert_eq!(visual_window_start(0, 4), 0);
    assert_eq!(visual_window_start(3, 4), 0);
    assert_eq!(visual_window_start(4, 4), 1);
}
