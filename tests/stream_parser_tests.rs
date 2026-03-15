use vexcoder::api::stream::StreamParser;
use vexcoder::types::{ContentBlock, StreamEvent};

#[test]
fn test_fragmented_events() {
    let mut parser = StreamParser::new();

    let chunk1 = b"event: content_block_delta\ndata: {\"type\":\"content";
    let events1 = parser.process(chunk1).expect("first chunk parse");
    assert_eq!(events1.len(), 0);

    let chunk2 =
        b"_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n";
    let events2 = parser.process(chunk2).expect("second chunk parse");
    assert_eq!(events2.len(), 1);
}

#[test]
fn test_parse_error_handling() {
    let mut parser = StreamParser::new();

    let chunk = b"event: message_start\ndata: {invalid json}\n\n";
    let events = parser
        .process(chunk)
        .expect("error handling should not fail parser");
    assert_eq!(events.len(), 0);
}

#[test]
fn test_partial_json_delta_is_parsed() {
    let mut parser = StreamParser::new();

    let chunk = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"src/\"}}\n\n";
    let events = parser
        .process(chunk)
        .expect("parser should parse input_json deltas");
    assert_eq!(events.len(), 1);

    match &events[0] {
        StreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(*index, 1);
            assert_eq!(delta.partial_json.as_deref(), Some("{\"path\":\"src/"));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_tool_use_start_without_input_is_accepted() {
    let mut parser = StreamParser::new();

    let chunk = b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_123\",\"name\":\"write_file\"}}\n\n";
    let events = parser
        .process(chunk)
        .expect("tool_use start without explicit input should parse");
    assert_eq!(events.len(), 1);

    match &events[0] {
        StreamEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            assert_eq!(*index, 1);
            match content_block {
                ContentBlock::ToolUse {
                    id, name, input, ..
                } => {
                    assert_eq!(id, "toolu_123");
                    assert_eq!(name, "write_file");
                    assert_eq!(input, &serde_json::json!({}));
                }
                other => panic!("unexpected block type: {other:?}"),
            }
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_chat_compat_tool_call_stream_maps_to_unified_events() {
    let mut parser = StreamParser::new();

    let chunk1 = br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Reading file now. "},"finish_reason":null}]}

"#;
    let events1 = parser
        .process(chunk1)
        .expect("chat-compat content delta should parse");
    assert_eq!(events1.len(), 2);
    match &events1[0] {
        StreamEvent::MessageStart { message } => {
            assert_eq!(message.id, "chatcmpl-1");
            assert_eq!(message.role, "assistant");
            assert!(message.metadata.is_some());
        }
        other => panic!("unexpected event: {other:?}"),
    }
    match &events1[1] {
        StreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(*index, 0);
            assert_eq!(delta.text.as_deref(), Some("Reading file now. "));
            assert_eq!(delta.choice_index, Some(0));
        }
        other => panic!("unexpected event: {other:?}"),
    }

    let chunk2 = br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"cal.rs\"}"}}]},"finish_reason":"tool_calls"}]}

"#;
    let events2 = parser
        .process(chunk2)
        .expect("chat-compat tool call delta should parse");
    assert_eq!(events2.len(), 4);

    match &events2[0] {
        StreamEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            assert_eq!(*index, 1);
            match content_block {
                ContentBlock::ToolUse {
                    id, name, metadata, ..
                } => {
                    assert_eq!(id, "call_abc");
                    assert_eq!(name, "read_file");
                    let metadata = metadata.as_ref().expect("tool metadata should be present");
                    assert_eq!(metadata.call_type.as_deref(), Some("function"));
                    assert_eq!(metadata.choice_index, Some(0));
                }
                other => panic!("unexpected block: {other:?}"),
            }
        }
        other => panic!("unexpected event: {other:?}"),
    }

    match &events2[1] {
        StreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(*index, 1);
            assert_eq!(delta.partial_json.as_deref(), Some("{\"path\":\"cal.rs\"}"));
            assert_eq!(delta.choice_index, Some(0));
        }
        other => panic!("unexpected event: {other:?}"),
    }

    match &events2[2] {
        StreamEvent::MessageDelta { delta, usage } => {
            assert!(usage.is_none());
            assert_eq!(delta.stop_reason.as_deref(), Some("tool_calls"));
            let metadata = delta.metadata.as_ref().expect("metadata should be present");
            assert_eq!(metadata.choice_index, Some(0));
        }
        other => panic!("unexpected event: {other:?}"),
    }

    match &events2[3] {
        StreamEvent::ContentBlockStop { index } => assert_eq!(*index, 1),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_chat_compat_state_reset_after_done() {
    let mut parser = StreamParser::new();

    let chunk1 = br#"data: {"id":"msg1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}

"#;
    let events1 = parser.process(chunk1).expect("first message");
    assert!(matches!(
        events1.first(),
        Some(StreamEvent::MessageStart { .. })
    ));

    let chunk_done = br#"data: [DONE]

"#;
    let _events_done = parser.process(chunk_done).expect("done");

    let chunk2 = br#"data: {"id":"msg2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"World"},"finish_reason":null}]}

"#;
    let events2 = parser.process(chunk2).expect("second message");
    let has_message_start = events2
        .iter()
        .any(|e| matches!(e, StreamEvent::MessageStart { .. }));
    assert!(
        has_message_start,
        "second message should emit MessageStart after [DONE]"
    );
}
