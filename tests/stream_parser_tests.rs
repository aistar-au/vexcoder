use pretty_assertions::assert_eq;
use vexcoder::api::stream::StreamParser;
use vexcoder::runtime::RuntimeEvent;
use vexcoder::state::StreamBlock;

#[test]
fn test_messages_v1_message_start_event_parsed() {
    let mut parser = StreamParser::new();

    let chunk = br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_01","type":"message","role":"assistant","content":[],"model":"local-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}

"#;
    let events = parser
        .process(chunk)
        .expect("message_start event should parse");
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0].event, RuntimeEvent::PulseStart { .. }));
    assert!(matches!(
        &events[1].event,
        RuntimeEvent::UsageUpdated { usage } if usage.input == 25 && usage.output == 1
    ));
}

#[test]
fn test_messages_v1_message_delta_with_stop_reason() {
    let mut parser = StreamParser::new();

    let chunk = br#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":15}}

"#;
    let events = parser
        .process(chunk)
        .expect("message_delta event should parse");
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0].event, RuntimeEvent::PulseStart { .. }));
    assert!(matches!(
        &events[1].event,
        RuntimeEvent::UsageUpdated { usage } if usage.output == 15
    ));
}

#[test]
fn test_messages_v1_text_content_block_start_and_stop() {
    let mut parser = StreamParser::new();

    let start = br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

"#;
    let start_events = parser
        .process(start)
        .expect("text content_block_start should parse");
    assert_eq!(start_events.len(), 2);
    assert!(matches!(
        &start_events[0].event,
        RuntimeEvent::PulseStart { .. }
    ));
    assert!(matches!(
        &start_events[1].event,
        RuntimeEvent::TranscriptBlockStart {
            index: 0,
            block: StreamBlock::Thinking { .. }
        }
    ));

    let stop = br#"event: content_block_stop
data: {"type":"content_block_stop","index":0}

"#;
    let stop_events = parser
        .process(stop)
        .expect("content_block_stop event should parse");
    assert_eq!(stop_events.len(), 1);
    assert!(matches!(
        &stop_events[0].event,
        RuntimeEvent::TranscriptBlockComplete { index: 0 }
    ));
}

#[test]
fn test_fragmented_events() {
    let mut parser = StreamParser::new();

    let chunk1 = b"event: content_block_delta\ndata: {\"type\":\"content";
    let events1 = parser.process(chunk1).expect("first chunk parse");
    assert!(events1.is_empty());

    let chunk2 =
        b"_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n";
    let events2 = parser.process(chunk2).expect("second chunk parse");
    assert_eq!(events2.len(), 3);
    assert!(matches!(&events2[0].event, RuntimeEvent::PulseStart { .. }));
    assert!(matches!(
        &events2[1].event,
        RuntimeEvent::TranscriptBlockStart {
            index: 0,
            block: StreamBlock::Thinking { .. }
        }
    ));
    assert!(matches!(
        &events2[2].event,
        RuntimeEvent::TranscriptBlockDelta { index: 0, delta } if delta == "Hi"
    ));
}

#[test]
fn test_parse_error_handling() {
    let mut parser = StreamParser::new();

    let chunk = b"event: message_start\ndata: {invalid json}\n\n";
    let events = parser
        .process(chunk)
        .expect("error handling should not fail parser");
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].event,
        RuntimeEvent::Error { code, .. } if code == "sse_parse_error"
    ));
}

