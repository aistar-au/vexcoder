use super::*;

#[test]
fn test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("hello".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);

    assert_eq!(mode.history_state.lines[0], "> hello");
    assert_eq!(mode.history_state.lines[1], "assistant");
}
#[test]
fn test_stream_delta_strips_tagged_tool_markup_from_history() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("show diff".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamDelta("I will check.\n<function=git_diff>\n</function>\nDone.".to_string()),
        &mut ctx,
    );

    assert_eq!(mode.history_state.lines[1], "I will check.\n\nDone.");
    assert!(!mode.history_state.lines[1].contains("<function="));
}
#[test]
fn test_stream_delta_hides_incomplete_tool_tag_suffix() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("status".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamDelta("Checking\n<function=git_status".to_string()),
        &mut ctx,
    );

    assert_eq!(mode.history_state.lines[1], "Checking\n");
}
#[test]
fn test_transcript_does_not_exceed_cap_after_n_turns() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var(MAX_HISTORY_LINES_ENV, "10");

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    assert_eq!(mode.history_line_cap, 10);

    for i in 0..20 {
        mode.on_user_input(format!("user-{i}"), &mut ctx);
        assert!(
            mode.history_state.lines.len() <= 10,
            "history must be capped after on_user_input"
        );
        if let Some(idx) = mode.history_state.active_assistant_index {
            assert!(
                idx < mode.history_state.lines.len(),
                "active assistant index must remain valid after cap enforcement"
            );
        }

        mode.on_model_update(UiUpdate::StreamDelta(format!("assistant-{i}")), &mut ctx);
        assert!(
            mode.history_state.lines.len() <= 10,
            "history must be capped after stream update"
        );
        if let Some(idx) = mode.history_state.active_assistant_index {
            assert!(
                idx < mode.history_state.lines.len(),
                "active assistant index must remain valid during streaming"
            );
        }

        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
        assert!(
            mode.history_state.lines.len() <= 10,
            "history must stay capped after turn completion"
        );
    }

    std::env::remove_var(MAX_HISTORY_LINES_ENV);
}
#[test]
fn test_scrollback_retains_position_during_streaming() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.lines = (0..20).map(|i| format!("line-{i}")).collect();
    mode.history_state.active_assistant_index = Some(10);
    mode.history_state.scroll_offset = 5;
    mode.history_state.auto_follow = false;

    mode.on_model_update(UiUpdate::StreamDelta(" assistant".to_string()), &mut ctx);

    assert_eq!(
        mode.history_state.scroll_offset, 5,
        "scrollback position must not be forced while auto-follow is disabled"
    );
}
#[test]
fn test_scrollback_commands_update_scroll_state() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.lines = (0..100).map(|i| format!("line-{i}")).collect();
    mode.history_state.scroll_offset = 80;
    mode.history_state.auto_follow = true;

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::History,
            action: ScrollAction::PageUp(10),
        },
        &mut ctx,
    );
    assert_eq!(mode.history_state.scroll_offset, 70);
    assert!(!mode.history_state.auto_follow);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::History,
            action: ScrollAction::PageDown(200),
        },
        &mut ctx,
    );
    assert_eq!(mode.history_state.scroll_offset, 99);
    assert!(mode.history_state.auto_follow);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::History,
            action: ScrollAction::Home,
        },
        &mut ctx,
    );
    assert_eq!(mode.history_state.scroll_offset, 0);
    assert!(!mode.history_state.auto_follow);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::History,
            action: ScrollAction::End,
        },
        &mut ctx,
    );
    assert_eq!(mode.history_state.scroll_offset, 99);
    assert!(mode.history_state.auto_follow);
    assert!(
        !mode.history_state.turn_in_progress,
        "scroll commands must not dispatch new turns"
    );
}
#[test]
fn test_history_status_and_scroll_use_visual_rows() {
    let mode = TuiMode {
        history_state: HistoryState {
            lines: vec!["a\nb\nc".to_string()],
            ..HistoryState::default()
        },
        ..TuiMode::new()
    };

    assert_eq!(mode.max_scroll_offset(), 2);
    assert!(mode.status_line().contains("history:3"));
}
#[test]
fn header_stable_during_streaming() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    let ready_status = mode.status_line();
    assert!(
        ready_status.contains("mode:ready"),
        "ready state must publish mode token"
    );
    assert!(
        ready_status.contains("approval:none"),
        "ready state must publish approval token"
    );
    assert!(
        ready_status.contains("history:0"),
        "ready state must publish history count"
    );
    assert!(
        ready_status.contains("repo:"),
        "ready state must publish repo token"
    );
    assert_eq!(
        render_pass_order(&mode).first(),
        Some(&RenderPass::Header),
        "header row must remain first in render order"
    );

    mode.on_user_input("hello".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);
    let streaming_status = mode.status_line();
    assert!(
        streaming_status.contains("mode:streaming"),
        "streaming state must publish mode token"
    );
    assert!(
        streaming_status.contains("approval:none"),
        "streaming state must preserve approval token"
    );
    assert!(
        streaming_status.contains("history:2"),
        "streaming state must keep compact history count"
    );
    assert_eq!(
        render_pass_order(&mode).first(),
        Some(&RenderPass::Header),
        "header row must remain first while streaming"
    );

    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );
    let overlay_status = mode.status_line();
    assert!(
        overlay_status.contains("mode:overlay"),
        "overlay state must publish overlay mode token"
    );
    assert!(
        overlay_status.contains("approval:pending"),
        "overlay state must publish pending approval token"
    );
    assert_eq!(
        render_pass_order(&mode).first(),
        Some(&RenderPass::Header),
        "header row must remain first under overlay"
    );
}
#[test]
fn test_stream_delta_ignored_without_active_turn_slot() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_model_update(UiUpdate::StreamDelta("ghost delta".to_string()), &mut ctx);
    assert!(
        mode.history_state.lines.is_empty(),
        "stale stream deltas must be ignored after turn completion/cancel"
    );
}
#[test]
fn test_cancel_pending_blocks_stream_delta_appends() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("hello".to_string(), &mut ctx);
    mode.on_interrupt(&mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("stale".to_string()), &mut ctx);
    assert_eq!(mode.history_state.lines[0], "> hello");
    assert_eq!(mode.history_state.lines[1], "");
}
