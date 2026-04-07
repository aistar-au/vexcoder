use super::*;

#[tokio::test]
async fn test_bang_prefix_runs_without_model_turn_after_approval() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let (mut ctx, mut rx) = setup_ctx_with_updates();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input(successful_bang_input(), &mut ctx);
    assert!(mode.overlay_state.pending_approval.is_some());
    assert!(!mode.is_turn_in_progress());

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(mode.is_turn_in_progress());

    drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

    assert!(mode.overlay_state.pending_approval.is_none());
    assert!(
        !mode.command_session_active(),
        "command session completion should restore normal TUI polling"
    );
    assert!(!mode.is_turn_in_progress());
    assert_eq!(ctx.test_message_count().await, initial_messages);
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[command session started")),
        "expected command session start marker in transcript"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("inline-shell")),
        "expected captured shell output in transcript"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[command session exit: 0]"),
        "expected command session exit status"
    );
}

#[tokio::test]
async fn test_shell_command_runner_invokes_sandbox_wrap() {
    let temp = tempfile::tempdir().unwrap();
    let wrapped = Arc::new(AtomicBool::new(false));
    let result = run_shell_command_with_runner(
        DefaultCommandRunner::new(),
        RecordingSandbox {
            wrapped: Arc::clone(&wrapped),
        },
        "echo sandbox-hit".to_string(),
        temp.path().to_path_buf(),
    )
    .await
    .unwrap();

    assert!(wrapped.load(Ordering::SeqCst));
    assert!(result.stdout.contains("sandbox-hit"));
}

#[tokio::test]
async fn test_shell_command_request_invokes_sandbox_wrap() {
    let temp = tempfile::tempdir().unwrap();
    let wrapped = Arc::new(AtomicBool::new(false));
    let result = run_shell_command_with_runner(
        DefaultCommandRunner::new(),
        RecordingSandbox {
            wrapped: Arc::clone(&wrapped),
        },
        "echo passthrough-hit".to_string(),
        temp.path().to_path_buf(),
    )
    .await
    .unwrap();

    assert!(wrapped.load(Ordering::SeqCst));
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("passthrough-hit"));
}

#[test]
fn test_command_session_updates_track_matching_session() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.begin_turn_capture("test".to_string());
    mode.begin_turn_capture("!first".to_string());
    let first = mode.begin_command_session("first".to_string());
    let second = mode.begin_command_session("second".to_string());

    mode.on_model_update(
        UiUpdate::CommandSessionAttached {
            session_id: second,
            pid: Some(22),
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::CommandSessionAttached {
            session_id: first,
            pid: Some(11),
        },
        &mut ctx,
    );

    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.get(&first).unwrap().pid,
        Some(11)
    );
    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.get(&second).unwrap().pid,
        Some(22)
    );

    mode.on_model_update(
        UiUpdate::CommandSessionFinished { session_id: first },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.len(),
        1
    );
    assert!(mode.is_turn_in_progress());
    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.get(&second).unwrap().command,
        "second"
    );

    mode.on_model_update(
        UiUpdate::CommandSessionFinished { session_id: second },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    assert!(
        mode.task_doc.active_turn.as_ref().map_or(true, |t| t.command_sessions.is_empty())
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_command_session_started_update_creates_running_session() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.begin_turn_capture("test".to_string());
    mode.on_model_update(
        UiUpdate::CommandSessionStarted {
            session_id: 77,
            command: "echo from-tool".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::CommandSessionAttached {
            session_id: 77,
            pid: Some(7700),
        },
        &mut ctx,
    );

    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.len(),
        1
    );
    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.get(&77).unwrap().session_id,
        77
    );
    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.get(&77).unwrap().command,
        "echo from-tool"
    );
    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.get(&77).unwrap().pid,
        Some(7700)
    );
    assert_eq!(
        mode.task_doc.active_turn.as_ref().unwrap().command_sessions.get(&77).unwrap().status,
        "running"
    );
}

#[test]
fn test_turn_complete_waits_for_last_command_session_to_finish() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.begin_turn_capture("test".to_string());
    mode.begin_turn_capture("!echo delayed-finish".to_string());
    let session_id = mode.begin_command_session("echo delayed-finish".to_string());

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    assert!(mode.is_turn_in_progress());
    assert!(mode.turn_completion_pending);

    mode.on_model_update(UiUpdate::CommandSessionFinished { session_id }, &mut ctx);

    assert!(
        mode.task_doc.active_turn.as_ref().map_or(true, |t| t.command_sessions.is_empty())
    );
    assert!(!mode.is_turn_in_progress());
    assert!(!mode.turn_completion_pending);
    assert_eq!(mode.task_doc.meta.status, crate::runtime::TaskStatus::Ready);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_model_run_command_streams_managed_session_into_tui_transcript() {
    let temp = tempfile::tempdir().unwrap();
    let responses = vec![
            vec![
                r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_run_command_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
                r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
                r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Running a command now."}}"#.to_string(),
                r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_run_command_01","name":"run_command","input":{}}}"#.to_string(),
                #[cfg(windows)]
                r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"cmd\",\"args\":[\"/C\",\"echo model-tool-output\"]}"}}"#.to_string(),
                #[cfg(not(windows))]
                r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"sh\",\"args\":[\"-c\",\"printf 'model-tool-output\\n'\"]}"}}"#.to_string(),
                r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
                r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
                r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
            ],
            vec![
                r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_run_command_02","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
                r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
                r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Finished running the command."}}"#.to_string(),
                r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":8}}"#.to_string(),
                r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
            ],
        ];

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    mode.task_doc
        .meta
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let (mut ctx, mut rx) = setup_ctx_with_responses_and_updates(responses);

    mode.on_user_input("run the managed tool command".to_string(), &mut ctx);
    drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

    let lines = mode.history_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[command session started")),
        "expected managed command-session start marker in transcript"
    );
    assert!(
        lines.iter().any(|line| line.contains("model-tool-output")),
        "expected model run_command output in transcript"
    );
    assert!(
        lines.iter().any(|line| line == "[command session exit: 0]"),
        "expected managed command-session exit marker in transcript"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Finished running the command.")),
        "expected final assistant response after tool completion"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_bang_prefix_cancellation_completes_turn() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let (mut ctx, mut rx) = setup_ctx_with_updates();
    let input = if cfg!(windows) {
        "!ping -n 60 127.0.0.1 > nul".to_string()
    } else {
        "!sleep 30".to_string()
    };

    mode.on_user_input(input, &mut ctx);
    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(mode.is_turn_in_progress());

    mode.on_interrupt(&mut ctx);
    drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

    assert!(!mode.is_turn_in_progress());
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[command session cancelled]"),
        "expected cancellation feedback for command sessions"
    );
}
