use super::*;

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
async fn test_tui_diff_limits_output_at_max_lines() {
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
        .any(|line| line == "[diff limited to first 200 lines]"));
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

// -- /edit & /fix ---------------------------------------------------------

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
    mode.push_document_notice(
        "prior assistant line".to_string(),
        crate::runtime::NoticeSeverity::Info,
    );

    mode.on_user_input("/edit fix the parser bug".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("new output".to_string()), &mut ctx);

    assert_eq!(mode.history_lines()[0], "prior assistant line");
    assert!(
        mode.history_lines()
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
    mode.begin_turn_capture("test".to_string());
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
        crate::runtime::TaskState::load(temp.path(), &mode.task_doc.info.id).expect("saved task");
    assert_eq!(
        mode.task_doc.info.status,
        crate::runtime::TaskStatus::MaxTurnsReached
    );
    assert_eq!(saved.status, crate::runtime::TaskStatus::MaxTurnsReached);

    std::env::remove_var("VEX_STATE_DIR");
}

// -- /explain -------------------------------------------------------------

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
        "turn completion must clear the read-only turn indicator"
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

// -- /review --------------------------------------------------------------

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
        .file_rollups
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

// -- /plan ----------------------------------------------------------------

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
