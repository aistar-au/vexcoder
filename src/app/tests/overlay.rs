use super::*;

#[tokio::test]
async fn test_ref_03_tui_mode_overlay_blocks_input() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();

    let (response_tx, _rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    mode.on_user_input("blocked".to_string(), &mut ctx);
    assert!(
        mode.task_doc.active_turn.is_none(),
        "overlay must block input dispatch"
    );

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(
        !mode.overlay_active(),
        "overlay should clear after decision"
    );

    mode.on_user_input("resume".to_string(), &mut ctx);
    assert!(
        mode.task_doc.active_turn.is_some(),
        "dispatch should resume after overlay clears"
    );
}

#[test]
fn resume_selection_overlay_routes_numeric_input_to_task_resume() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let saved = TaskState::new("task-resume-overlay".to_string());
    saved.save(temp.path()).unwrap();

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.prompt_resume_selection(vec![ResumeTaskEntry {
        dir: temp.path().to_path_buf(),
        id: "task-resume-overlay".to_string(),
        status: "Ready".to_string(),
    }]);

    mode.on_user_input("1".to_string(), &mut ctx);

    assert_eq!(mode.current_task_id(), "task-resume-overlay");
    assert!(
        !mode.overlay_active(),
        "resume overlay should clear on selection"
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn approval_selection_parser_handles_shared_overlay_inputs() {
    assert_eq!(
        parse_approval_selection("1"),
        Some(ApprovalSelection::ApproveOnce)
    );
    assert_eq!(
        parse_approval_selection("yes"),
        Some(ApprovalSelection::ApproveOnce)
    );
    assert_eq!(
        parse_approval_selection("2"),
        Some(ApprovalSelection::ApproveSession)
    );
    assert_eq!(
        parse_approval_selection("always"),
        Some(ApprovalSelection::ApproveSession)
    );
    assert_eq!(parse_approval_selection("3"), Some(ApprovalSelection::Deny));
    assert_eq!(
        parse_approval_selection("esc"),
        Some(ApprovalSelection::Deny)
    );
    assert_eq!(parse_approval_selection("later"), None);
}
#[test]
fn test_composer_focus_depends_on_overlays_not_scroll_offsets() {
    let mut mode = TuiMode::new();
    assert!(mode.composer_is_focused());

    mode.timeline_follow_mode = false;
    mode.transcript_scroll_offset = 3;
    mode.inspector_scroll_offset = 2;
    assert!(mode.composer_is_focused());

    mode.overlay_state.pending_memory_clear = true;
    assert!(!mode.composer_is_focused());
}

#[test]
fn force_config_enables_session_auto_approval_at_startup() {
    let mut config = Config::default_for_tui();
    config.force = true;

    let mode = TuiMode::new_with_config(None, config);
    assert!(mode.overlay_state.auto_approve_session);
}

#[test]
fn expand_context_config_raises_context_scan_cap() {
    let mut config = Config::default_for_tui();
    config.expand_context = true;

    let mode = TuiMode::new_with_config(None, config);
    assert!(mode.context_assembler.max_related > ContextAssembler::default().max_related);
}

#[tokio::test]
async fn diff_overlay_scrolls() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    let patch_preview = [
        "@@ -1,3 +1,4".to_string(),
        " context line".to_string(),
        "-old value".to_string(),
        "+new value".to_string(),
        " context suffix".to_string(),
        "-removed again".to_string(),
        "+added again".to_string(),
    ]
    .join("\n");

    let (approve_tx, approve_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.overlay_state.pending_patch_approval = Some(PendingPatchApproval {
        patch_preview: patch_preview.clone(),
        scroll_offset: 0,
        response_tx: Some(approve_tx),
    });

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Overlay,
            action: ScrollAction::LineDown,
        },
        &mut ctx,
    );
    assert_eq!(
        mode.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|p| p.scroll_offset),
        Some(1),
        "down must advance diff overlay scroll"
    );

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Overlay,
            action: ScrollAction::PageDown(3),
        },
        &mut ctx,
    );
    assert_eq!(
        mode.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|p| p.scroll_offset),
        Some(4),
        "page down must advance by requested step"
    );

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Overlay,
            action: ScrollAction::End,
        },
        &mut ctx,
    );
    assert_eq!(
        mode.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|p| p.scroll_offset),
        Some(patch_preview.lines().count().saturating_sub(1)),
        "end must jump to last diff line"
    );

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(
        approve_rx.await.expect("patch approval should resolve"),
        "approve binding must resolve true"
    );
    assert!(
        !mode.patch_overlay_active(),
        "overlay must clear after approve decision"
    );

    let (deny_tx, deny_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.overlay_state.pending_patch_approval = Some(PendingPatchApproval {
        patch_preview,
        scroll_offset: 2,
        response_tx: Some(deny_tx),
    });
    mode.on_user_input("n".to_string(), &mut ctx);
    assert!(
        !deny_rx.await.expect("patch denial should resolve"),
        "deny binding must resolve false"
    );
    assert!(
        !mode.patch_overlay_active(),
        "overlay must clear after deny decision"
    );
}
#[tokio::test]
async fn test_invalid_approval_input_keeps_overlay_active_with_feedback() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    mode.on_user_input("x".to_string(), &mut ctx);
    assert!(
        mode.overlay_active(),
        "overlay should stay active on invalid input"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[invalid selection, expected 1/2/3]")),
        "expected invalid selection feedback line"
    );
}
#[tokio::test]
async fn approval_sender_resolved_exactly_once() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();

    let (first_tx, first_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "first".to_string(),
            response_tx: first_tx,
        }),
        &mut ctx,
    );

    let mut first_rx = Box::pin(first_rx);
    assert!(
        first_rx.as_mut().now_or_never().is_none(),
        "first approval sender must remain unresolved while overlay is active"
    );

    let (second_tx, second_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "write_file".to_string(),
            input_preview: "second".to_string(),
            response_tx: second_tx,
        }),
        &mut ctx,
    );

    assert!(
        !first_rx
            .await
            .expect("first sender should resolve when replaced"),
        "replaced approval sender must resolve false exactly once"
    );

    let mut second_rx = Box::pin(second_rx);
    assert!(
        second_rx.as_mut().now_or_never().is_none(),
        "second approval sender must remain unresolved before decision"
    );

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(
        second_rx
            .await
            .expect("second sender should resolve on accept"),
        "approved overlay should resolve true exactly once"
    );

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    mode.on_model_update(UiUpdate::Error("post-resolution".to_string()), &mut ctx);
    assert!(
        !mode.overlay_active(),
        "overlay lifecycle should clear cleanly after sender resolution"
    );
}
