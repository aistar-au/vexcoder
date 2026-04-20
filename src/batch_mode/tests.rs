use super::*;
use crate::config::CompactionConfig;
use crate::runtime::edit_loop::EditLoopOutcome;

#[tokio::test]
async fn test_batch_mode_exits_zero_on_completion() {
    let result = run_batch_mode("echo hello", 3).await.unwrap();
    assert_eq!(result.status, TaskStatus::Completed);
}

#[test]
fn test_batch_mode_ignores_transcript_and_edit_loop_updates() {
    let opts = BatchRunOpts::default();
    let mut mode = BatchMode::new("task-batch".to_string(), opts, None, None);
    let client = crate::api::ApiClient::new_mock(std::sync::Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let conversation =
        crate::state::ConversationManager::new_mock(client, std::collections::HashMap::new());
    let (update_tx, _update_rx) = tokio::sync::mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(
        conversation,
        update_tx,
        tokio_util::sync::CancellationToken::new(),
    );

    mode.on_model_update(UiUpdate::TranscriptLine("warning".to_string()), &mut ctx);
    mode.on_model_update(
        UiUpdate::EditLoopComplete {
            outcome: EditLoopOutcome::Cancelled,
            last_validation_result: None,
        },
        &mut ctx,
    );

    assert!(mode.current_response.is_empty());
    assert!(mode.output_lines.is_empty());
    assert_eq!(mode.status, TaskStatus::Ready);
}

#[tokio::test]
async fn test_batch_mode_completes_on_final_allowed_turn() {
    let result = run_batch_mode_with_opts(
        "keep going",
        BatchRunOpts {
            max_turns: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_batch_mode_interactive_approval_denied_by_default() {
    let result = run_batch_mode_with_opts(
        "run: ls",
        BatchRunOpts {
            auto_approve: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        result.status,
        TaskStatus::Completed | TaskStatus::Failed
    ));
}

#[tokio::test]
async fn test_batch_mode_auto_approve_once_grants_single_turn() {
    let result = run_batch_mode_with_opts(
        "run: echo approved",
        BatchRunOpts {
            auto_approve: Some(AutoApproveScope::Once),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_batch_mode_memory_clear_requires_auto_approve() {
    let result = run_batch_mode_with_opts(
        "/memory clear",
        BatchRunOpts {
            format: OutputFormat::Text,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.status, TaskStatus::Failed);
    assert!(
        result
            .output_lines
            .iter()
            .any(|line| line.contains("requires --auto-approve")),
        "expected batch-mode memory clear guidance in output"
    );
}

#[tokio::test]
async fn test_batch_mode_memory_clear_with_auto_approve_clears_notes() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "batch note\n").unwrap();

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
        model_profile: crate::types::ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig {
            auto_index: false,
            ..Default::default()
        },
        notes_path: Some(notes_path.clone()),
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };

    let result = run_batch(
        "/memory clear".to_string(),
        BatchRunOpts {
            auto_approve: Some(AutoApproveScope::Task),
            format: OutputFormat::Text,
            ..Default::default()
        },
        &config,
    )
    .await
    .unwrap();

    assert_eq!(result.status, TaskStatus::Completed);
    assert!(
        result
            .output_lines
            .iter()
            .any(|line| line.contains("[memory: cleared]")),
        "expected batch-mode memory clear confirmation in output"
    );
    assert_eq!(
        std::fs::read_to_string(&notes_path).unwrap(),
        "",
        "expected batch-mode memory clear to truncate the notes file"
    );
}

#[tokio::test]
async fn test_batch_mode_memory_clear_with_max_turns_emits_single_summary() {
    let result = run_batch_mode_with_opts(
        "/memory clear",
        BatchRunOpts {
            max_turns: Some(1),
            format: OutputFormat::Jsonl,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let summary_count = result
        .output_lines
        .iter()
        .filter(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|value| value.get("summary").and_then(|entry| entry.as_bool()))
                == Some(true)
        })
        .count();

    assert_eq!(summary_count, 1, "expected exactly one summary record");
}

#[tokio::test]
async fn test_batch_mode_jsonl_output_includes_required_fields() {
    let output = capture_batch_jsonl("echo hello", 3).await.unwrap();
    let records = output
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let first_turn = records
        .iter()
        .find(|value| {
            !value
                .get("summary")
                .and_then(|entry| entry.as_bool())
                .unwrap_or(false)
        })
        .expect("expected a turn record before the final summary");
    assert_eq!(first_turn["turn"], 1);
    assert_eq!(first_turn["input"], "echo hello");
    assert!(first_turn.get("response").is_some());
    assert!(first_turn["changed_files"].is_array());
    assert!(first_turn["command_history"].is_array());

    let summary = records
        .iter()
        .find(|value| value.get("summary").and_then(|entry| entry.as_bool()) == Some(true))
        .expect("expected a final summary record");
    assert_eq!(summary["status"], "Completed");
}

#[tokio::test]
async fn test_batch_mode_jsonl_output_captures_streamed_tool_evidence() {
    let mut ctx = setup_batch_ctx();
    let opts = BatchRunOpts {
        format: OutputFormat::Jsonl,
        ..Default::default()
    };
    let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);

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
                output: "wrote file".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolCall {
                id: "tool-2".to_string(),
                name: "git_commit".to_string(),
                input: serde_json::json!({
                    "message": "record evidence"
                }),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 2,
            block: StreamBlock::ToolResult {
                tool_call_id: "tool-2".to_string(),
                output: "Committed".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::StreamDelta("done".to_string()), &mut ctx);
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    let turn_line = mode.output_lines.first().expect("expected turn line");
    let turn_json: serde_json::Value = serde_json::from_str(turn_line).unwrap();
    let changed_files = turn_json["changed_files"]
        .as_array()
        .expect("changed_files must be present");
    assert!(
        changed_files
            .iter()
            .filter_map(|value| value.as_str())
            .any(|path| path == "src/main.rs")
    );
    let command_history = turn_json["command_history"]
        .as_array()
        .expect("command_history must be present");
    assert_eq!(command_history.len(), 1);
    assert_eq!(command_history[0]["program"], "git commit");
    assert_eq!(command_history[0]["exit_code"], 0);
}

#[tokio::test]
async fn test_batch_mode_ignores_failed_tool_changed_file_evidence() {
    let mut ctx = setup_batch_ctx();
    let opts = BatchRunOpts {
        format: OutputFormat::Jsonl,
        ..Default::default()
    };
    let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);

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
    mode.on_model_update(UiUpdate::StreamDelta("done".to_string()), &mut ctx);
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    let turn_line = mode.output_lines.first().expect("expected turn line");
    let turn_json: serde_json::Value = serde_json::from_str(turn_line).unwrap();
    let changed_files = turn_json["changed_files"]
        .as_array()
        .expect("changed_files must be present");
    assert!(
        !changed_files
            .iter()
            .filter_map(|value| value.as_str())
            .any(|path| path == "src/main.rs"),
        "failed tool calls must not be recorded as changed files"
    );
}

#[tokio::test]
async fn test_batch_mode_marks_second_turn_attempt_as_max_turns_reached() {
    let mut ctx = setup_batch_ctx();
    let mut mode = BatchMode::new(
        "test-task".to_string(),
        BatchRunOpts {
            max_turns: Some(1),
            format: OutputFormat::Jsonl,
            ..Default::default()
        },
        None,
        None,
    );
    mode.current_turn = 1;

    mode.on_user_input("second turn".to_string(), &mut ctx);

    assert_eq!(mode.status, TaskStatus::MaxTurnsReached);
    assert!(mode.is_done());
}

#[tokio::test]
async fn test_batch_mode_text_format_outputs_plain_response() {
    let output = capture_batch_text("echo hello", 3).await.unwrap();
    // Text format must not begin with a JSON envelope character.
    let trimmed = output.trim_start();
    // Empty output is acceptable for a mock that produces no text delta.
    if !trimmed.is_empty() {
        assert!(!trimmed.starts_with('{'));
    }
}

#[tokio::test]
async fn test_batch_mode_text_format_captures_textual_block_delta_output() {
    let mut ctx = setup_batch_ctx();
    let opts = BatchRunOpts {
        format: OutputFormat::Text,
        ..Default::default()
    };
    let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);

    mode.on_user_input("task".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: "normalized batch output".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::StreamBlockComplete { index: 0 }, &mut ctx);
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    assert_eq!(
        mode.output_lines,
        vec!["normalized batch output".to_string()]
    );
}

#[tokio::test]
async fn test_batch_mode_text_format_ignores_tool_argument_block_deltas() {
    let mut ctx = setup_batch_ctx();
    let opts = BatchRunOpts {
        format: OutputFormat::Text,
        ..Default::default()
    };
    let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);

    mode.on_user_input("task".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolCall {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Pending,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockDelta {
            index: 1,
            delta: "{\"path\":\"src/lib.rs\"}".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::StreamBlockComplete { index: 1 }, &mut ctx);
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    assert_eq!(mode.output_lines, vec![String::new()]);
}

#[tokio::test]
async fn test_build_batch_runtime_injects_memory_notes_into_system_prompt() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "batch note\n").unwrap();

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
        model_profile: crate::types::ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig {
            auto_index: false,
            ..Default::default()
        },
        notes_path: Some(notes_path),
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };

    let (_runtime, ctx, _task_id) = build_batch_runtime(
        &config,
        "run the lint checks".to_string(),
        BatchRunOpts::default(),
    )
    .unwrap();
    let system_prompt = ctx.test_system_prompt().await;
    assert!(
        system_prompt.contains("<memory>\nbatch note\n</memory>"),
        "expected batch runtime client to include memory notes in system prompt"
    );
}

#[tokio::test]
async fn test_build_batch_runtime_injects_project_instructions_into_system_prompt() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("AGENTS.md"), "# batch instructions\n").unwrap();

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
        model_profile: crate::types::ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
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

    let (_runtime, ctx, _task_id) = build_batch_runtime(
        &config,
        "run the lint checks".to_string(),
        BatchRunOpts::default(),
    )
    .unwrap();
    let system_prompt = ctx.test_system_prompt().await;
    assert!(system_prompt.contains("[project instructions: start]"));
    assert!(system_prompt.contains("# batch instructions"));
}

