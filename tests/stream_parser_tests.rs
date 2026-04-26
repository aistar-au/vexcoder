use vexcoder::api::stream::StreamParser;
use vexcoder::runtime::RuntimeEvent;
use vexcoder::state::StreamBlock;

#[test]
fn messages_v1_parses_full_message_lifecycle_and_usage() {
    let mut parser = StreamParser::new();
    let events = parser.process(br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_01","type":"message","role":"assistant","content":[],"model":"local-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}

"#).unwrap();
    assert!(matches!(&events[0].event, RuntimeEvent::PulseStart { .. }));
    assert!(matches!(&events[1].event, RuntimeEvent::UsageUpdated { usage } if usage.input == 25));

    let events2 = parser.process(br#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":15}}

"#).unwrap();
    assert!(
        matches!(&events2[1].event, RuntimeEvent::UsageUpdated { usage } if usage.output == 15)
    );
}

#[test]
fn text_block_lifecycle_and_fragmented_input_reassemble() {
    let mut parser = StreamParser::new();
    let start_events = parser
        .process(
            br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

"#,
        )
        .unwrap();
    assert!(matches!(
        &start_events[1].event,
        RuntimeEvent::TranscriptBlockStart {
            index: 0,
            block: StreamBlock::Thinking { .. }
        }
    ));

    let stop_events = parser
        .process(
            b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        )
        .unwrap();
    assert!(matches!(
        &stop_events[0].event,
        RuntimeEvent::TranscriptBlockComplete { index: 0 }
    ));

    let mut p2 = StreamParser::new();
    assert!(
        p2.process(b"event: content_block_delta\ndata: {\"type\":\"content")
            .unwrap()
            .is_empty()
    );
    let events = p2
        .process(
            b"_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
        )
        .unwrap();
    assert!(
        matches!(&events[2].event, RuntimeEvent::TranscriptBlockDelta { index: 0, delta } if delta == "Hi")
    );
}

#[test]
fn invalid_json_produces_error_event_and_tool_use_streams_partial_json() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(b"event: message_start\ndata: {invalid json}\n\n")
        .unwrap();
    assert!(
        matches!(&events[0].event, RuntimeEvent::Error { code, .. } if code == "sse_parse_error")
    );

    let mut p2 = StreamParser::new();
    let start = p2.process(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_123\",\"name\":\"write_file\"}}\n\n").unwrap();
    assert!(
        matches!(&start[1].event, RuntimeEvent::TranscriptBlockStart { index: 1, block: StreamBlock::ToolCall { id, name, .. } } if id == "toolu_123" && name == "write_file")
    );
    let delta = p2.process(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"src/\"}}\n\n").unwrap();
    assert!(
        matches!(&delta[1].event, RuntimeEvent::ToolCallArgumentsDelta { delta, .. } if delta == "{\"path\":\"src/")
    );
}

#[test]
fn chat_compat_protocol_text_and_tool_call_and_state_reset() {
    let mut parser = StreamParser::new();
    let events1 = parser.process(br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Reading file now. "},"finish_reason":null}]}

"#).unwrap();
    assert!(
        matches!(&events1[3].event, RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "Reading file now. ")
    );

    let events2 = parser.process(br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"cal.rs\"}"}}]},"finish_reason":"tool_calls"}]}

"#).unwrap();
    assert!(events2.iter().any(|e| matches!(&e.event, RuntimeEvent::ToolCallStarted { tool_name, .. } if tool_name == "read_file")));

    parser.process(b"data: [DONE]\n\n").unwrap();
    let events3 = parser.process(br#"data: {"id":"msg2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"World"},"finish_reason":null}]}

"#).unwrap();
    assert!(events3.iter().any(
        |e| matches!(&e.event, RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "World")
    ));
}

#[test]
fn sse_protocol_id_retry_and_edge_case_fields() {
    let mut parser = StreamParser::new();
    parser.process(b"id: evt-42\ndata: {}\n\n").unwrap();
    assert_eq!(parser.last_event_id(), Some("evt-42"));
    parser.process(b"id: second\ndata: {}\n\n").unwrap();
    assert_eq!(parser.last_event_id(), Some("second"));

    parser.process(b"retry: 3000\ndata: {}\n\n").unwrap();
    assert_eq!(parser.reconnect_delay_ms(), Some(3000));

    parser.process(b"retry: abc\ndata: {}\n\n").unwrap();
    assert_eq!(
        parser.reconnect_delay_ms(),
        Some(3000),
        "non-decimal retry must be ignored"
    );

    assert!(
        parser
            .process(b"custom-field: ignored\ndata: {\"type\":\"ping\"}\n\n")
            .is_ok()
    );

    parser.process(b"id: stored\ndata: {}\n\n").unwrap();
    parser.process(b"id\ndata: {}\n\n").unwrap();
    assert_eq!(
        parser.last_event_id(),
        Some(""),
        "bare id must clear last event id"
    );

    parser.process(b"id: valid\ndata: {}\n\n").unwrap();
    parser.process(b"id: bad\x00id\ndata: {}\n\n").unwrap();
    assert_eq!(
        parser.last_event_id(),
        Some("valid"),
        "nul in id must be ignored"
    );
}
