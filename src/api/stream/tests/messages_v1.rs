use super::*;
use serde_json::json;

#[test]
fn test_process_messages_v1_materialized_tool_use_inputs_do_not_emit_synthetic_deltas() {
    let cases = vec![
        (
            "object arguments",
            br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_1","name":"read_file","input":{"path":"main.rs","encoding":"utf-8"}}}

"# as &[u8],
            "read_file",
            json!({"path": "main.rs", "encoding": "utf-8"}),
        ),
        (
            "array arguments",
            br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_1","name":"search","input":["main.rs",{"max_depth":2}]}}

"# as &[u8],
            "search",
            json!(["main.rs", {"max_depth": 2}]),
        ),
    ];

    for (label, frame, expected_name, expected_input) in cases {
        let mut parser = StreamParser::new();
        let events = parser.process(frame).unwrap();

        assert!(
            events.iter().any(|event| matches!(
                &event.event,
                RuntimeEvent::TranscriptBlockStart {
                    index: 1,
                    block: crate::state::StreamBlock::ToolCall { id, name, input, .. }
                } if id == "call_1"
                    && name == expected_name
                    && input == &expected_input
            )),
            "{label} must emit a materialized tool-call block start"
        );
        assert!(
            events.iter().any(|event| matches!(
                &event.event,
                RuntimeEvent::ToolCallStarted {
                    tool_name,
                    arguments,
                    ..
                } if tool_name == expected_name && arguments == &expected_input
            )),
            "{label} must emit ToolCallStarted with the materialized JSON input"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(&event.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
            "{label} should not synthesize ToolCallArgumentsDelta events",
        );
    }
}

#[test]
fn test_process_messages_v1_tool_use_delta_emits_tool_call_arguments_delta() {
    let mut parser = StreamParser::new();
    let start = parser
        .process(
            br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_1","name":"read_file","input":{}}}

"#,
        )
        .unwrap();
    let delta = parser
        .process(
            br#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"main.rs\"}"}}

"#,
        )
        .unwrap();

    assert!(start.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1,
            block: crate::state::StreamBlock::ToolCall { id, name, .. }
        } if id == "call_1" && name == "read_file"
    )));
    assert!(delta.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ToolCallArgumentsDelta {
            tool_name,
            delta,
            arguments,
            ..
        } if tool_name.as_deref() == Some("read_file")
            && delta == "{\"path\":\"main.rs\"}"
            && arguments.as_ref() == Some(&json!({"path": "main.rs"}))
    )));
}

#[test]
fn test_process_messages_v1_tool_use_with_null_id_emits_tool_call_started() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":null,"name":"read_file","input":{"path":"main.rs"}}}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::Error {
            code,
            message,
            recoverable,
        } if code == "provider_content_block_start_decode"
            && *recoverable
            && message.contains("invalid type: null")
    )));
}
