use super::*;
use crate::runtime::AssistantPhase;

#[test]
fn stream_delta_appends_to_assistant_line_not_user_line() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("describe the error".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);
    let hl = mode.history_lines();
    assert_eq!(hl[0], "> describe the error");
    assert!(hl[1].starts_with("assistant"), "got: {:?}", hl.get(1));
}

#[test]
fn scrollback_retains_position_during_streaming() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    use crate::runtime::task_document::NoticeSeverity;
    for i in 0..20 {
        mode.push_document_notice(format!("line-{i}"), NoticeSeverity::Info);
    }
    mode.on_user_input("fix the import error".to_string(), &mut ctx);
    mode.transcript_scroll_offset = 5;
    mode.on_model_update(UiUpdate::StreamDelta(" assistant".to_string()), &mut ctx);
    assert!(
        mode.transcript_scroll_offset > 0,
        "scrollback must not force bottom while user has scrolled up"
    );
}

#[test]
fn output_scroll_end_restores_auto_follow() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("list the test failures".to_string(), &mut ctx);
    for i in 0..50 {
        mode.push_history_line(format!("line-{i}"));
    }
    assert!(mode.auto_follow());
    mode.apply_output_scroll_action(ScrollAction::LineUp);
    assert!(!mode.auto_follow());
    mode.apply_output_scroll_action(ScrollAction::End);
    assert!(mode.auto_follow());
}

#[test]
fn history_status_uses_visual_row_count() {
    use crate::runtime::task_document::NoticeSeverity;
    let mut mode = TuiMode::new();
    mode.push_document_notice("a\nb\nc".to_string(), NoticeSeverity::Info);
    assert!(mode.status_line().contains("history:3"));
}
