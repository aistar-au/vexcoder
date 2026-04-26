use super::*;

#[tokio::test]
async fn tool_approval_accept_deny_and_session_grant_auto_approve() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx: tx,
        }),
        &mut ctx,
    );
    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(
        rx.await.expect("response must resolve"),
        "input '1' must approve"
    );

    let mut ctx2 = setup_ctx();
    let mut mode2 = TuiMode::new();
    let (tx2, rx2) = tokio::sync::oneshot::channel::<bool>();
    mode2.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx: tx2,
        }),
        &mut ctx2,
    );
    mode2.on_user_input("n".to_string(), &mut ctx2);
    assert!(
        !rx2.await.expect("response must resolve"),
        "input 'n' must deny"
    );

    let mut ctx3 = setup_ctx();
    let mut mode3 = TuiMode::new();
    mode3
        .task_doc
        .info
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let (tx3, rx3) = tokio::sync::oneshot::channel::<bool>();
    mode3.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "run_command".to_string(),
            input_preview: "{}".to_string(),
            response_tx: tx3,
        }),
        &mut ctx3,
    );
    assert!(
        rx3.await.expect("response must resolve"),
        "session grant must auto-approve"
    );
}
