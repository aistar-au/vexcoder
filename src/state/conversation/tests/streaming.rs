use super::*;

#[tokio::test]
async fn test_crit_01_protocol_flow() -> Result<()> {
    // ANCHOR: This test verifies the multi-turn conversation protocol.
    // It will PASS if the protocol is correctly implemented.
    //
    // The test should:
    // 1. Create a ConversationManager with a mock client
    // 2. Send a message that triggers tool use
    // 3. Verify the tool is executed
    // 4. Verify the final response incorporates tool results

    // Mock responses for the API client
    let first_response_sse = vec![
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

    let second_response_sse = vec![
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

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])))
        .with_structured_tool_protocol(false);

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "Hello from file.txt".to_string());

    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager
        .send_message("What is in file.txt?".into(), None)
        .await?;

    assert!(final_text.contains("The content of file.txt is 'Hello from file.txt'"));

    // Verify the message history order
    let messages = &manager.api_messages;
    assert_eq!(messages.len(), 4);

    // Initial user message
    assert_eq!(messages[0].role, "user");
    if let Content::Text(text) = &messages[0].content {
        assert!(text.contains("What is in file.txt?"));
    }

    // Assistant message with tool_use
    assert_eq!(messages[1].role, "assistant");
    if let Content::Blocks(blocks) = &messages[1].content {
        assert_eq!(blocks.len(), 2);
        if let ContentBlock::Text { text, .. } = &blocks[0] {
            assert!(text.contains("Okay, I can help with that."));
        }
        if let ContentBlock::ToolUse {
            id: _, name, input, ..
        } = &blocks[1]
        {
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({ "path": "file.txt" }));
        }
    }

    // User message with tool_result
    assert_eq!(messages[2].role, "user");
    if let Content::Blocks(blocks) = &messages[2].content {
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolResult {
            tool_use_id: _,
            content,
            is_error,
        } = &blocks[0]
        {
            assert!(content.contains("Read file.txt:"));
            assert!(content.contains("Content for model context:"));
            assert!(content.contains("Hello from file.txt"));
            assert!(!is_error);
        }
    }

    // Final assistant message
    assert_eq!(messages[3].role, "assistant");
    if let Content::Blocks(blocks) = &messages[3].content {
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::Text { text, .. } = &blocks[0] {
            assert!(text.contains("The content of file.txt is 'Hello from file.txt'"));
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_tool_call_argument_delta_emits_typed_stream_update() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type": "message_start", "message": {"id": "msg_mock_args_01", "type": "message", "role": "assistant", "model": "mock-model", "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}"#.to_string(),
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

    let second_response_sse = vec![
        r#"event: message_start
data: {"type": "message_start", "message": {"id": "msg_mock_args_02", "type": "message", "role": "assistant", "model": "mock-model", "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type": "content_block_start", "index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type": "content_block_delta", "index":0,"delta":{"type":"text_delta","text":"The content of file.txt is 'Hello from file.txt'"}}"#.to_string(),
        r#"event: message_delta
data: {"type": "message_delta", "delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
        r#"event: message_stop
data: {"type": "message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])))
        .with_structured_tool_protocol(false);

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "Hello from file.txt".to_string());

    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let tx_for_send = tx.clone();
    let mut send_future = std::pin::pin!(
        manager.send_message("What is in file.txt?".to_string(), Some(&tx_for_send))
    );

    let mut saw_block_delta = false;
    let mut saw_typed_update = false;

    let final_text = loop {
        tokio::select! {
            result = &mut send_future => break result?,
            maybe_update = rx.recv() => {
                let Some(update) = maybe_update else { continue; };
                match update {
                    ConversationStreamUpdate::BlockDelta { index, delta } => {
                        if index == 1 && delta.contains("file.txt") {
                            saw_block_delta = true;
                        }
                    }
                    ConversationStreamUpdate::ToolCallArgumentsUpdated {
                        tool_call_id,
                        tool_name: _,
                        arguments,
                    } => {
                        if !tool_call_id.is_empty() && arguments == json!({"path": "file.txt"}) {
                            saw_typed_update = true;
                        }
                    }
                    ConversationStreamUpdate::ToolApprovalRequest(request) => {
                        let _ = request.response_tx.send(true);
                    }
                    ConversationStreamUpdate::Delta(_)
                    | ConversationStreamUpdate::BlockStart { .. }
                    | ConversationStreamUpdate::BlockComplete { .. }
                    | ConversationStreamUpdate::TranscriptLine(_)
                    | ConversationStreamUpdate::ServerMetadata(_)
                    | ConversationStreamUpdate::CommandSessionStarted { .. }
                    | ConversationStreamUpdate::CommandSessionAttached { .. }
                    | ConversationStreamUpdate::CommandSessionFinished { .. }
                    | ConversationStreamUpdate::ContextCompacted { .. }
                    | ConversationStreamUpdate::StreamError(_) => {}
                }
            }
        }
    };

    assert!(
        saw_block_delta,
        "expected raw block delta for envelope projection"
    );
    assert!(
        saw_typed_update,
        "expected typed tool-call arguments update for downstream consumers"
    );
    assert!(final_text.contains("Hello from file.txt"));

    Ok(())
}

#[tokio::test]
async fn test_structured_text_only_round_streams_final_text_block() -> Result<()> {
    let response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_text_only_1","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"This is the final answer."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":8}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            response_sse,
        ])));

    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let final_text = manager
        .send_message("say hi".to_string(), Some(&tx))
        .await?;
    assert_eq!(final_text, "This is the final answer.");

    drop(tx);

    let mut saw_thinking_start = false;
    let mut saw_final_start = false;
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart { block, .. } = update {
            match block {
                StreamBlock::Thinking { .. } => saw_thinking_start = true,
                StreamBlock::FinalText { .. } => saw_final_start = true,
                StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
            }
        }
    }

    assert!(saw_thinking_start);
    assert!(saw_final_start);
    Ok(())
}
#[tokio::test]
async fn test_structured_tool_then_final_round_streams_thinking_then_final_text() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_1","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read the file."}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_mock_round_1","name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"file.txt\"}"}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_2","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"The file says hello."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":7}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "hello".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut saw_thinking_start = false;
    let mut saw_final_start = false;

    let tx_for_send = tx.clone();
    let mut send_future =
        std::pin::pin!(manager.send_message("read file".to_string(), Some(&tx_for_send)));
    let final_text = loop {
        tokio::select! {
            result = &mut send_future => break result?,
            maybe_update = rx.recv() => {
                let Some(update) = maybe_update else { continue; };
                match update {
                    ConversationStreamUpdate::BlockStart { block, .. } => {
                        match block {
                            StreamBlock::Thinking { .. } => saw_thinking_start = true,
                            StreamBlock::FinalText { .. } => saw_final_start = true,
                            StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                        }
                    }
                    ConversationStreamUpdate::ToolApprovalRequest(request) => {
                        let _ = request.response_tx.send(true);
                    }
                    ConversationStreamUpdate::Delta(_)
                    | ConversationStreamUpdate::BlockDelta { .. }
                    | ConversationStreamUpdate::ToolCallArgumentsUpdated { .. }
                    | ConversationStreamUpdate::BlockComplete { .. }
                    | ConversationStreamUpdate::TranscriptLine(_)
                    | ConversationStreamUpdate::ServerMetadata(_)
                    | ConversationStreamUpdate::CommandSessionStarted { .. }
                    | ConversationStreamUpdate::CommandSessionAttached { .. }
                    | ConversationStreamUpdate::CommandSessionFinished { .. }
                    | ConversationStreamUpdate::ContextCompacted { .. }
                    | ConversationStreamUpdate::StreamError(_) => {}
                }
            }
        }
    };
    assert_eq!(final_text, "The file says hello.");
    drop(tx);

    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart { block, .. } = update {
            match block {
                StreamBlock::Thinking { .. } => saw_thinking_start = true,
                StreamBlock::FinalText { .. } => saw_final_start = true,
                StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
            }
        }
    }

    assert!(saw_thinking_start);
    assert!(saw_final_start);
    Ok(())
}
#[tokio::test]
async fn test_text_tagged_tool_call_executes_as_fallback_for_local_endpoint() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_10","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I'll read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_11","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Read complete: Hello from fallback."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "Hello from fallback.".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("Read file".into(), None).await?;
    assert!(final_text.contains("Read complete: Hello from fallback."));

    let messages = &manager.api_messages;
    assert!(
        messages.iter().any(|message| {
            if message.role != "assistant" {
                return false;
            }
            match &message.content {
                Content::Text(text) => {
                    text.contains("I'll read it.") && text.contains("<function=read_file>")
                }
                _ => false,
            }
        }),
        "expected fallback parser to persist text-protocol tool call markup"
    );
    assert!(
        messages.iter().any(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("tool_result read_file")
                )
        }),
        "expected fallback parser to execute read_file and append tool_result text"
    );

    Ok(())
}
#[tokio::test]
async fn test_text_tagged_tool_call_emits_structured_tool_blocks_for_fallback() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_fallback_20","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_fallback_21","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "Hello from fallback.".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut saw_tool_call_block = false;
    let tx_for_send = tx.clone();
    let mut send_future =
        std::pin::pin!(manager.send_message("Read file".to_string(), Some(&tx_for_send)));
    let _final_text = loop {
        tokio::select! {
            result = &mut send_future => break result?,
            maybe_update = rx.recv() => {
                let Some(update) = maybe_update else { continue; };
                match update {
                    ConversationStreamUpdate::BlockStart { block, .. } => {
                        if matches!(block, StreamBlock::ToolCall { ref name, .. } if name == "read_file") {
                            saw_tool_call_block = true;
                        }
                    }
                    ConversationStreamUpdate::ToolApprovalRequest(request) => {
                        let _ = request.response_tx.send(true);
                    }
                    ConversationStreamUpdate::Delta(_)
                    | ConversationStreamUpdate::BlockDelta { .. }
                    | ConversationStreamUpdate::ToolCallArgumentsUpdated { .. }
                    | ConversationStreamUpdate::BlockComplete { .. }
                    | ConversationStreamUpdate::TranscriptLine(_)
                    | ConversationStreamUpdate::ServerMetadata(_)
                    | ConversationStreamUpdate::CommandSessionStarted { .. }
                    | ConversationStreamUpdate::CommandSessionAttached { .. }
                    | ConversationStreamUpdate::CommandSessionFinished { .. }
                    | ConversationStreamUpdate::ContextCompacted { .. }
                    | ConversationStreamUpdate::StreamError(_) => {}
                }
            }
        }
    };

    drop(tx);
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart { block, .. } = update
            && matches!(block, StreamBlock::ToolCall { ref name, .. } if name == "read_file")
        {
            saw_tool_call_block = true;
        }
    }

    assert!(saw_tool_call_block);
    Ok(())
}

