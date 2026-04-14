use super::*;

#[test]
fn test_missing_mutating_location_prompt_requires_explicit_paths() {
    let edit_missing = json!({
        "old_str": "a",
        "new_str": "b"
    });
    let edit_with_path = json!({
        "file_path": "src/calculator.rs",
        "old_str": "a",
        "new_str": "b"
    });
    let rename_missing = json!({
        "old_path": "src/a.rs"
    });
    let rename_ready = json!({
        "from": "src/a.rs",
        "to": "src/b.rs"
    });

    assert!(missing_mutating_location_prompt("edit_file", &edit_missing).is_some());
    assert!(missing_mutating_location_prompt("edit_file", &edit_with_path).is_none());
    assert!(missing_mutating_location_prompt("rename_file", &rename_missing).is_some());
    assert!(missing_mutating_location_prompt("rename_file", &rename_ready).is_none());
    assert!(missing_mutating_location_prompt("read_file", &json!({"path":"x"})).is_none());
}
#[tokio::test]
async fn test_edit_file_missing_path_returns_clarification_instead_of_looping() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_missing_path_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_missing_path_01","name":"edit_file","input":{"old_text":"x","new_text":"y"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse =
        plain_text_round("msg_missing_path_02", "Please provide a target file path.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let final_text = manager
        .send_message("please edit".to_string(), None)
        .await?;
    assert!(final_text.contains("target file path"));
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
        assert!(blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolResult { is_error: true, .. })));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}
#[tokio::test]
async fn test_generate_tests_blocks_non_test_patch_before_approval() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    std::env::set_var("VEX_TOOL_CONFIRM", "off");

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_generate_tests_guard_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_generate_tests_guard_01","name":"write_file","input":{"path":"src/lib.rs","content":"pub fn answer() -> i32 { 42 }\n"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":5}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round(
        "msg_generate_tests_guard_02",
        "Test generation stayed within test files.",
    );
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let approval_task = tokio::spawn(async move {
        let mut saw_approval_request = false;
        while let Some(update) = rx.recv().await {
            if matches!(update, ConversationStreamUpdate::ToolApprovalRequest(_)) {
                saw_approval_request = true;
            }
        }
        saw_approval_request
    });
    let final_text = manager
        .send_message_with_policy(
            "generate tests for src/lib.rs".to_string(),
            Some(&tx),
            TurnToolPolicy::TestsOnlyMutations,
        )
        .await?;
    drop(tx);
    let saw_approval_request = approval_task.await?;
    std::env::remove_var("VEX_TOOL_CONFIRM");

    assert!(
        !saw_approval_request,
        "/generate-tests guard must block non-test file writes before approval"
    );
    assert!(final_text.contains("Test generation stayed within test files."));

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
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error: true,
            } if tool_use_id == "toolu_generate_tests_guard_01"
                && content.contains("Dropped non-test patch target `src/lib.rs`")
        )));
    } else {
        panic!("expected tool_result blocks");
    }

    Ok(())
}
#[test]
fn test_current_turn_has_successful_mutation_requires_successful_mutating_tool_result() {
    use crate::runtime::json_handoff::RuntimeEvent;
    use crate::runtime::task_document::TurnOutcome;
    use crate::usage::TurnTokens;

    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let mut manager = ConversationManager::new_mock(client, HashMap::new());

    // Populate an active turn with a successful write_file call.
    manager.ensure_task_doc();
    manager.begin_turn_doc("prompt".to_string(), TurnToolPolicy::Default);
    manager.apply_doc_event(RuntimeEvent::ToolCall {
        id: "tool_mut".to_string(),
        name: "apply_patch".to_string(),
        arguments: json!({"path": "src/lib.rs"}),
    });
    manager.apply_doc_event(RuntimeEvent::ToolResult {
        tool_call_id: "tool_mut".to_string(),
        tool_name: Some("apply_patch".to_string()),
        is_error: false,
        output: "patched".to_string(),
    });
    assert!(manager.current_turn_has_successful_mutation());

    // Replace active turn with a read-only call.
    manager.finish_turn_doc(TurnOutcome::Completed, TurnTokens::default());
    manager.begin_turn_doc("prompt2".to_string(), TurnToolPolicy::Default);
    manager.apply_doc_event(RuntimeEvent::ToolCall {
        id: "tool_read".to_string(),
        name: "read_file".to_string(),
        arguments: json!({"path": "src/lib.rs"}),
    });
    manager.apply_doc_event(RuntimeEvent::ToolResult {
        tool_call_id: "tool_read".to_string(),
        tool_name: Some("read_file".to_string()),
        is_error: false,
        output: "contents".to_string(),
    });
    assert!(
        !manager.current_turn_has_successful_mutation(),
        "read-only tools must not count as a patch-applied turn"
    );

    // Replace active turn with a failed mutating call.
    manager.finish_turn_doc(TurnOutcome::Completed, TurnTokens::default());
    manager.begin_turn_doc("prompt3".to_string(), TurnToolPolicy::Default);
    manager.apply_doc_event(RuntimeEvent::ToolCall {
        id: "tool_fail".to_string(),
        name: "apply_patch".to_string(),
        arguments: json!({"path": "src/lib.rs"}),
    });
    manager.apply_doc_event(RuntimeEvent::ToolResult {
        tool_call_id: "tool_fail".to_string(),
        tool_name: Some("apply_patch".to_string()),
        is_error: true,
        output: "error".to_string(),
    });
    assert!(
        !manager.current_turn_has_successful_mutation(),
        "failed mutating tools must not count as an applied patch"
    );
}
// ---------------------------------------------------------------------------
// Phase 3 — write_file guards
// ---------------------------------------------------------------------------

#[test]
fn test_write_file_rejects_content_above_max_lines() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_WRITE_FILE_MAX_LINES", "10");
    let dir = tempfile::tempdir().unwrap();
    let op = ToolOperator::new(dir.path().to_path_buf());

    let long_content: String = (0..15).map(|i| format!("line {i}\n")).collect();
    let input = json!({"path": "big.rs", "content": long_content});
    let result = super::tools::execute_tool_dispatch(&op, "write_file", &input);
    std::env::remove_var("VEX_WRITE_FILE_MAX_LINES");

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("rejected") && err.contains("limit"),
        "expected rejection message, got: {err}"
    );
}
#[test]
fn test_write_file_warns_above_diff_preferred_threshold() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_DIFF_PREFERRED_ABOVE_LINES", "15");
    std::env::set_var("VEX_WRITE_FILE_MAX_LINES", "500");
    let dir = tempfile::tempdir().unwrap();
    let op = ToolOperator::new(dir.path().to_path_buf());

    let content: String = (0..20).map(|i| format!("line {i}\n")).collect();
    let input = json!({"path": "medium.rs", "content": content});
    let result = super::tools::execute_tool_dispatch(&op, "write_file", &input);
    std::env::remove_var("VEX_DIFF_PREFERRED_ABOVE_LINES");
    std::env::remove_var("VEX_WRITE_FILE_MAX_LINES");

    let output = result.expect("write_file should succeed");
    assert!(
        output.contains("Prefer apply_patch"),
        "expected diff-preferred warning, got: {output}"
    );
}