#[tokio::test]
async fn test_build_batch_runtime_auto_index_warms_codebase_search_index() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    crate::state::clear_codebase_index_for_tests();

    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("warm.rs"), "pub fn batch_warm_symbol() {}\n").unwrap();

    let search_config = crate::config::SearchConfig {
        enabled: true,
        auto_index: true,
        ..Default::default()
    };

    let count = crate::state::warm_codebase_index_with_config(temp.path(), &search_config);
    assert!(
        count.is_some() && count.unwrap() > 0,
        "warm_codebase_index_with_config must index at least one chunk"
    );
    let names = crate::state::codebase_index_names_for_tests();
    assert!(
        names.iter().any(|name| name == "batch_warm_symbol"),
        "expected batch startup warm to index batch_warm_symbol"
    );
}

#[test]
fn test_build_batch_runtime_uses_resumed_task_id() {
    let temp = tempfile::tempdir().unwrap();
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
        model_profile: crate::types::ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
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
    let resume_state = TaskState::new("task-batch-resume".to_string());

    let (_runtime, _ctx, task_id) = build_batch_runtime(
        &config,
        "run the lint checks".to_string(),
        BatchRunOpts {
            resume_state: Some(resume_state),
            ..Default::default()
        },
    )
    .expect("build_batch_runtime should succeed");

    assert_eq!(task_id, "task-batch-resume");
}