#[tokio::test]
async fn test_text_tagged_tool_call_with_wrapper_round_trip_sanitizes_history() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_wrapper_30","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<tool_call>\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_wrapper_31","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert(
        "file.txt".to_string(),
        "Hello from wrapper fallback.".to_string(),
    );
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("Read file".into(), None).await?;
    assert!(final_text.contains("done"));

    assert!(
        manager.api_messages.iter().any(|message| {
            if message.role != "assistant" {
                return false;
            }
            match &message.content {
                Content::Text(text) => {
                    text.contains("I will read it.")
                        && text.contains("<function=read_file>")
                        && !text.contains("<tool_call>")
                }
                _ => false,
            }
        }),
        "wrapper markers must be stripped from persisted assistant history"
    );
    assert!(
        manager.api_messages.iter().any(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("tool_result read_file")
                )
        }),
        "wrapper-tagged calls must still execute and append tool_result context"
    );

    Ok(())
}

#[tokio::test]
async fn test_text_xml_tool_call_executes_with_default_local_hybrid_fallback() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_xml_40","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<tool_call>\n{\"name\":\"read_file\",\"arguments\":{\"path\":\"file.txt\"}}\n</tool_call>"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_xml_41","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Read complete: Hello from xml fallback."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert(
        "file.txt".to_string(),
        "Hello from xml fallback.".to_string(),
    );
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("Read file".into(), None).await?;
    assert!(final_text.contains("Read complete: Hello from xml fallback."));

    assert!(
        manager.api_messages.iter().any(|message| {
            if message.role != "assistant" {
                return false;
            }
            match &message.content {
                Content::Text(text) => {
                    text.contains("I will read it.")
                        && text.contains("<function=read_file>")
                        && !text.contains("<tool_call>")
                }
                _ => false,
            }
        }),
        "generic XML fallback should normalize persisted history into the tagged text protocol"
    );
    assert!(
        manager.api_messages.iter().any(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("tool_result read_file")
                )
        }),
        "generic XML fallback must still execute the parsed tool call"
    );

    Ok(())
}