#[test]
fn test_partial_json_delta_is_parsed() {
    let mut parser = StreamParser::new();

    let start = b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_123\",\"name\":\"write_file\"}}\n\n";
    let start_events = parser
        .process(start)
        .expect("tool_use start without explicit input should parse");
    assert_eq!(start_events.len(), 3);
    assert!(matches!(
        &start_events[0].event,
        RuntimeEvent::PulseStart { .. }
    ));
    assert!(matches!(
        &start_events[1].event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1,
            block: StreamBlock::ToolCall { id, name, input, .. }
        } if id == "toolu_123" && name == "write_file" && input == &serde_json::json!({})
    ));
    assert!(matches!(
        &start_events[2].event,
        RuntimeEvent::ToolCallStarted {
            tool_name,
            arguments,
            ..
        } if tool_name == "write_file" && arguments == &serde_json::json!({})
    ));

    let delta = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"src/\"}}\n\n";
    let delta_events = parser
        .process(delta)
        .expect("parser should parse input_json deltas");
    assert_eq!(delta_events.len(), 2);
    assert!(matches!(
        &delta_events[0].event,
        RuntimeEvent::TranscriptBlockDelta { index: 1, delta } if delta == "{\"path\":\"src/"
    ));
    assert!(matches!(
        &delta_events[1].event,
        RuntimeEvent::ToolCallArgumentsDelta { delta, .. } if delta == "{\"path\":\"src/"
    ));
}

#[test]
fn test_chat_compat_tool_call_stream_maps_to_runtime_events() {
    let mut parser = StreamParser::new();

    let chunk1 = br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Reading file now. "},"finish_reason":null}]}

"#;
    let events1 = parser
        .process(chunk1)
        .expect("chat-compat content delta should parse");
    assert_eq!(events1.len(), 4);
    assert!(matches!(&events1[0].event, RuntimeEvent::PulseStart { .. }));
    assert!(matches!(
        &events1[1].event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata.object.as_deref() == Some("chat.completion.chunk")
    ));
    assert!(matches!(
        &events1[2].event,
        RuntimeEvent::TranscriptBlockStart {
            index: 0,
            block: StreamBlock::Thinking { .. }
        }
    ));
    assert!(matches!(
        &events1[3].event,
        RuntimeEvent::TranscriptBlockDelta { index: 0, delta } if delta == "Reading file now. "
    ));

    let chunk2 = br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"cal.rs\"}"}}]},"finish_reason":"tool_calls"}]}

"#;
    let events2 = parser
        .process(chunk2)
        .expect("chat-compat tool call delta should parse");
    assert_eq!(events2.len(), 7);
    assert!(matches!(
        &events2[0].event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata.object.as_deref() == Some("chat.completion.chunk")
                && metadata.choice_index.is_none()
    ));
    assert!(matches!(
        &events2[1].event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1,
            block: StreamBlock::ToolCall { id, name, .. }
        } if id == "call_abc" && name == "read_file"
    ));
    assert!(matches!(
        &events2[2].event,
        RuntimeEvent::ToolCallStarted { tool_name, .. } if tool_name == "read_file"
    ));
    assert!(matches!(
        &events2[3].event,
        RuntimeEvent::TranscriptBlockDelta { index: 1, delta }
            if delta == "{\"path\":\"cal.rs\"}"
    ));
    assert!(matches!(
        &events2[4].event,
        RuntimeEvent::ToolCallArgumentsDelta { delta, .. }
            if delta == "{\"path\":\"cal.rs\"}"
    ));
    assert!(events2.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ServerMetadata { metadata } if metadata.choice_index == Some(0)
    )));
    assert!(events2.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockComplete { index: 1 }
    )));
}

#[test]
fn test_chat_compat_state_reset_after_done() {
    let mut parser = StreamParser::new();

    let chunk1 = br#"data: {"id":"msg1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}

"#;
    let events1 = parser.process(chunk1).expect("first message");
    assert!(events1.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata.object.as_deref() == Some("chat.completion.chunk")
    )));

    let chunk_done = br#"data: [DONE]

"#;
    let done_events = parser.process(chunk_done).expect("done");
    assert!(done_events.is_empty());

    let chunk2 = br#"data: {"id":"msg2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"World"},"finish_reason":null}]}

"#;
    let events2 = parser.process(chunk2).expect("second message");
    assert!(events2.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata.object.as_deref() == Some("chat.completion.chunk")
    )));
    assert!(events2.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "World"
    )));
}