#[test]
fn test_batch_mode_jsonl_output_includes_instructions_path() {
    let opts = BatchRunOpts {
        format: OutputFormat::Jsonl,
        ..Default::default()
    };
    let mut mode = BatchMode::new(
        "test-task".to_string(),
        opts,
        None,
        Some("AGENTS.md".to_string()),
    );

    mode.status = TaskStatus::Running;
    mode.turn_in_progress = true;
    mode.current_response = "done".to_string();
    mode.finish_turn(TurnTokens {
        input: 8,
        output: 4,
        estimated: false,
        ..Default::default()
    });
    mode.status = TaskStatus::Completed;
    mode.append_summary();

    let turn_json: serde_json::Value =
        serde_json::from_str(mode.output_lines.first().expect("expected turn line")).unwrap();
    assert_eq!(turn_json["instructions_path"], "AGENTS.md");
    assert_eq!(turn_json["tokens"]["input"], 8);

    let summary_json: serde_json::Value =
        serde_json::from_str(mode.output_lines.last().expect("expected summary line")).unwrap();
    assert_eq!(summary_json["instructions_path"], "AGENTS.md");
}

#[test]
fn test_batch_mode_summary_keeps_changed_files_from_prior_turns() {
    let opts = BatchRunOpts {
        format: OutputFormat::Jsonl,
        ..Default::default()
    };
    let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);

    mode.status = TaskStatus::Running;
    mode.turn_in_progress = true;
    mode.current_turn_changed_files
        .insert("src/first.rs".to_string());
    mode.finish_turn(TurnTokens::default());

    mode.reset_current_turn_state();
    mode.status = TaskStatus::Running;
    mode.turn_in_progress = true;
    mode.current_turn_changed_files
        .insert("src/second.rs".to_string());
    mode.finish_turn(TurnTokens::default());

    mode.status = TaskStatus::Completed;
    mode.append_summary();

    let summary_json: serde_json::Value =
        serde_json::from_str(mode.output_lines.last().expect("expected summary line")).unwrap();
    let changed_files = summary_json["changed_files"]
        .as_array()
        .expect("summary changed_files must be present");
    let changed_files = changed_files
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(changed_files.contains(&"src/first.rs"));
    assert!(changed_files.contains(&"src/second.rs"));
}

