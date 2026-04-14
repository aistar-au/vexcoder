use super::*;

#[test]
fn test_missing_read_only_location_prompt_requires_explicit_paths() {
    let read_missing = json!({});
    let read_blank = json!({
        "path": "   "
    });
    let read_ready = json!({
        "path": "src/calculator.rs"
    });

    let prompt = missing_read_only_location_prompt("read_file", &read_missing)
        .expect("expected clarification for missing read path");
    assert!(prompt.contains("explicit file path"));
    assert!(prompt.contains("[file: ...]"));
    assert!(missing_read_only_location_prompt("read_file", &read_blank).is_some());
    assert!(missing_read_only_location_prompt("read_file", &read_ready).is_none());
    assert!(missing_read_only_location_prompt("edit_file", &json!({})).is_none());
}
#[test]
fn test_read_only_user_request_detection_and_mutating_guard() {
    assert!(is_read_only_user_request("show me calculator.rs"));
    assert!(is_read_only_user_request("what is in src/runtime/loop.rs"));
    assert!(is_read_only_user_request(
        "read-only review src/app.rs and provide an exact diff if changes are needed"
    ));
    assert!(is_read_only_user_request("show tests/fixtures/data.txt"));
    assert!(!is_read_only_user_request(
        "add a new function and commit it"
    ));
    assert!(!is_read_only_user_request(
        "show tests/fixtures/data.txt and fix it"
    ));

    let guard = mutating_tool_read_only_conflict_prompt("show the git diff", "write_file");
    assert!(
        guard.is_some(),
        "mutating call should be blocked for read-only request"
    );
    assert!(
        guard.unwrap().contains("No file changes were made"),
        "guard copy should be explicit about mutation safety"
    );

    assert!(
        mutating_tool_read_only_conflict_prompt("add calculator.rs", "write_file").is_none(),
        "explicit mutating intent should not be blocked"
    );
    assert!(
        mutating_tool_read_only_conflict_prompt("read-only inspect src/app.rs", "edit_file")
            .is_some(),
        "explicit read-only phrasing should block mutating calls"
    );
}
#[tokio::test]
async fn test_read_file_missing_path_returns_clarification_instead_of_looping() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_missing_read_path_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_missing_read_path_01","name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse = plain_text_round(
        "msg_missing_read_path_02",
        "Please provide a concrete file path or ask for a repo overview.",
    );
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let final_text = manager
        .send_message("summarise this repo briefly".to_string(), None)
        .await?;
    assert!(final_text.contains("repo overview"));
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
            ContentBlock::ToolResult { content, is_error: true, .. }
                if content.contains("explicit file path")
                    && content.contains("[file: ...]")
        )));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}
#[tokio::test]
async fn test_parallel_read_only_round_clarifies_missing_read_path() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_parallel_missing_read_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_list_root","name":"list_files","input":{}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_missing_read_parallel","name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round(
        "msg_parallel_missing_read_02",
        "Use list_files or codebase_search first when the file path is unknown.",
    );
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let temp = TempDir::new()?;
    std::fs::write(temp.path().join("README.md"), "workspace root\n")?;
    let mut manager = ConversationManager::new(
        mock_api_client,
        ToolOperator::new(temp.path().to_path_buf()),
    );

    let final_text = manager
        .send_message("summarise this repo briefly".to_string(), None)
        .await?;

    assert!(final_text.contains("list_files or codebase_search first"));

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
        let tool_result_ids = blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            tool_result_ids,
            vec!["toolu_list_root", "toolu_missing_read_parallel"]
        );
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult {
                tool_use_id,
                is_error: false,
                ..
            } if tool_use_id == "toolu_list_root"
        )));
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult { tool_use_id, content, is_error }
                if tool_use_id == "toolu_missing_read_parallel"
                    && *is_error
                    && content.contains("explicit file path")
                    && content.contains("[file: ...]")
                    && !content.contains("requires a non-empty 'path'")
        )));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}
#[tokio::test]
async fn test_read_only_request_blocks_mutating_tool_without_approval_prompt() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    crate::test_support::test_set_var("VEX_TOOL_CONFIRM", "off");

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_readonly_guard_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_readonly_guard_01","name":"write_file","input":{"path":"calculator.rs","content":"fn main() {}\n"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":5}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = plain_text_round(
        "msg_readonly_guard_02",
        "Read-only request handled without file mutation.",
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
        .send_message("show me calculator.rs".to_string(), Some(&tx))
        .await?;
    drop(tx);
    let saw_approval_request = approval_task.await?;
    crate::test_support::test_remove_var("VEX_TOOL_CONFIRM");

    assert!(
        !saw_approval_request,
        "read-only request guard should block mutating tool before approval overlay"
    );
    assert!(final_text.contains("Read-only request handled"));

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
            } if tool_use_id == "toolu_readonly_guard_01" && content.contains("appears read-only")
        )));
    } else {
        panic!("expected tool_result blocks");
    }

    Ok(())
}
