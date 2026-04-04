use super::*;

#[test]
fn tool_call_only_marks_changed_files_after_successful_result() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("task".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tool-1".to_string(),
                name: "write_file".to_string(),
                input: serde_json::json!({
                    "path": "src/main.rs",
                    "content": "fn main() {}\n"
                }),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    assert!(
        mode.current_turn_changed_files.is_empty(),
        "tool calls should not record changed files until they succeed"
    );

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tool-1".to_string(),
                output: "ok".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );
    assert!(mode.current_turn_changed_files.contains("src/main.rs"));

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.changed_files, vec!["src/main.rs".to_string()]);
}

#[test]
fn failed_tool_result_does_not_record_changed_files() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("task".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tool-1".to_string(),
                name: "write_file".to_string(),
                input: serde_json::json!({
                    "path": "src/main.rs",
                    "content": "fn main() {}\n"
                }),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tool-1".to_string(),
                output: "permission denied".to_string(),
                is_error: true,
            },
        },
        &mut ctx,
    );
    assert!(
        mode.current_turn_changed_files.is_empty(),
        "failed tool calls must not be exported as changed files"
    );
}

#[test]
fn error_reset_clears_live_changed_file_projection() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.current_turn_changed_files
        .insert("src/main.rs".to_string());

    mode.on_model_update(UiUpdate::Error("reset".to_string()), &mut ctx);

    let state = mode.task_layout_state().expect("task layout state");
    assert!(
        state.changed_files.is_empty(),
        "error reset should clear in-flight changed file projection"
    );
}
#[test]
fn test_idle_interrupt_shows_feedback() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    assert!(!mode.history_state.turn_in_progress);
    assert!(!mode.pending_quit);
    assert!(!mode.quit_requested);

    mode.on_interrupt(&mut ctx);
    assert!(mode.pending_quit, "first idle interrupt must arm quit");
    assert!(!mode.quit_requested, "first idle interrupt must not quit");
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line.contains("[press Ctrl+C again to exit]")),
        "first idle interrupt must show user-visible feedback"
    );

    mode.on_interrupt(&mut ctx);
    assert!(
        mode.quit_requested,
        "second idle interrupt must request quit"
    );
    assert!(
        mode.quit_requested(),
        "frontend quit path must observe mode quit request"
    );
}
#[test]
fn test_input_drop_shows_feedback() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.turn_in_progress = true;
    mode.on_user_input("hello".to_string(), &mut ctx);

    assert!(
        mode.history_state.turn_in_progress,
        "busy input must not start a new turn"
    );
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line.starts_with("[busy")),
        "busy input must produce visible rejection feedback"
    );
    assert!(
        !mode
            .history_state
            .lines
            .iter()
            .any(|line| line == "> hello"),
        "discarded busy input must not be appended as user message"
    );
}
#[test]
fn test_pending_quit_resets_on_new_turn_accept() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_interrupt(&mut ctx);
    assert!(mode.pending_quit);

    mode.on_user_input("resume".to_string(), &mut ctx);
    assert!(
        !mode.pending_quit,
        "pending quit must reset when a new turn is accepted"
    );
    assert!(!mode.quit_requested);
    assert!(mode.history_state.turn_in_progress);
}
#[tokio::test]
async fn test_interrupt_is_typed_event_not_magic_string_collision() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();

    mode.on_user_input("__VEX_INTERRUPT__".to_string(), &mut ctx);
    assert!(
        mode.history_state.turn_in_progress,
        "plain text matching old sentinel must be treated as normal user input"
    );

    mode.on_interrupt(&mut ctx);
    assert!(
        mode.history_state.turn_in_progress,
        "typed interrupt should keep turn active until TurnComplete drains"
    );
    assert!(
        mode.history_state.cancel_pending,
        "typed interrupt should arm cancel-pending state"
    );
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line.contains("[turn cancellation requested]")),
        "cancel path should provide visible feedback"
    );

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    assert!(!mode.history_state.turn_in_progress);
    assert!(!mode.history_state.cancel_pending);
}
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
    mode.current_task
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
        mode.current_task.active_grants.get(&Capability::RunCommand),
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
    mode.current_task
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
            .current_task
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
    mode.current_task
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
        mode.current_task.active_grants.get(&Capability::ApplyPatch),
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
        mode.current_task.status,
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
        mode.current_task.status,
        crate::runtime::TaskStatus::AwaitingApproval
    );

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(response_rx.await.expect("approval should resolve"));

    mode.on_model_update(
        UiUpdate::TranscriptLine("[tool] resumed".to_string()),
        &mut ctx,
    );
    assert_eq!(
        mode.current_task.status,
        crate::runtime::TaskStatus::Running
    );
}
#[tokio::test]
async fn test_tool_approval_request_persists_awaiting_approval_status_in_task_state() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path());

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
        crate::runtime::TaskState::load(temp.path(), &mode.current_task.id).expect("saved task");
    assert_eq!(saved.status, crate::runtime::TaskStatus::AwaitingApproval);

    std::env::remove_var("VEX_STATE_DIR");
}
// -- PC-01: /model --------------------------------------------------------