#[tokio::test]
async fn test_chat_compat_stream_tool_call_round_trip() -> Result<()> {
    let first_response_sse = vec![
        r#"data: {"id":"chatcmpl_mock_1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"I'll read it. "},"finish_reason":null}]}"#.to_string(),
        r#"data: {"id":"chatcmpl_mock_1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_mock_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"file.txt\"}"}}]},"finish_reason":"tool_calls"}]}"#.to_string(),
        "data: [DONE]".to_string(),
    ];

    let second_response_sse = vec![
        r#"data: {"id":"chatcmpl_mock_2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"The content is Hello from chat-compat stream."},"finish_reason":"stop"}]}"#.to_string(),
        "data: [DONE]".to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert(
        "file.txt".to_string(),
        "Hello from chat-compat stream.".to_string(),
    );
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("Read file".into(), None).await?;
    assert!(final_text.contains("Hello from chat-compat stream."));

    let messages = &manager.api_messages;
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1].role, "assistant");
    if let Content::Blocks(blocks) = &messages[1].content {
        assert!(blocks.iter().any(
            |block| matches!(block, ContentBlock::ToolUse { name, .. } if name == "read_file")
        ));
    } else {
        panic!("expected assistant blocks");
    }

    Ok(())
}
#[tokio::test]
async fn test_local_text_protocol_tool_round_trip() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_local_10","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mock_local_11","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Tool result consumed."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])))
        .with_structured_tool_protocol(false);

    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert(
        "file.txt".to_string(),
        "Hello local text protocol.".to_string(),
    );
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("Read file".into(), None).await?;
    assert!(final_text.contains("Tool result consumed."));

    let messages = &manager.api_messages;
    assert!(
        messages.iter().any(|message| {
            if message.role != "assistant" {
                return false;
            }
            match &message.content {
                Content::Text(text) => {
                    text.contains("I will read it.") && text.contains("<function=read_file>")
                }
                _ => false,
            }
        }),
        "expected fallback to preserve rendered text-protocol tool call in history"
    );
    assert!(
        messages.iter().any(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("tool_result read_file")
                )
        }),
        "expected text protocol tool_result payload to be appended for the next round"
    );

    Ok(())
}
#[tokio::test]
async fn test_local_endpoint_retries_once_when_tool_evidence_required() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me check that."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_02","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<function=read_file>\n<parameter=path>\nCargo.toml\n</parameter>\n</function>"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let third_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_03","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Tool-backed summary complete."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
            third_response_sse,
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert(
        "Cargo.toml".to_string(),
        "[package]\nname = \"vexcoder\"".to_string(),
    );
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager
        .send_message("how many files are in this tree".to_string(), None)
        .await?;
    assert!(final_text.contains("Tool-backed summary complete."));

    let correction_count = manager
        .api_messages
        .iter()
        .filter(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("did not execute any tool call")
                )
        })
        .count();
    assert_eq!(
        correction_count, 1,
        "expected exactly one corrective tool-use retry message"
    );

    Ok(())
}
#[tokio::test]
async fn test_local_endpoint_retry_only_once_when_model_stays_toolless() -> Result<()> {
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_once_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me inspect that."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_once_02","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Still no tool call."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let third_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_retry_once_03","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Still no tool call."}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
            third_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let final_text = manager
        .send_message("show me the file count".to_string(), None)
        .await?;
    assert!(final_text.contains("Still no tool call."));
    assert!(
        final_text.contains("[loop guard]"),
        "tool-evidence-required prompts must return guard text when model stays toolless"
    );

    let correction_count = manager
        .api_messages
        .iter()
        .filter(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("did not execute any tool call")
                )
        })
        .count();
    assert_eq!(
        correction_count, 2,
        "retry message should be inserted twice"
    );

    Ok(())
}
#[tokio::test]
async fn test_repeated_read_only_round_injects_nudge_then_recovers() -> Result<()> {
    // Local endpoints use a tighter repeat threshold (1 vs 2) so the nudge
    // fires after just one repeated round. Sequence: initial round → repeated
    // round triggers nudge → recovery round produces final text.
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            tagged_read_file_round("msg_loop_nudge_01"),
            tagged_read_file_round("msg_loop_nudge_02"),
            plain_text_round("msg_loop_nudge_03", "Done after loop correction."),
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "loop sample".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("read file".to_string(), None).await?;
    assert!(final_text.contains("Done after loop correction."));

    let nudge_count = manager
        .api_messages
        .iter()
        .filter(|message| {
            message.role == "user"
                && matches!(
                    &message.content,
                    Content::Text(text) if text.contains("Do not repeat identical tool calls")
                )
        })
        .count();
    assert_eq!(nudge_count, 1, "expected exactly one loop-correction nudge");

    Ok(())
}

