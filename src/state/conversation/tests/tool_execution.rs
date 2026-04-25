use super::*;

#[test]
fn test_read_only_tool_round_helpers() {
    let read_round = vec![ContentBlock::ToolUse {
        id: "tool_1".to_string(),
        name: "read_file".to_string(),
        input: json!({"path":"src/app/mod.rs"}),
        metadata: None,
    }];
    assert!(is_read_only_tool_round(&read_round));

    let git_read_round = vec![ContentBlock::ToolUse {
        id: "tool_git".to_string(),
        name: "git_diff".to_string(),
        input: json!({}),
        metadata: None,
    }];
    assert!(is_read_only_tool_round(&git_read_round));

    let write_round = vec![ContentBlock::ToolUse {
        id: "tool_2".to_string(),
        name: "write_file".to_string(),
        input: json!({"path":"src/app/mod.rs","content":"x"}),
        metadata: None,
    }];
    assert!(!is_read_only_tool_round(&write_round));

    let sig_a = tool_round_signature(&read_round);
    let sig_b = tool_round_signature(&read_round);
    assert_eq!(sig_a, sig_b);

    let changed_read_round = vec![ContentBlock::ToolUse {
        id: "tool_3".to_string(),
        name: "read_file".to_string(),
        input: json!({"path":"src/state/conversation.rs"}),
        metadata: None,
    }];
    let sig_c = tool_round_signature(&changed_read_round);
    assert_ne!(sig_a, sig_c);
}
#[test]
fn test_parallel_safe_tool_round_helpers() {
    let parallel_round = vec![
        ContentBlock::ToolUse {
            id: "tool_read_1".to_string(),
            name: "read_file".to_string(),
            input: json!({"path":"src/app/mod.rs"}),
            metadata: None,
        },
        ContentBlock::ToolUse {
            id: "tool_read_2".to_string(),
            name: "codebase_search".to_string(),
            input: json!({"query":"ConversationManager"}),
            metadata: None,
        },
    ];
    assert!(is_parallel_safe_tool_round(&parallel_round));

    let mixed_round = vec![
        ContentBlock::ToolUse {
            id: "tool_read_3".to_string(),
            name: "read_file".to_string(),
            input: json!({"path":"src/app/mod.rs"}),
            metadata: None,
        },
        ContentBlock::ToolUse {
            id: "tool_write_1".to_string(),
            name: "write_file".to_string(),
            input: json!({"path":"src/app/mod.rs","content":"x"}),
            metadata: None,
        },
    ];
    assert!(!is_parallel_safe_tool_round(&mixed_round));
    assert!(should_parallelize_tool_round(&parallel_round, false));
    assert!(!should_parallelize_tool_round(&parallel_round, true));
}
#[test]
fn test_tool_requires_confirmation_for_mutating_tools() {
    assert!(tool_requires_confirmation("write_file"));
    assert!(tool_requires_confirmation("apply_patch"));
    assert!(tool_requires_confirmation("edit_file"));
    assert!(tool_requires_confirmation("rename_file"));
    assert!(tool_requires_confirmation("git_add"));
    assert!(tool_requires_confirmation("git_commit"));

    assert!(tool_requires_confirmation("mcp.myserver.some_tool"));
    assert!(tool_requires_confirmation("mcp.fs.write"));

    assert!(!tool_requires_confirmation("read_file"));
    assert!(!tool_requires_confirmation("search_files"));
    assert!(!tool_requires_confirmation("list_files"));
    assert!(!tool_requires_confirmation("git_status"));
    assert!(!tool_requires_confirmation("git_diff"));
    assert!(!tool_requires_confirmation("git_log"));
    assert!(!tool_requires_confirmation("git_show"));
}

