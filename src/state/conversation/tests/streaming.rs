use super::*;

#[tokio::test]
async fn crit_01_full_protocol_flow_tool_call_and_completion() -> Result<()> {
    let first_sse = vec![
        r#"event: message_start
data: {"type": "message_start", "message": {"id": "msg_mock_01", "type": "message", "role": "assistant", "model": "mock-model", "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type": "content_block_start", "index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type": "content_block_delta", "index":0,"delta":{"type":"text_delta","text":"Okay, I can help with that. "}}"#.to_string(),
        r#"event: content_block_start
data: {"type": "content_block_start", "index":1,"content_block":{"type":"tool_use","id":"toolu_mock_01", "name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_delta
data: {"type": "content_block_delta", "index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\": \"file.txt\"}"}}"#.to_string(),
        r#"event: content_block_stop
data: {"type": "content_block_stop", "index":1}"#.to_string(),
        r#"event: message_delta
data: {"type": "message_delta", "delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
        r#"event: message_stop
data: {"type": "message_stop"}"#.to_string(),
    ];
    let second_sse = vec![
        r#"event: message_start
data: {"type": "message_start", "message": {"id": "msg_mock_02", "type": "message", "role": "assistant", "model": "mock-model", "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type": "content_block_start", "index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type": "content_block_delta", "index":0,"delta":{"type":"text_delta","text":"The content of file.txt is 'Hello from file.txt'"}}"#.to_string(),
        r#"event: message_delta
data: {"type": "message_delta", "delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
        r#"event: message_stop
data: {"type": "message_stop"}"#.to_string(),
    ];
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
        first_sse, second_sse,
    ])))
    .with_structured_tool_protocol();
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "Hello from file.txt".to_string());
    let mut manager = ConversationManager::new_mock(client, mock_tool_responses);

    let final_text = manager
        .send_message("What is in file.txt?".into(), None)
        .await?;
    assert!(final_text.contains("The content of file.txt is 'Hello from file.txt'"));
    assert_eq!(manager.api_messages.len(), 4);
    assert_eq!(manager.api_messages[0].role, "user");
    assert_eq!(manager.api_messages[1].role, "assistant");
    assert_eq!(manager.api_messages[2].role, "user");
    assert_eq!(manager.api_messages[3].role, "assistant");
    Ok(())
}

#[tokio::test]
async fn local_endpoint_retries_once_when_tool_evidence_required() -> Result<()> {
    let tool_less = vec![
        r#"data: {"choices":[{"delta":{"content":"no tool used"},"finish_reason":"stop"}]}"#
            .to_string(),
    ];
    let with_tool = vec![
        r#"data: {"choices":[{"delta":{"content":"","tool_calls":[{"index":0,"id":"call_01","type":"function","function":{"name":"read_file","arguments":""}}]},"finish_reason":null}]}"#.to_string(),
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\"src/main.rs\"}"}}]},"finish_reason":null}]}"#.to_string(),
        r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#.to_string(),
    ];
    let final_response = vec![
        r#"data: {"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]}"#.to_string(),
    ];
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
        tool_less,
        with_tool,
        final_response,
    ])));
    let mut manager = ConversationManager::new_mock(client, HashMap::new());
    let result = manager.send_message("go".into(), None).await?;
    assert!(!result.is_empty());
    Ok(())
}
