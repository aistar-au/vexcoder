use super::*;

#[tokio::test]
async fn test_tool_approval_accept_once() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );
    mode.on_user_input("1".to_string(), &mut ctx);

    assert!(response_rx.await.expect("response should resolve"));
}

#[tokio::test]
async fn test_tool_approval_deny() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );
    mode.on_user_input("n".to_string(), &mut ctx);

    assert!(!response_rx.await.expect("response should resolve"));
}

#[tokio::test]
async fn test_tool_approval_auto_approves_matching_session_grant() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "run_command".to_string(),
            input_preview: "{\"tool\":\"write_file\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(response_rx.await.expect("response should resolve"));
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::RunCommand),
        Some(&ApprovalScope::Session),
        "session grant must remain after auto-approval"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "matching grant must not open the approval overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[approval] run_command auto-approved via session grant")),
        "expected paragraph-style auto-approval transcript entry"
    );
}

#[tokio::test]
async fn test_tool_approval_consumes_matching_once_grant() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Once);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(response_rx.await.expect("response should resolve"));
    assert!(
        !mode
            .task_doc
            .info
            .active_grants
            .contains_key(&Capability::ApplyPatch),
        "once grant must be consumed after auto-approval"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "matching once grant must not open the approval overlay"
    );
}

#[tokio::test]
async fn test_tool_approval_prompts_when_grant_does_not_match_tool() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Session);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "run_command".to_string(),
            input_preview: "{\"tool\":\"write_file\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    let mut response_rx = Box::pin(response_rx);
    assert!(
        response_rx.as_mut().now_or_never().is_none(),
        "non-matching grant must leave approval unresolved"
    );
    assert!(
        mode.overlay_state.pending_approval.is_some(),
        "non-matching grant must still open the approval overlay"
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Session),
        "non-matching grant must remain intact"
    );
}

#[tokio::test]
async fn test_tool_approval_updates_task_status_until_turn_resumes() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_user_input("review the plan".to_string(), &mut ctx);
    assert_eq!(
        mode.task_doc.info.status,
        crate::runtime::TaskStatus::Running
    );

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{\"path\":\"src/main.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );
    assert_eq!(
        mode.task_doc.info.status,
        crate::runtime::TaskStatus::AwaitingApproval
    );

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(response_rx.await.expect("approval should resolve"));

    mode.on_model_update(
        UiUpdate::TranscriptLine("[tool] resumed".to_string()),
        &mut ctx,
    );
    assert_eq!(
        mode.task_doc.info.status,
        crate::runtime::TaskStatus::Running
    );
}

#[tokio::test]
async fn test_tool_approval_request_persists_awaiting_approval_status_in_task_state() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path());

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.on_user_input("review the plan".to_string(), &mut ctx);

    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{\"path\":\"src/main.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    let saved =
        crate::runtime::TaskState::load(temp.path(), &mode.task_doc.info.id).expect("saved task");
    assert_eq!(saved.status, crate::runtime::TaskStatus::AwaitingApproval);

    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}
