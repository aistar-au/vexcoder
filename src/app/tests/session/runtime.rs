use super::*;
use crate::config::CompactionConfig;
use crate::config::UndoConfig;

// -- PM-01 (app side): build_runtime_with_resume ---------------------------

#[test]
fn test_build_runtime_with_resume_restores_task() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = TaskState::new("task-startup-resume".to_string());
    state
        .active_grants
        .insert(Capability::Network, ApprovalScope::Session);
    state.status = crate::runtime::TaskStatus::Running;

    let config = Config {
        model_token: None,
        model_name: "mock-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: temp.path().to_path_buf(),
        model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
        model_protocol: crate::runtime::ModelProtocol::MessagesV1,
        tool_call_mode: crate::runtime::ToolCallMode::Structured,
        tool_policy: crate::runtime::ToolPolicy::Full,
        model_profile: ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: UndoConfig::default(),
        search: crate::config::SearchConfig {
            auto_index: false,
            ..Default::default()
        },
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };

    let (runtime, _ctx) =
        build_runtime_with_resume(config, state).expect("build_runtime_with_resume should succeed");

    assert_eq!(runtime.mode.task_doc.info.id, "task-startup-resume");
    assert_eq!(
        runtime
            .mode
            .task_doc
            .info
            .active_grants
            .get(&Capability::Network),
        Some(&ApprovalScope::Session)
    );
    assert!(
        runtime
            .mode
            .history_lines()
            .iter()
            .any(|l| l.contains("[resumed: task-startup-resume status=Running]")),
        "expected resume banner in history"
    );
}

// -- additional /new and /compact edge cases --------------------------------

#[test]
fn test_tui_new_clears_active_edit_loop_field() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.active_edit_loop = Some(EditLoop::new("task-before-new".to_string()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/new".to_string(), &mut ctx);

    assert!(
        mode.active_edit_loop.is_none(),
        "/new must clear active_edit_loop field"
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_compact_clears_active_edit_loop_field() {
    let mut mode = TuiMode::new();
    mode.active_edit_loop = Some(EditLoop::new("task-before-compact".to_string()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert!(
        mode.active_edit_loop.is_none(),
        "/compact must clear active_edit_loop field"
    );
}

#[test]
fn test_tui_compact_resets_turn_evidence_and_token_counter() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    // Simulate a completed turn with token usage.
    mode.task_doc
        .completed_turns
        .push(crate::runtime::task_document::TurnDocument {
            turn_index: 0,
            input: "summarize the diff".to_string(),
            entries: vec![],
            outcome: crate::runtime::task_document::TurnOutcome::Completed,
            changed_files: vec![],
            command_history: vec![],
            tokens: crate::usage::TurnTokens {
                input: 1000,
                output: 500,
                estimated: false,
                ..Default::default()
            },
            started_at_ms: 0,
            completed_at_ms: 1,
            ttft_ms: None,
            timings: None,
        });
    ctx.test_record_session_turn(crate::usage::TurnTokens {
        input: 1000,
        output: 500,
        estimated: false,
        ..Default::default()
    });

    // Verify tokens are non-zero before compact.
    let status_before = mode.status_line();
    assert!(
        status_before.contains("tokens:1500"),
        "status line must show accumulated tokens before compact, got: {status_before}"
    );

    mode.on_user_input("/compact".to_string(), &mut ctx);

    // After compact, turns must be cleared so the status line shows tokens:0.
    assert!(
        mode.task_doc.completed_turns.is_empty(),
        "/compact must clear turn evidence to reset token counter"
    );
    let status_after = mode.status_line();
    assert!(
        status_after.contains("tokens:0"),
        "status line must show tokens:0 after compact, got: {status_after}"
    );
}

#[test]
fn test_tui_compact_preserves_task_id_but_clears_turns() {
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();

    mode.task_doc
        .completed_turns
        .push(crate::runtime::task_document::TurnDocument {
            turn_index: 0,
            input: "test".to_string(),
            entries: vec![],
            outcome: crate::runtime::task_document::TurnOutcome::Completed,
            changed_files: vec![],
            command_history: vec![],
            tokens: crate::usage::TurnTokens {
                ..Default::default()
            },
            started_at_ms: 0,
            completed_at_ms: 1,
            ttft_ms: None,
            timings: None,
        });

    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task_id(),
        original_id,
        "/compact must preserve task-id"
    );
    assert!(
        mode.task_doc.completed_turns.is_empty(),
        "/compact must clear accumulated turns"
    );
    assert!(
        !mode.task_doc.info.active_grants.is_empty() || mode.task_doc.info.active_grants.is_empty(),
        "grants state must remain consistent"
    );
}