#[test]
fn test_tool_requires_confirmation_for_run_command_and_all_aliases() {
    assert!(tool_requires_confirmation("run_command"));
    assert!(tool_requires_confirmation("run_shell_command"));
    assert!(tool_requires_confirmation("bash"));
    assert!(tool_requires_confirmation("call_command"));
    assert!(tool_requires_confirmation("call_bash"));
}
#[test]
fn test_search_files_accepts_common_query_aliases() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("notes.txt"), "hello alias world\n").unwrap();
    let operator = ToolOperator::new(temp.path().to_path_buf());

    let result = call_tool_routing(
        &operator,
        "search_files",
        &json!({
            "pattern": "alias",
            "directory": ".",
            "limit": "5"
        }),
    )
    .unwrap();

    assert!(result.contains("notes.txt:1:hello alias world"));
}
#[test]
fn test_builtin_supported_git_tools_response_lists_only_supported_tools() {
    let response = builtin_supported_git_tools_response("what other git tools can you call")
        .expect("expected capability response");
    assert!(response.contains("git_status"));
    assert!(response.contains("git_diff"));
    assert!(response.contains("git_log"));
    assert!(response.contains("git_show"));
    assert!(response.contains("git_add"));
    assert!(response.contains("git_commit"));
    assert!(!response.contains("git_clone"));
    assert!(!response.contains("git_init"));
    assert!(builtin_supported_git_tools_response("show the git diff").is_none());
}
#[tokio::test]
async fn test_tool_execution_error_sets_error_status() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_error_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_error_01","name":"read_file","input":{"path":"does-not-exist.txt"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round("msg_tool_error_02", "Handled read error.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let final_text = manager
        .send_message("read a missing file".to_string(), Some(&tx))
        .await?;
    assert!(final_text.contains("Handled read error."));
    drop(tx);

    let mut saw_error_status = false;
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart {
            block: StreamBlock::ToolCall { id, status, .. },
            ..
        } = update
            && id.starts_with("tx_")
            && status == ToolStatus::Error
        {
            saw_error_status = true;
        }
    }
    assert!(
        saw_error_status,
        "tool execution failure must emit ToolStatus::Error"
    );
    Ok(())
}
#[tokio::test]
async fn test_multi_tool_round_collects_results_after_approval_denial() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    crate::test_support::test_set_var(&_env_lock, "VEX_TOOL_CONFIRM", "off");

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_multi_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_multi_mut","name":"write_file","input":{"path":"calculator.rs","content":"fn main() {}\n"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_multi_read","name":"read_file","input":{"path":"file.txt"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round("msg_multi_02", "Handled both tool outcomes.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "hello".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let approval_task = tokio::spawn(async move {
        let mut saw_approval_request = false;
        while let Some(update) = rx.recv().await {
            if let ConversationStreamUpdate::ToolApprovalRequest(request) = update {
                saw_approval_request = true;
                let _ = request.response_tx.send(false);
            }
        }
        saw_approval_request
    });
    let final_text = manager
        .send_message("run mixed tools".to_string(), Some(&tx))
        .await?;
    drop(tx);
    let saw_approval_request = approval_task.await?;
    crate::test_support::test_remove_var(&_env_lock, "VEX_TOOL_CONFIRM");

    assert!(
        saw_approval_request,
        "expected mutating tool approval request"
    );
    assert!(final_text.contains("Handled both tool outcomes."));

    let tool_result_message = manager
        .api_messages
        .iter()
        .find(|message| {
            message.role == "user"
                && matches!(message.content, Content::Blocks(_))
                && message_contains_tool_result(message)
        })
        .expect("expected tool_result message in history");
    if let Content::Blocks(blocks) = &tool_result_message.content {
        let tool_results = blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => Some((tool_use_id.as_str(), content.as_str(), *is_error)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tool_results.len(), 2);
        assert!(
            tool_results
                .iter()
                .all(|(tool_use_id, _, _)| tool_use_id.starts_with("tx_"))
        );
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolResult { is_error: true, .. }))
        );
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult {
                content,
                is_error: false,
                ..
            } if content.contains("hello")
        )));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}