#[tokio::test]
async fn test_model_shows_current_name() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.on_user_input("/model".to_string(), &mut ctx);
    assert!(
        mode.history_lines().iter().any(|l| l.contains("[model]")),
        "bare /model must echo current model"
    );
}
#[tokio::test]
async fn test_model_switches_name() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let old = mode.model_name.clone();
    mode.on_user_input("/model local/coder-8b".to_string(), &mut ctx);
    assert_eq!(mode.model_name, "local/coder-8b");
    assert_eq!(ctx.test_model_name().await, "local/coder-8b");
    assert!(mode
        .history_lines()
        .iter()
        .any(|l| l.contains(&old) && l.contains("local/coder-8b")));
}
#[tokio::test]
async fn test_model_rejects_local_on_api_backend() {
    let mut ctx = setup_ctx();
    let mut config = Config::default_for_tui();
    config.model_backend = crate::runtime::ModelBackendKind::ApiServer;
    config.model_name = "remote-model".to_string();
    let mut mode = TuiMode::new_with_config(None, config);
    // local/ prefix on an ApiServer session must be rejected.
    mode.on_user_input("/model local/phi-3".to_string(), &mut ctx);
    assert_ne!(
        mode.model_name, "local/phi-3",
        "must reject local/ model on api-server backend"
    );
    assert!(mode.history_lines().iter().any(|l| l.contains("rejected")));
    assert_eq!(ctx.test_model_name().await, "mock-model");
}
#[tokio::test]
async fn test_model_rejects_remote_on_local_backend() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let original = mode.model_name.clone();
    mode.on_user_input("/model remote-model".to_string(), &mut ctx);
    assert_eq!(mode.model_name, original);
    assert_eq!(ctx.test_model_name().await, "mock-model");
    assert!(mode.history_lines().iter().any(|l| l.contains("rejected")));
}
#[tokio::test]
async fn test_model_does_not_start_turn() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input("/model".to_string(), &mut ctx);
    assert!(!mode.is_turn_in_progress(), "/model must not start a turn");

    mode.on_user_input("/model local/phi-3".to_string(), &mut ctx);
    assert!(
        !mode.is_turn_in_progress(),
        "/model <n> must not start a turn"
    );
    assert_eq!(ctx.test_message_count().await, initial_messages);
}
// -- PK-07: /diff ---------------------------------------------------------

