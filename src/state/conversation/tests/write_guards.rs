use super::*;

#[test]
fn missing_mutating_location_prompt_requires_explicit_paths() {
    assert!(
        missing_mutating_location_prompt("edit_file", &json!({"old_str":"a","new_str":"b"}))
            .is_some()
    );
    assert!(
        missing_mutating_location_prompt(
            "edit_file",
            &json!({"file_path":"src/x.rs","old_str":"a","new_str":"b"})
        )
        .is_none()
    );
    assert!(
        missing_mutating_location_prompt("rename_file", &json!({"old_path":"src/a.rs"})).is_some()
    );
    assert!(
        missing_mutating_location_prompt(
            "rename_file",
            &json!({"from":"src/a.rs","to":"src/b.rs"})
        )
        .is_none()
    );
    assert!(missing_mutating_location_prompt("read_file", &json!({"path":"x"})).is_none());
}

#[test]
fn write_file_rejects_content_above_max_lines() {
    let content: String = (0..1001).map(|i| format!("line {i}\n")).collect();
    let dir = tempfile::tempdir().unwrap();
    let operator = ToolOperator::new(dir.path().to_path_buf());
    let result = crate::state::conversation::tools::routing::call_tool_routing(
        &operator,
        "write_file",
        &json!({"path": "big.rs", "content": content}),
    );
    assert!(result.is_err(), "very large write must be blocked by guard");
}

#[tokio::test]
async fn edit_file_missing_path_returns_clarification_not_loop() -> Result<()> {
    let first_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_ep_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_ep_01","name":"edit_file","input":{"old_text":"x","new_text":"y"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_sse = plain_text_round("msg_ep_02", "Please provide a target file path.");
    let mut manager = ConversationManager::new_mock(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_sse, second_sse,
        ]))),
        HashMap::new(),
    );
    let final_text = manager
        .send_message("please edit".to_string(), None)
        .await?;
    assert!(final_text.contains("target file path"));
    let tool_result = manager.api_messages.iter().find(|m| m.role == "user"
        && matches!(&m.content, Content::Blocks(b) if b.iter().any(|blk| matches!(blk, ContentBlock::ToolResult { .. }))));
    assert!(
        tool_result.is_some(),
        "clarification must be sent as tool result, not user message"
    );
    Ok(())
}
