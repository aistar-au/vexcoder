use super::*;

#[tokio::test]
async fn bang_prefix_runs_shell_without_model_turn_after_approval() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let (mut ctx, mut rx) = setup_ctx_with_updates();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input(successful_bang_input(), &mut ctx);
    assert!(mode.overlay_state.pending_approval.is_some());
    assert!(!mode.is_pulse_in_progress());

    mode.on_user_input("1".to_string(), &mut ctx);
    drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

    assert!(mode.overlay_state.pending_approval.is_none());
    assert!(!mode.is_pulse_in_progress());
    assert_eq!(ctx.test_message_count().await, initial_messages);
    assert!(mode.history_lines().iter().any(|l| l.contains("[command session started")));
    assert!(mode.history_lines().iter().any(|l| l.contains("inline-shell")));
    assert!(mode.history_lines().iter().any(|l| l == "[command session exit: 0]"));
}

#[tokio::test]
async fn shell_command_runner_invokes_sandbox_wrap() {
    let temp = tempfile::tempdir().unwrap();
    let wrapped = Arc::new(AtomicBool::new(false));
    let result = run_shell_command_with_runner(
        DefaultCommandRunner::new(),
        RecordingSandbox { wrapped: Arc::clone(&wrapped) },
        "echo sandbox-hit".to_string(),
        temp.path().to_path_buf(),
    ).await.unwrap();
    assert!(wrapped.load(Ordering::SeqCst), "sandbox must have been invoked");
    assert!(result.output.contains("sandbox-hit"));
}

#[test]
fn command_session_updates_track_matching_session() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("task".to_string(), &mut ctx);
    let session_id = "cmd-session-01";
    mode.on_model_update(UiUpdate::CommandSessionStarted {
        session_id: session_id.to_string(),
        label: "echo test".to_string(),
    }, &mut ctx);
    assert!(mode.command_session_active(), "active session must be tracked");
    mode.on_model_update(UiUpdate::CommandSessionEnded {
        session_id: session_id.to_string(),
        exit_code: 0,
    }, &mut ctx);
    assert!(!mode.command_session_active(), "ended session must deactivate");
}