#[tokio::test]
async fn test_tui_diff_renders_working_tree_diff() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("a.txt"), "hello\n").unwrap();
    git_success(temp.path(), &["add", "a.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("a.txt"), "world\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff".to_string(), &mut ctx);

    let has_diff = mode
        .history_lines()
        .iter()
        .any(|l| l.contains("diff --git") || l.contains("a.txt"));
    assert!(has_diff, "expected git diff output in history");
}
#[tokio::test]
async fn test_tui_diff_staged_flag() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    std::fs::write(temp.path().join("tracked.txt"), "staged\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    std::fs::write(temp.path().join("tracked.txt"), "unstaged\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff --staged".to_string(), &mut ctx);

    let history = mode.history_lines().join("\n");
    assert!(history.contains("tracked.txt"));
    assert!(history.contains("+staged"));
    assert!(!history.contains("+unstaged"));
}
#[tokio::test]
async fn test_tui_diff_non_git_repo() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|l| l == "[diff] not a git repository"));
}
#[tokio::test]
async fn test_tui_diff_clean_working_tree() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("clean.txt"), "clean\n").unwrap();
    git_success(temp.path(), &["add", "clean.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|l| l == "[diff] working tree is clean"));
}
#[tokio::test]
async fn test_tui_diff_truncates_at_max_lines() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    let path = temp.path().join("large.txt");
    std::fs::write(&path, "seed\n").unwrap();
    git_success(temp.path(), &["add", "large.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    let large_body = (0..260)
        .map(|index| format!("line-{index}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, large_body).unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[diff truncated \u{2014} showing first 200 lines]"));
}
#[tokio::test]
async fn test_tui_diff_does_not_start_model_turn() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "clean\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let initial_messages = ctx.test_message_count().await;
    mode.on_user_input("/diff".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/diff must not start a model turn"
    );
    assert_eq!(ctx.test_message_count().await, initial_messages);
}
#[test]
fn test_tui_edit_command_starts_edit_loop() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit fix the parser bug".to_string(), &mut ctx);
    assert!(
        mode.active_edit_loop.is_some(),
        "/edit must set active_edit_loop"
    );
    assert!(
        mode.is_turn_in_progress(),
        "/edit must mark turn_in_progress"
    );
}
#[test]
fn test_tui_edit_command_preserves_prior_history_line() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.history_state
        .lines
        .push("prior assistant line".to_string());

    mode.on_user_input("/edit fix the parser bug".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("new output".to_string()), &mut ctx);

    assert_eq!(mode.history_state.lines[0], "prior assistant line");
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line.contains("new output")),
        "stream output must target the fresh placeholder line"
    );
}
#[test]
fn test_tui_fix_without_prior_loop_emits_guidance() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/fix".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[no recent validation failure in this session")),
        "expected guidance message when no prior loop exists"
    );
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_tui_fix_during_active_edit_emits_reentrancy_guard() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.active_edit_loop = Some(EditLoop::new("task-existing".to_string()));
    mode.history_state.turn_in_progress = true;
    mode.on_user_input("/fix".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[edit loop already active")),
        "expected reentrancy guard message"
    );
}
#[test]
fn test_tui_edit_empty_instruction_emits_usage() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[edit] usage: /edit <instruction>")),
        "expected usage hint when /edit called without instruction"
    );
    assert!(!mode.is_turn_in_progress());
    assert!(mode.active_edit_loop.is_none());
}
#[test]
fn test_tui_edit_loop_completion_clears_busy_state() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit refactor the parser".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::EditLoopComplete {
            outcome: EditLoopOutcome::MaxTurnsReached { last_error: None },
            last_validation_result: None,
        },
        &mut ctx,
    );

    assert!(!mode.is_turn_in_progress());
    assert!(mode.history_state.active_assistant_index.is_none());
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[edit loop reached max turns]")),
        "expected loop completion summary"
    );
}
#[test]
fn test_tui_edit_loop_completion_persists_max_turn_status_in_task_state() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path());

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit refactor the parser".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::EditLoopComplete {
            outcome: EditLoopOutcome::MaxTurnsReached { last_error: None },
            last_validation_result: None,
        },
        &mut ctx,
    );

    let saved =
        crate::runtime::TaskState::load(temp.path(), &mode.current_task.id).expect("saved task");
    assert_eq!(
        mode.current_task.status,
        crate::runtime::TaskStatus::MaxTurnsReached
    );
    assert_eq!(saved.status, crate::runtime::TaskStatus::MaxTurnsReached);

    std::env::remove_var("VEX_STATE_DIR");
}
#[tokio::test]
async fn test_tui_explain_does_not_invoke_edit_loop() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Explained\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);

    mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/explain").await;

    assert!(
        mode.active_edit_loop.is_none(),
        "/explain must not invoke EditLoop"
    );
    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("Explain the relevant code for the request below.")
                && prompt.contains("Request:\nexplain src/app.rs")
        }),
        "/explain must render the explain template prompt"
    );
}
#[tokio::test]
async fn test_tui_explain_silently_denies_tool_approval_requests() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);

    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(
        !response_rx.await.expect("response should resolve"),
        "/explain must silently deny approval-requiring tool calls"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "/explain must not surface the approval overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[tool approval requested:")),
        "/explain denial should stay silent in transcript output"
    );
}
#[tokio::test]
async fn test_read_only_turn_flag_clears_after_turn_completion() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);
    assert!(
        mode.read_only_turn_active,
        "/explain must mark the active turn as read-only"
    );

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    assert!(
        !mode.read_only_turn_active,
        "turn completion must clear the read-only turn flag"
    );

    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    let mut response_rx = Box::pin(response_rx);
    assert!(
        response_rx.as_mut().now_or_never().is_none(),
        "normal turns must keep approval unresolved until operator input"
    );
    assert!(
        mode.overlay_state.pending_approval.is_some(),
        "normal turns must restore the approval overlay"
    );
}
#[tokio::test]
async fn test_tui_review_default_assembles_head_diff() {
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Reviewed\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "hello\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("tracked.txt"), "world\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/review").await;

    assert!(
        mode.active_edit_loop.is_none(),
        "/review must not invoke EditLoop"
    );
    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("Review the implementation described below.")
                && prompt.contains(
                    "Review these changes for correctness, clarity, and potential issues.",
                )
                && prompt.contains("Diff context:\n")
                && prompt.contains("diff --git")
                && prompt.contains("tracked.txt")
        }),
        "/review must render the review prompt with git diff context"
    );
}
#[tokio::test]
async fn test_tui_review_base_flag_validates_ref() {
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Reviewed\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("tracked.txt"), "changed\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "change"]);

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review --base HEAD~1 inspect".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/review --base").await;

    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("Request:\ninspect")
                && prompt.contains("diff --git")
                && prompt.contains("tracked.txt")
                && prompt.contains("+changed")
        }),
        "/review --base must start a turn with the requested diff"
    );
}
#[tokio::test]
async fn test_tui_review_invalid_ref_emits_error_no_turn() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let initial_messages = ctx.test_message_count().await;
    mode.on_user_input("/review --base missing-ref".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[review: invalid base ref 'missing-ref']"));
    assert!(
        !mode.is_turn_in_progress(),
        "invalid /review base refs must not start a turn"
    );
    assert_eq!(ctx.test_message_count().await, initial_messages);
}
#[tokio::test]
async fn test_tui_review_mutual_exclusion_base_and_files() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input(
        "/review --base HEAD --files src/*.rs inspect".to_string(),
        &mut ctx,
    );

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[review: --base and --files are mutually exclusive]"));
    assert!(!mode.is_turn_in_progress());
    assert_eq!(ctx.test_message_count().await, initial_messages);
}
#[tokio::test]
async fn test_tui_review_drops_pending_patch_silently() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "hello\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("tracked.txt"), "world\n").unwrap();

    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review".to_string(), &mut ctx);

    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"tracked.txt\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(
        !response_rx.await.expect("response should resolve"),
        "/review must silently deny approval-requiring tool calls"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "/review must not surface the approval overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[tool approval requested:")),
        "/review denial should stay silent in transcript output"
    );
}
#[tokio::test]
async fn test_tui_review_files_flag_uses_context_assembler() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Reviewed\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review --files @src/*.rs inspect".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/review --files").await;

    let assembled = mode
        .last_assembled_context
        .as_ref()
        .expect("/review --files must capture assembled context");
    assert!(assembled
        .file_snapshots
        .iter()
        .any(|snapshot| snapshot.path == std::path::Path::new("src/lib.rs")));
    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("[review files] pattern: src/*.rs")
                && prompt.contains("src/lib.rs")
                && prompt.contains("pub fn answer() -> i32 { 42 }")
                && !prompt.contains("@src/*.rs")
        }),
        "/review --files must render assembled file context"
    );
}
#[tokio::test]
async fn test_tui_review_expands_at_path_inside_instruction() {
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Reviewed\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "hello\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("tracked.txt"), "world\n").unwrap();
    std::fs::write(temp.path().join("note.txt"), "review context\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review inspect @note.txt carefully".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/review instruction context").await;

    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("[file: note.txt]")
                && prompt.contains("review context")
                && !prompt.contains("inspect @note.txt carefully")
        }),
        "/review must expand @path mentions inside the free-form instruction"
    );
}
#[tokio::test]
async fn test_tui_plan_starts_single_turn_no_loop() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Plan\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);

    mode.on_user_input("/plan implement feature X".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/plan").await;

    assert!(
        mode.active_edit_loop.is_none(),
        "/plan must not invoke EditLoop"
    );
    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("Create a concise implementation plan for the request below.")
                && prompt.contains(
                    "Request:
implement feature X",
                )
        }),
        "/plan must render the plan template prompt"
    );
}
#[tokio::test]
async fn test_tui_plan_drops_pending_patch_silently() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/plan implement feature X".to_string(), &mut ctx);

    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"src/lib.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(
        !response_rx.await.expect("response should resolve"),
        "/plan must silently deny approval-requiring tool calls"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "/plan must not surface the approval overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[tool approval requested:")),
        "/plan denial should stay silent in transcript output"
    );
}
#[tokio::test]
async fn test_tui_plan_scope_populated_from_assembler() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Plan\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/plan implement feature X".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/plan scope").await;

    assert!(
        mode.last_assembled_context.is_some(),
        "/plan must populate last_assembled_context via ContextAssembler"
    );
}
#[test]
fn test_edit_command_grants_task_permissions() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/edit refactor src/app.rs".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Task)
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Task)
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::RunCommand),
        Some(&ApprovalScope::Task)
    );
}
#[test]
fn test_edit_command_preserves_session_permissions() {
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::WriteFile, ApprovalScope::Session);
    mode.current_task
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Session);
    mode.current_task
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let mut ctx = setup_ctx();

    mode.on_user_input("/edit refactor src/app.rs".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Session)
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Session)
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::RunCommand),
        Some(&ApprovalScope::Session)
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[permissions: /edit task grants")),
        "/edit must not announce a task grant when the capability is already session-scoped"
    );
}
#[test]
fn test_fix_command_grants_task_permissions() {
    let mut mode = TuiMode::new();
    let mut edit_loop = EditLoop::new("task-1".to_string());
    edit_loop.set_last_validation_result(crate::runtime::ValidationResult {
        passed: false,
        outputs: vec![crate::runtime::ValidationOutput {
            label: "cargo test".to_string(),
            exit_code: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }],
    });
    mode.active_edit_loop = Some(edit_loop);
    let mut ctx = setup_ctx();

    mode.on_user_input("/fix".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Task)
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Task)
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::RunCommand),
        Some(&ApprovalScope::Task)
    );
}
#[test]
fn test_fix_command_preserves_session_permissions() {
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::WriteFile, ApprovalScope::Session);
    mode.current_task
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Session);
    mode.current_task
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let mut edit_loop = EditLoop::new("task-1".to_string());
    edit_loop.set_last_validation_result(crate::runtime::ValidationResult {
        passed: false,
        outputs: vec![crate::runtime::ValidationOutput {
            label: "cargo test".to_string(),
            exit_code: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }],
    });
    mode.active_edit_loop = Some(edit_loop);
    let mut ctx = setup_ctx();

    mode.on_user_input("/fix".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Session)
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Session)
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::RunCommand),
        Some(&ApprovalScope::Session)
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[permissions: /fix task grants")),
        "/fix must not announce a task grant when the capability is already session-scoped"
    );
}
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

    mode.history_state.turn_in_progress = true;
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

    assert_eq!(mode.command_sessions[0].pid, Some(11));
    assert_eq!(mode.command_sessions[1].pid, Some(22));

    mode.on_model_update(
        UiUpdate::CommandSessionFinished { session_id: first },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    assert_eq!(mode.command_sessions.len(), 1);
    assert!(mode.is_turn_in_progress());
    assert_eq!(mode.command_sessions[0].command, "second");

    mode.on_model_update(
        UiUpdate::CommandSessionFinished { session_id: second },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    assert!(mode.command_sessions.is_empty());
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_command_session_started_update_creates_running_session() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.turn_in_progress = true;
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

    assert_eq!(mode.command_sessions.len(), 1);
    assert_eq!(mode.command_sessions[0].id, 77);
    assert_eq!(mode.command_sessions[0].command, "echo from-tool");
    assert_eq!(mode.command_sessions[0].pid, Some(7700));
    assert_eq!(mode.command_sessions[0].status, "running");
}
#[test]
fn test_turn_complete_waits_for_last_command_session_to_finish() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.turn_in_progress = true;
    mode.begin_turn_capture("!echo delayed-finish".to_string());
    let session_id = mode.begin_command_session("echo delayed-finish".to_string());

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    assert!(mode.is_turn_in_progress());
    assert!(mode.turn_completion_pending);

    mode.on_model_update(UiUpdate::CommandSessionFinished { session_id }, &mut ctx);

    assert!(mode.command_sessions.is_empty());
    assert!(!mode.is_turn_in_progress());
    assert!(!mode.turn_completion_pending);
    assert_eq!(
        mode.current_task.status,
        crate::runtime::TaskStatus::Completed
    );
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
    mode.current_task
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

#[test]
fn test_waiting_indicator_appears_on_turn_start() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("hello world".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[thinking] Mapping adjacent sectors..."),
        "turn start must show waiting indicator"
    );
}

#[test]
fn test_waiting_indicator_cleared_on_stream_delta() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("test prompt".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[thinking] Mapping adjacent sectors..."),
        "waiting indicator must be present before first delta"
    );

    mode.on_model_update(UiUpdate::StreamDelta("Hello".to_string()), &mut ctx);

    assert!(
        !mode
            .history_lines()
            .iter()
            .any(|line| line.contains("[thinking] Mapping adjacent sectors...")),
        "waiting indicator must be cleared after first stream delta"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("Hello")),
        "first stream delta content must be visible"
    );
}

#[test]
fn test_waiting_indicator_cleared_on_tool_block() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("test prompt".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-1".to_string(),
                name: "list_files".to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );

    assert!(
        !mode
            .history_lines()
            .iter()
            .any(|line| line == "[thinking] Mapping adjacent sectors..."),
        "waiting indicator must be cleared when a tool block starts"
    );
}

#[test]
fn test_verb_first_read_file_empty_path() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("read something".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-read".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-read".to_string(),
                output: "I need an explicit file path".to_string(),
                is_error: true,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[tool] read_file · failed"),
        "read_file with empty path and error must show paragraph tool header"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[detail] Result: I need an explicit file path")),
        "read_file with empty path and error must preserve the result summary"
    );
}

#[test]
fn test_verb_first_list_files_empty_path() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("list files".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-list".to_string(),
                name: "list_files".to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-list".to_string(),
                output: "src/main.rs\nCargo.toml\n".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.starts_with("[tool] list_files · ")),
        "list_files with empty path must show a paragraph tool header"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[evidence] src/main.rs")
            && mode
                .history_lines()
                .iter()
                .any(|line| line == "[evidence] Cargo.toml"),
        "list_files with empty path must retain the listed workspace evidence"
    );
}

#[test]
fn test_tool_blocks_emit_paragraph_rows_into_history() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect the file".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-read".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path":"src/main.rs"}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[tool] read_file · src/main.rs · Mapping adjacent sectors..."),
        "pending tool calls must render into the scrolling transcript"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| { line == "[detail] Input: path: src/main.rs" }),
        "pending tool calls must preserve the compact input preview"
    );

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-read".to_string(),
                output: "42 lines read from src/main.rs\nfn main() {}".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[tool] read_file · src/main.rs · Response complete."),
        "completed tool calls must render their terminal paragraph header"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[evidence] fn main() {}"),
        "completed tool calls must keep enriched evidence in the transcript"
    );
}
#[test]
fn test_stream_block_delta_updates_pending_tool_call_input() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.turn_in_progress = true;

    // Start a ToolCall block with empty input.
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::Value::Object(Default::default()),
                status: crate::state::ToolStatus::Pending,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.pending_turn_tool_calls.contains_key("tc1"),
        "StreamBlockStart must register pending tool call"
    );

    // First partial delta — not yet valid JSON.
    mode.on_model_update(
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: r#"{"path":"#.to_string(),
        },
        &mut ctx,
    );
    // Input should remain the initial empty object, but the preview should
    // surface the streamed fragment while the JSON is incomplete.
    assert_eq!(
        mode.pending_turn_tool_calls["tc1"].input,
        serde_json::Value::Object(Default::default()),
        "partial delta must not update pending tool call input"
    );
    assert!(
        mode.pending_turn_tool_calls["tc1"]
            .input_preview
            .contains(r#"{"path":"#),
        "partial delta must update the pending tool call preview"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("partial streamed input")),
        "partial delta must replace the pending transcript rows with the streamed preview"
    );

    // Second delta completes the JSON.
    mode.on_model_update(
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: r#""foo.rs"}"#.to_string(),
        },
        &mut ctx,
    );
    assert_eq!(
        mode.pending_turn_tool_calls["tc1"].input,
        serde_json::json!({"path": "foo.rs"}),
        "complete delta must update pending tool call input with parsed value"
    );
    assert!(
        mode.pending_turn_tool_calls["tc1"]
            .input_preview
            .contains("foo.rs"),
        "complete delta must replace the partial preview with the parsed preview"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[detail] Input: path: foo.rs"),
        "complete delta must replace the pending transcript rows with the parsed preview"
    );
}

// -- duplicate tool-call folding -------------------------------------------------

fn tool_call_start(index: usize, id: &str, name: &str, input: serde_json::Value) -> UiUpdate {
    UiUpdate::StreamBlockStart {
        index,
        block: StreamBlock::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input,
            status: crate::state::ToolStatus::Executing,
        },
    }
}

fn tool_result(index: usize, tool_call_id: &str, output: &str) -> UiUpdate {
    UiUpdate::StreamBlockStart {
        index,
        block: StreamBlock::ToolResult {
            tool_call_id: tool_call_id.to_string(),
            output: output.to_string(),
            is_error: false,
        },
    }
}

#[test]
fn test_duplicate_tool_calls_fold_to_repeated_indicator() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("task".to_string(), &mut ctx);

    let input = serde_json::json!({"path": "src/main.rs"});

    // First call
    mode.on_model_update(
        tool_call_start(0, "t1", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "fn main() {}"), &mut ctx);
    // Second identical call
    mode.on_model_update(
        tool_call_start(2, "t2", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(3, "t2", "fn main() {}"), &mut ctx);

    let lines = &mode.history_state.lines;
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("[detail] (repeated 2\u{d7}, same call)")),
        "expected folded-duplicate indicator in history; got:\n{:#?}",
        lines
    );
    assert_eq!(
        mode.duplicate_tool_count, 2,
        "duplicate_tool_count should be 2 after two identical consecutive tool calls"
    );
}

#[test]
fn test_different_consecutive_tool_calls_not_folded() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("task".to_string(), &mut ctx);

    // First call: read_file
    mode.on_model_update(
        tool_call_start(0, "t1", "read_file", serde_json::json!({"path": "a.rs"})),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "content of a"), &mut ctx);

    // Second call: different tool
    mode.on_model_update(
        tool_call_start(
            2,
            "t2",
            "write_file",
            serde_json::json!({"path": "b.rs", "content": "x"}),
        ),
        &mut ctx,
    );
    mode.on_model_update(tool_result(3, "t2", "written"), &mut ctx);

    let lines = &mode.history_state.lines;
    assert!(
        !lines.iter().any(|l| l.starts_with("[detail] (repeated")),
        "different tool calls must not produce a folded-duplicate line; got:\n{:#?}",
        lines
    );
    // Each unique tool result must produce its own completed [tool] header row.
    // Pending rows also emit [tool] headers, so we expect 4 total (2 pending + 2 completed).
    let tool_headers: Vec<_> = lines.iter().filter(|l| l.starts_with("[tool]")).collect();
    assert!(
        tool_headers.len() >= 2,
        "expected at least two [tool] header lines for different tool calls; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_tool_fold_state_resets_after_turn_ends() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    // Turn 1: complete a tool call.
    mode.on_user_input("first".to_string(), &mut ctx);
    let input = serde_json::json!({"path": "src/main.rs"});
    mode.on_model_update(
        tool_call_start(0, "t1", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "content"), &mut ctx);
    assert_eq!(
        mode.duplicate_tool_count, 1,
        "count should be 1 after first completion"
    );

    // End the turn via Error (causes reset_turn_capture).
    mode.on_model_update(UiUpdate::Error("test reset".to_string()), &mut ctx);
    assert!(
        mode.last_completed_tool_header.is_none(),
        "last_completed_tool_header must be cleared after turn reset"
    );

    // Turn 2: same tool call in a fresh turn must not fold.
    mode.on_user_input("second".to_string(), &mut ctx);
    mode.on_model_update(
        tool_call_start(0, "t2", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t2", "content"), &mut ctx);

    let lines = &mode.history_state.lines;
    assert!(
        !lines.iter().any(|l| l.starts_with("[detail] (repeated")),
        "tool call in new turn must not fold against previous turn; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_same_name_different_target_tool_calls_fold_into_paragraph() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("list project".to_string(), &mut ctx);

    // First list_files call for path "src"
    mode.on_model_update(
        tool_call_start(0, "t1", "list_files", serde_json::json!({"path": "src"})),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "src/main.rs\nsrc/lib.rs"), &mut ctx);

    // Second list_files call for a different path "tests"
    mode.on_model_update(
        tool_call_start(2, "t2", "list_files", serde_json::json!({"path": "tests"})),
        &mut ctx,
    );
    mode.on_model_update(tool_result(3, "t2", "tests/integration.rs"), &mut ctx);

    let lines = &mode.history_state.lines;

    // The second completed call must NOT produce its own [tool] header —
    // it should be folded into the paragraph started by the first call.
    // Expect exactly 2 completed [tool] headers: 1 pending for t1 (before
    // its result), 1 completed for t1, and t2's pending and completed
    // headers are both folded. The pending header for t1 is emitted before
    // t2 exists, so 2 pending + 1 completed = 3 max.
    let tool_headers: Vec<_> = lines
        .iter()
        .filter(|l| l.starts_with("[tool] list_files"))
        .collect();
    assert!(
        tool_headers.len() <= 3,
        "same-name tool calls must fold; expected <= 3 [tool] headers \
         (1 pending + 1 completed + 1 pending), got {}:\n{:#?}",
        tool_headers.len(),
        lines
    );

    // Must show a batch count annotation.
    assert!(
        lines.iter().any(|l| l.contains("2 calls")),
        "folded same-name tool calls must show a batch count; got:\n{:#?}",
        lines
    );

    // Both results must have their evidence rows in the transcript.
    assert!(
        lines.iter().any(|l| l.contains("src/main.rs")),
        "first tool result evidence must be preserved; got:\n{:#?}",
        lines
    );
    assert!(
        lines.iter().any(|l| l.contains("tests/integration.rs")),
        "second tool result evidence must be preserved after folding; got:\n{:#?}",
        lines
    );

    assert_eq!(
        mode.same_name_tool_count, 2,
        "same_name_tool_count should be 2 after two list_files calls with different paths"
    );
}

#[test]
fn test_long_transcript_line_truncated() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("task".to_string(), &mut ctx);

    // Simulate a very long transcript line (e.g. large git diff output).
    let long_line = "x".repeat(1024);
    mode.on_model_update(UiUpdate::TranscriptLine(long_line.clone()), &mut ctx);

    let lines = &mode.history_state.lines;
    let last = lines.last().expect("should have at least one line");
    assert!(
        last.len() < long_line.len(),
        "transcript lines exceeding 512 chars must be truncated; got {} chars",
        last.len()
    );
    assert!(
        last.contains("truncated"),
        "truncated transcript line must indicate truncation; got: {}",
        &last[last.len().saturating_sub(60)..]
    );
}

#[test]
fn test_edit_loop_lines_rendered_as_transcript() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("edit task".to_string(), &mut ctx);

    // Edit loop turn markers must be stored in history.
    mode.on_model_update(
        UiUpdate::TranscriptLine("[edit loop turn 1/6]".to_string()),
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::TranscriptLine("[edit loop: running validation]".to_string()),
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::TranscriptLine("[edit loop: validation passed]".to_string()),
        &mut ctx,
    );

    let lines = &mode.history_state.lines;
    assert!(
        lines.iter().any(|l| l.contains("edit loop turn 1/6")),
        "edit loop turn marker must appear in transcript history"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("edit loop: validation passed")),
        "edit loop validation status must appear in transcript history"
    );
}

#[test]
fn test_edit_loop_warning_preserved_in_transcript() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("edit task".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::TranscriptLine(
            "[edit loop warning: workspace has uncommitted changes; proceeding without mutating git state]"
                .to_string(),
        ),
        &mut ctx,
    );

    let lines = &mode.history_state.lines;
    assert!(
        lines
            .iter()
            .any(|l| l.contains("edit loop warning: workspace has uncommitted changes")),
        "edit loop warning must appear in transcript history; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_edit_loop_turn_error_preserved_in_transcript() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("edit task".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::TranscriptLine("[edit loop turn error: connection timeout]".to_string()),
        &mut ctx,
    );

    let lines = &mode.history_state.lines;
    assert!(
        lines
            .iter()
            .any(|l| l.contains("edit loop turn error: connection timeout")),
        "edit loop turn error must appear in transcript history; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_edit_loop_complete_emits_telemetry_line() {
    use crate::runtime::edit_loop::EditLoopOutcome;
    use crate::types::StreamTimings;
    use std::time::Instant;

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("edit task".to_string(), &mut ctx);

    // Simulate server metadata arriving during the edit loop turn.
    mode.turn_started_at = Some(Instant::now());
    mode.ttft = Some(std::time::Duration::from_millis(200));
    mode.current_turn_timings = Some(StreamTimings {
        prompt_ms: Some(500.0),
        prompt_n: Some(100),
        predicted_ms: Some(1500.0),
        predicted_n: Some(50),
        ..Default::default()
    });

    // EditLoopComplete should capture and emit telemetry.
    mode.on_model_update(
        UiUpdate::EditLoopComplete {
            outcome: EditLoopOutcome::Success {
                patch_applied: true,
                validate_passed: true,
            },
            last_validation_result: None,
        },
        &mut ctx,
    );

    let lines = &mode.history_state.lines;
    // Must contain the timing summary line.
    assert!(
        lines
            .iter()
            .any(|l| l.contains("total:") && l.contains("ttft:")),
        "EditLoopComplete must emit a telemetry summary line; got:\n{:#?}",
        lines
    );
}