#[tokio::test]
async fn test_multi_read_only_tool_round_preserves_result_order() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_parallel_reads_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_read_a","name":"read_file","input":{"path":"file-a.txt"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_read_b","name":"read_file","input":{"path":"file-b.txt"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round("msg_parallel_reads_02", "Handled both reads.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file-a.txt".to_string(), "alpha".to_string());
    mock_tool_responses.insert("file-b.txt".to_string(), "beta".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager
        .send_message("read both files".to_string(), None)
        .await?;

    assert!(final_text.contains("Handled both reads."));

    let tool_result_message = manager
        .api_messages
        .iter()
        .find(|message| {
            message.role == "user"
                && matches!(message.content, Content::Blocks(_))
                && message_contains_tool_result(message)
        })
        .expect("expected tool_result message in history");

    if let Content::Blocks(blocks) = &tool_result_message.content {
        let tool_results = blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => Some((tool_use_id.as_str(), content.as_str(), *is_error)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tool_results.len(), 2);
        assert!(
            tool_results
                .iter()
                .all(|(tool_use_id, _, _)| tool_use_id.starts_with("tx_"))
        );
        assert!(!tool_results[0].2);
        assert!(!tool_results[1].2);
        assert!(tool_results[0].1.contains("alpha"));
        assert!(tool_results[1].1.contains("beta"));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}
#[tokio::test]
async fn test_git_tool_capability_query_short_circuits_without_api_round() -> Result<()> {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let response = manager
        .send_message("what other git tools can you call".to_string(), None)
        .await?;

    assert!(response.contains("git_status"));
    assert!(response.contains("git_diff"));
    assert!(response.contains("git_log"));
    assert!(response.contains("git_show"));
    assert!(response.contains("git_add"));
    assert!(response.contains("git_commit"));
    assert_eq!(
        manager.api_messages.len(),
        2,
        "capability response should not call API or create extra rounds"
    );

    Ok(())
}
#[tokio::test]
async fn test_tool_use_without_input_then_partial_json_executes_write_file() -> Result<()> {
    let temp = TempDir::new()?;

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_20","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Saving now."}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_mock_write_1","name":"write_file"}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"cal.rs\",\"content\":\"fn main() {}\\n\"}"}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":12}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_21","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Saved cal.rs."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let executor = ToolOperator::new(temp.path().to_path_buf());
    let mut manager = ConversationManager::new(mock_api_client, executor);

    let final_text = manager
        .send_message("create calculator".to_string(), None)
        .await?;
    assert!(final_text.contains("Saved cal.rs."));

    let written = std::fs::read_to_string(temp.path().join("cal.rs"))?;
    assert_eq!(written, "fn main() {}\n");

    Ok(())
}
#[tokio::test]
async fn test_execute_tool_edit_file_empty_path_rejected_before_executor() -> Result<()> {
    let temp = TempDir::new()?;
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new(mock_api_client, executor);

    let err = manager
        .execute_tool_with_timeout(
            "edit_file",
            &json!({
                "path": "",
                "old_str": "old",
                "new_str": "new"
            }),
            Duration::from_secs(1),
        )
        .await
        .expect_err("empty path should be rejected");
    assert!(err.to_string().contains("non-empty 'path'"));
    Ok(())
}
#[tokio::test]
async fn test_execute_tool_edit_file_accepts_alias_argument_names() -> Result<()> {
    let temp = TempDir::new()?;
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new(mock_api_client, executor);

    let target = temp.path().join("src").join("calculator.rs");
    std::fs::create_dir_all(target.parent().expect("target parent exists"))?;
    std::fs::write(&target, "pub fn calc() -> i32 { 1 }\n")?;

    let result = manager
        .execute_tool_with_timeout(
            "edit_file",
            &json!({
                "file_path": "src/calculator.rs",
                "old_text": "1",
                "new_text": "2"
            }),
            Duration::from_secs(1),
        )
        .await?;
    assert!(result.contains("Updated snippet in src/calculator.rs"));

    let updated = std::fs::read_to_string(&target)?;
    assert!(updated.contains("2"));
    Ok(())
}
#[tokio::test]
async fn test_execute_tool_edit_file_delete_summary_is_clear() -> Result<()> {
    let temp = TempDir::new()?;
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new(mock_api_client, executor);

    let target = temp.path().join("src").join("calculator.rs");
    std::fs::create_dir_all(target.parent().expect("target parent exists"))?;
    std::fs::write(&target, "pub fn sqrt() {}\n// keep\n")?;

    let result = manager
        .execute_tool_with_timeout(
            "edit_file",
            &json!({
                "path": "src/calculator.rs",
                "old_str": "pub fn sqrt() {}\n",
                "new_str": ""
            }),
            Duration::from_secs(1),
        )
        .await?;

    assert!(result.contains("Deleted snippet in src/calculator.rs"));
    assert_eq!(std::fs::read_to_string(&target)?, "// keep\n");
    Ok(())
}
#[tokio::test]
async fn test_execute_tool_run_command_uses_workspace_working_dir() -> Result<()> {
    let temp = TempDir::new()?;
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new(mock_api_client, executor);

    #[cfg(windows)]
    let input = json!({
        "command": "cmd",
        "args": ["/C", "cd"],
    });
    #[cfg(not(windows))]
    let input = json!({
        "command": "pwd",
        "args": [],
    });

    let result = manager
        .execute_tool_with_timeout("run_command", &input, Duration::from_secs(2))
        .await?;

    assert!(
        result.contains(&temp.path().display().to_string()),
        "run_command must call from the workspace working directory: {result}"
    );
    Ok(())
}
#[tokio::test]
async fn test_execute_tool_run_command_streams_managed_session_updates() -> Result<()> {
    let temp = TempDir::new()?;
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new(mock_api_client, executor);
    let (update_tx, mut update_rx) = mpsc::unbounded_channel();

    #[cfg(windows)]
    let input = json!({
        "command": "cmd",
        "args": ["/C", "echo streamed-from-tool"],
    });
    #[cfg(not(windows))]
    let input = json!({
        "command": "sh",
        "args": ["-c", "printf 'streamed-from-tool\\n'"],
    });

    let result = manager
        .execute_tool_with_timeout_with_updates(
            "run_command",
            &input,
            Duration::from_secs(3),
            Some(&update_tx),
        )
        .await?;

    let mut saw_session_started = false;
    let mut saw_session_attached = false;
    let mut saw_transcript_output = false;
    let mut saw_session_finished = false;

    while let Ok(update) = update_rx.try_recv() {
        match update {
            ConversationStreamUpdate::CommandSessionStarted { command, .. } => {
                saw_session_started = !command.trim().is_empty();
            }
            ConversationStreamUpdate::CommandSessionAttached { pid, .. } => {
                saw_session_attached = pid.is_some();
            }
            ConversationStreamUpdate::TranscriptLine(line) => {
                if line.contains("streamed-from-tool") {
                    saw_transcript_output = true;
                }
            }
            ConversationStreamUpdate::CommandSessionFinished { .. } => {
                saw_session_finished = true;
            }
            ConversationStreamUpdate::Delta(_)
            | ConversationStreamUpdate::BlockStart { .. }
            | ConversationStreamUpdate::BlockDelta { .. }
            | ConversationStreamUpdate::ToolCallArgumentsUpdated { .. }
            | ConversationStreamUpdate::BlockComplete { .. }
            | ConversationStreamUpdate::ToolApprovalRequest(_)
            | ConversationStreamUpdate::ServerMetadata(_)
            | ConversationStreamUpdate::ContextCompacted { .. }
            | ConversationStreamUpdate::StreamError(_) => {}
        }
    }

    assert!(saw_session_started, "expected command session start update");
    assert!(
        saw_session_attached,
        "expected command session attach update"
    );
    assert!(
        saw_transcript_output,
        "expected command session output to stream into transcript updates"
    );
    assert!(
        saw_session_finished,
        "expected command session finished update"
    );
    assert!(
        result.contains("streamed-from-tool"),
        "run_command tool result must still include final command output: {result}"
    );
    Ok(())
}
