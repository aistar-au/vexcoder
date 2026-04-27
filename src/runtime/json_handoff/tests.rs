use super::*;
use serde_json::json;

#[test]
fn runtime_envelope_serializes_with_required_fields() {
    let envelope = RuntimeEnvelope {
        version: 1,
        task_id: "task-1741700000000".to_string(),
        pulse: 1,
        seq: 3,
        frame_id: "evt:task-1741700000000:1:3".to_string(),
        emitted_at: "2026-04-16T00:00:00.000Z".to_string(),
        source: RuntimeEnvelopeSource::Model,
        request_id: Some("req-1".to_string()),
        parent_frame_id: None,
        signal: RuntimeSignal::ToolCallStarted {
            tool_call_id: "tx_1_9a2f".to_string(),
            tool_name: "read_file".to_string(),
            arguments: json!({"path": "src/app.rs"}),
            status: crate::state::ToolStatus::Pending,
            started_at: "2026-04-16T00:00:00.000Z".to_string(),
        },
    };
    let v = serde_json::to_value(&envelope).unwrap();
    assert_eq!(v["version"], 1);
    assert_eq!(v["source"], "model");
    assert_eq!(v["signal"]["type"], "tool_call_started");
    assert_eq!(v["signal"]["tool_name"], "read_file");
}

#[test]
fn schema_assets_parse_as_json_with_correct_ids() {
    let envelope_schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/runtime_envelope_v1.json")).unwrap();
    let request_schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/runtime_request_v1.json")).unwrap();
    assert_eq!(
        envelope_schema["$id"],
        "https://vexcoder.com/schemas/runtime_envelope_v1.json"
    );
    assert_eq!(
        request_schema["$id"],
        "https://vexcoder.com/schemas/runtime_request_v1.json"
    );
    assert_eq!(envelope_schema["properties"]["version"]["const"], 1);
}

#[test]
fn grammar_asset_contains_required_tool_rules() {
    let grammar = include_str!("../../../grammars/tool_call.gbnf");
    assert!(grammar.contains("tool_call ::= \"{\""));
    assert!(grammar.contains("mcp_tool ::= \"\\\"mcp."));
    assert!(grammar.contains("\\\"read_file\\\""));
}

#[test]
fn normalizer_discards_provider_ids_and_tracks_tool_results() {
    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-1");
    let _ = normalizer.start_pulse(1, Some("ship it".to_string()));

    let tool_call = normalizer
        .normalize_content_block(&ContentBlock::ToolUse {
            id: "provider-call-1".to_string(),
            name: "write_file".to_string(),
            input: json!({"path": "src/main.rs"}),
            metadata: None,
        })
        .pop()
        .expect("tool call envelope");

    let runtime_call_id = match &tool_call.signal {
        RuntimeSignal::ToolCallStarted {
            tool_call_id,
            tool_name,
            arguments,
            ..
        } => {
            assert_ne!(
                tool_call_id, "provider-call-1",
                "provider ID must be replaced"
            );
            assert_eq!(tool_name, "write_file");
            assert_eq!(arguments["path"], "src/main.rs");
            tool_call_id.clone()
        }
        other => panic!("expected ToolCallStarted, got {other:?}"),
    };

    let tool_result = normalizer
        .normalize_content_block(&ContentBlock::ToolResult {
            tool_use_id: "provider-call-1".to_string(),
            content: "ok".to_string(),
            is_error: false,
        })
        .pop()
        .expect("tool result envelope");

    match tool_result.signal {
        RuntimeSignal::ToolCallCompleted {
            tool_call_id,
            status,
            output,
            ..
        } => {
            assert_eq!(
                tool_call_id, runtime_call_id,
                "result must reference runtime ID"
            );
            assert_eq!(status, crate::state::ToolStatus::Complete);
            assert_eq!(output, "ok");
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }
}