#[test]
fn test_regression_metadata_only_chunk_not_dropped() {
    let mut parser = StreamParser::new();

    let start = br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":null},"finish_reason":null}],"prompt_progress":{"total":2641,"processed":512,"cache":0,"time_ms":38000},"timings":{"prompt_n":512,"prompt_ms":38000.0,"predicted_n":0,"predicted_ms":0.0}}

"#;
    let events = parser
        .process(start)
        .expect("metadata-only chunk should parse");

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata
                .prompt_progress
                .as_ref()
                .is_some_and(|progress| progress.processed == Some(512))
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata.timings.as_ref().is_some_and(|timings| timings.prompt_ms == Some(38000.0))
    )));
}

#[test]
fn test_regression_progress_updates_across_multiple_chunks() {
    let mut parser = StreamParser::new();

    let chunks = [
        br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":null},"finish_reason":null}],"prompt_progress":{"total":2641,"processed":512,"cache":0,"time_ms":38000}}

"#
        .as_slice(),
        br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":null},"finish_reason":null}],"prompt_progress":{"total":2641,"processed":1024,"cache":0,"time_ms":76000}}

"#
        .as_slice(),
        br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":null},"finish_reason":null}],"prompt_progress":{"total":2641,"processed":2048,"cache":0,"time_ms":153000}}

"#
        .as_slice(),
    ];

    let mut progress_values = Vec::new();
    for chunk in &chunks {
        let events = parser.process(chunk).expect("chunk should parse");
        for event in &events {
            if let RuntimeEvent::ServerMetadata { metadata } = &event.event
                && let Some(progress) = &metadata.prompt_progress
            {
                progress_values.push(progress.processed.unwrap_or(0));
            }
        }
    }

    assert_eq!(progress_values, vec![512, 1024, 2048]);
}

#[test]
fn sse_parser_tracks_last_event_id_from_id_field() {
    let mut parser = StreamParser::new();
    let chunk = b"id: evt-42\ndata: {}\n\n";
    parser.process(chunk).expect("frame should parse");
    assert_eq!(parser.last_event_id(), Some("evt-42"));
}

#[test]
fn sse_parser_updates_last_event_id_across_frames() {
    let mut parser = StreamParser::new();
    parser.process(b"id: first\ndata: {}\n\n").unwrap();
    parser.process(b"id: second\ndata: {}\n\n").unwrap();
    assert_eq!(parser.last_event_id(), Some("second"));
}

#[test]
fn sse_parser_stores_retry_delay_ms() {
    let mut parser = StreamParser::new();
    parser.process(b"retry: 3000\ndata: {}\n\n").unwrap();
    assert_eq!(parser.reconnect_delay_ms(), Some(3000));
}

#[test]
fn sse_parser_ignores_unknown_fields_and_does_not_error() {
    let mut parser = StreamParser::new();
    let chunk = b"custom-field: ignored\ndata: {\"type\":\"ping\"}\n\n";
    let result = parser.process(chunk);
    assert!(result.is_ok(), "unknown field must not cause a parse error");
}

#[test]
fn sse_parser_bare_id_without_colon_clears_last_event_id() {
    let mut parser = StreamParser::new();
    parser
        .process(b"id: stored-id\ndata: {}\n\n")
        .expect("setup frame");
    assert_eq!(parser.last_event_id(), Some("stored-id"));
    parser.process(b"id\ndata: {}\n\n").expect("bare id frame");
    assert_eq!(parser.last_event_id(), Some(""));
}

#[test]
fn sse_parser_id_with_nul_is_ignored() {
    let mut parser = StreamParser::new();
    parser
        .process(b"id: valid-id\ndata: {}\n\n")
        .expect("setup frame");
    parser
        .process(b"id: bad\x00id\ndata: {}\n\n")
        .expect("nul frame");
    assert_eq!(parser.last_event_id(), Some("valid-id"));
}

#[test]
fn sse_parser_non_decimal_retry_value_is_ignored() {
    let mut parser = StreamParser::new();
    parser
        .process(b"retry: abc\ndata: {}\n\n")
        .expect("non-decimal retry frame");
    assert_eq!(parser.reconnect_delay_ms(), None);
}
