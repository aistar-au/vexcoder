use super::*;
use serde_json::json;

#[test]
fn materialized_tool_use_emits_block_start_not_delta() {
    for (label, frame, name, input) in [
        ("object", br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_1","name":"read_file","input":{"path":"main.rs"}}}

"# as &[u8], "read_file", json!({"path": "main.rs"})),
    ] {
        let mut parser = StreamParser::new();
        let events = parser.process(frame).unwrap();
        assert!(events.iter().any(|e| matches!(&e.event, RuntimeEvent::TranscriptBlockStart { index: 1, block: crate::state::StreamBlock::ToolCall { id, name: n, input: i, .. } } if id == "call_1" && n == name && i == &input)), "{label}: must emit materialized tool-call block");
        assert!(!events.iter().any(|e| matches!(&e.event, RuntimeEvent::ToolCallArgumentsDelta { .. })), "{label}: must not emit ToolCallArgumentsDelta for materialized input");
    }
}

#[test]
fn streaming_tool_use_delta_emits_arguments_delta() {
    let mut parser = StreamParser::new();
    parser.process(br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_1","name":"read_file","input":{}}}

"#).unwrap();
    let delta = parser.process(br#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"main.rs\"}"}}

"#).unwrap();
    assert!(delta.iter().any(|e| matches!(&e.event, RuntimeEvent::ToolCallArgumentsDelta { tool_name, delta, .. } if tool_name.as_deref() == Some("read_file") && delta == "{\"path\":\"main.rs\"}")));
}

#[test]
fn null_tool_use_id_emits_recoverable_error() {
    let mut parser = StreamParser::new();
    let events = parser.process(br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":null,"name":"read_file","input":{}}}

"#).unwrap();
    assert!(events.iter().any(|e| matches!(&e.event, RuntimeEvent::Error { code, recoverable, .. } if code == "provider_content_block_start_decode" && *recoverable)));
}