#[tokio::test]
async fn test_duplicate_tagged_read_only_calls_in_single_round_are_deduplicated() -> Result<()> {
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            tagged_duplicate_read_file_round("msg_loop_dedupe_01"),
            plain_text_round("msg_loop_dedupe_02", "Done after dedup."),
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "loop sample".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("read file".to_string(), None).await?;
    assert!(final_text.contains("Done after dedup."));

    let assistant_history = manager
        .api_messages
        .iter()
        .find_map(|message| match &message.content {
            Content::Text(text)
                if message.role == "assistant" && text.contains("<function=read_file>") =>
            {
                Some(text.as_str())
            }
            _ => None,
        })
        .expect("expected assistant history with tagged tool call");
    assert_eq!(assistant_history.matches("<function=read_file>").count(), 1);

    let tool_result_history = manager
        .api_messages
        .iter()
        .find_map(|message| match &message.content {
            Content::Text(text)
                if message.role == "user" && text.contains("tool_result read_file") =>
            {
                Some(text.as_str())
            }
            _ => None,
        })
        .expect("expected tool_result history");
    assert_eq!(
        tool_result_history.matches("tool_result read_file").count(),
        1
    );

    Ok(())
}

#[tokio::test]
async fn test_repeated_read_only_round_returns_guard_message_instead_of_error() -> Result<()> {
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            tagged_read_file_round("msg_loop_guard_01"),
            tagged_read_file_round("msg_loop_guard_02"),
            tagged_read_file_round("msg_loop_guard_03"),
            tagged_read_file_round("msg_loop_guard_04"),
        ])));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "loop sample".to_string());
    let mut manager = ConversationManager::new_mock(mock_api_client, mock_tool_responses);

    let final_text = manager.send_message("read file".to_string(), None).await?;
    assert!(final_text.contains("[loop guard]"));
    assert!(final_text.contains("Repeated identical read/search tool calls"));

    Ok(())
}
#[tokio::test]
async fn test_repeated_mutating_round_returns_guard_message_instead_of_looping() -> Result<()> {
    let mutating_round = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mut_loop_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_mut_loop_01","name":"edit_file","input":{"path":"src/calculator.rs","old_str":"","new_str":"x"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            mutating_round.clone(),
            mutating_round,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let final_text = manager
        .send_message("edit calculator".to_string(), None)
        .await?;
    assert!(final_text.contains("[loop guard]"));
    assert!(final_text.contains("Repeated identical mutating tool calls"));
    Ok(())
}