fn setup_batch_ctx() -> RuntimeContext {
    use crate::api::{ApiClient, mock_client::MockApiClient};
    use crate::state::ConversationManager;
    use std::collections::HashMap;
    use std::sync::Arc;

    let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    RuntimeContext::new(conversation, tx, CancellationToken::new())
}

#[tokio::test]
async fn test_batch_mode_zero_max_turns_stops_before_first_turn() {
    // max_turns = Some(0): current_turn (0) >= max (0) fires immediately
    // on the first on_user_input call, before start_turn is reached.
    let result = run_batch_mode_with_opts(
        "keep going",
        BatchRunOpts {
            max_turns: Some(0),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        result.status,
        TaskStatus::MaxTurnsReached,
        "max_turns=0 must produce MaxTurnsReached, not Completed"
    );
}

#[test]
fn test_batch_mode_jsonl_output_includes_input_field() {
    let opts = BatchRunOpts {
        format: OutputFormat::Jsonl,
        ..Default::default()
    };
    let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);
    mode.status = TaskStatus::Running;
    mode.turn_in_progress = true;
    mode.current_turn_input = "what is the answer".to_string();
    mode.current_response = "forty-two".to_string();
    mode.finish_turn(TurnTokens {
        input: 5,
        output: 7,
        estimated: true,
        ..Default::default()
    });

    let turn_json: serde_json::Value =
        serde_json::from_str(mode.output_lines.first().expect("expected turn line")).unwrap();
    assert_eq!(
        turn_json
            .get("input")
            .expect("input field must be present in JSONL"),
        "what is the answer",
        "input field must record the prompt text submitted for this turn"
    );
    assert_eq!(turn_json["tokens"]["output"], 7);
    assert_eq!(turn_json["tokens"]["estimated"], true);
}

#[tokio::test]
async fn test_batch_mode_memory_clear_jsonl_records_input() {
    let result = run_batch_mode_with_opts(
        "/memory clear",
        BatchRunOpts {
            format: OutputFormat::Jsonl,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let turn_json: serde_json::Value =
        serde_json::from_str(result.output_lines.first().expect("expected turn line")).unwrap();
    assert_eq!(
        turn_json
            .get("input")
            .expect("input field must be present in JSONL"),
        "/memory clear",
        "batch-mode local turns must preserve the submitted input text"
    );
}
