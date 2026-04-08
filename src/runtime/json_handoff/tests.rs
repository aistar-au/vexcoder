use super::*;
use crate::state::ToolApprovalRequest;
use serde_json::json;
use tokio::sync::oneshot;

#[test]
fn test_pi_09_anchor_runtime_envelope_serde_shape() {
    let envelope = RuntimeEnvelope {
        version: 1,
        task_id: "task-1741700000000".to_string(),
        turn: 1,
        seq: 3,
        event: RuntimeEvent::ToolCall {
            id: "call_1741700123456_9a2f".to_string(),
            name: "read_file".to_string(),
            arguments: json!({
                "path": "src/app.rs"
            }),
        },
    };

    let value = serde_json::to_value(&envelope).expect("runtime envelope must serialize");
    assert_eq!(value["version"], 1);
    assert_eq!(value["task_id"], "task-1741700000000");
    assert_eq!(value["turn"], 1);
    assert_eq!(value["seq"], 3);
    assert_eq!(value["event"]["type"], "tool_call");
    assert_eq!(value["event"]["id"], "call_1741700123456_9a2f");
    assert_eq!(value["event"]["name"], "read_file");
    assert_eq!(value["event"]["arguments"]["path"], "src/app.rs");
}

#[test]
fn test_pi_11_schema_assets_parse_as_json() {
    let envelope_schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/runtime_envelope_v1.json"))
            .expect("runtime envelope schema must parse");
    let request_schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/runtime_request_v1.json"))
            .expect("runtime request schema must parse");

    assert_eq!(
        envelope_schema["$id"],
        "https://vexcoder.io/schemas/runtime_envelope_v1.json"
    );
    assert_eq!(
        request_schema["$id"],
        "https://vexcoder.io/schemas/runtime_request_v1.json"
    );
    assert_eq!(envelope_schema["properties"]["version"]["const"], 1);
    assert_eq!(
        envelope_schema["$defs"]["tool_name"]["pattern"],
        "^([a-z][a-z0-9_-]*|mcp\\.[a-z][a-z0-9_-]*\\.[a-z][a-z0-9_-]*)$"
    );
    assert_eq!(
        envelope_schema["$defs"]["transcript_line"]["properties"]["type"]["const"],
        "transcript_line"
    );
    assert_eq!(
        envelope_schema["$defs"]["transcript_block_start"]["properties"]["type"]["const"],
        "transcript_block_start"
    );
    assert_eq!(
        envelope_schema["$defs"]["stream_block_tool_call"]["properties"]["name"]["$ref"],
        "#/$defs/tool_name"
    );
    assert_eq!(request_schema["$defs"]["scope"]["enum"][0], "once");
    assert_eq!(request_schema["$defs"]["scope"]["enum"][1], "session");
}

#[test]
fn test_pi_11_tool_call_grammar_keeps_mcp_namespace_rule() {
    let grammar = include_str!("../../../grammars/tool_call.gbnf");

    assert!(grammar.contains("tool_call ::= \"{\""));
    assert!(grammar.contains("mcp_tool ::= \"\\\"mcp."));
    assert!(grammar.contains("\\\"read_file\\\""));
    assert!(grammar.contains("\\\"apply_patch\\\""));
    assert!(grammar.contains("\"*\" | \"?\""));
}

#[test]
fn test_pi_10_normalization_discards_provider_ids_and_tracks_results() {
    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-1");
    let start = normalizer.start_turn(1, Some("ship it".to_string()));
    assert_eq!(start.seq, 1);

    let tool_call = normalizer
        .normalize_content_block(&ContentBlock::ToolUse {
            id: "provider-call-1".to_string(),
            name: "write_file".to_string(),
            input: json!({"path":"src/main.rs"}),
            metadata: None,
        })
        .pop()
        .expect("tool call envelope");

    let runtime_call_id = match &tool_call.event {
        RuntimeEvent::ToolCall {
            id,
            name,
            arguments,
        } => {
            assert_ne!(id, "provider-call-1");
            assert_runtime_tool_id(id);
            assert_eq!(name, "write_file");
            assert_eq!(arguments["path"], "src/main.rs");
            id.clone()
        }
        other => panic!("expected tool_call event, got {other:?}"),
    };

    let tool_result = normalizer
        .normalize_content_block(&ContentBlock::ToolResult {
            tool_use_id: "provider-call-1".to_string(),
            content: "ok".to_string(),
            is_error: false,
        })
        .pop()
        .expect("tool result envelope");

    match tool_result.event {
        RuntimeEvent::ToolResult {
            tool_call_id,
            tool_name,
            is_error,
            output,
        } => {
            assert_eq!(tool_call_id, runtime_call_id);
            assert_eq!(tool_name.as_deref(), Some("write_file"));
            assert!(!is_error);
            assert_eq!(output, "ok");
        }
        other => panic!("expected tool_result event, got {other:?}"),
    }

    let grammar_calls = normalizer.normalize_tool_call_array(&json!([
        {"name":"read_file","arguments":{"path":"src/lib.rs"}},
        {"name":"apply_patch","arguments":{"path":"src/lib.rs"}}
    ]));
    assert_eq!(grammar_calls.len(), 2);
    assert_eq!(grammar_calls[0].seq, 4);
    assert_eq!(grammar_calls[1].seq, 5);
}

#[test]
fn test_pi_10_normalization_projects_ui_updates_and_approval_events() {
    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-2");
    let _ = normalizer.start_turn(1, Some("review".to_string()));

    let delta = normalizer.normalize_ui_update(
        &UiUpdate::StreamDelta("partial model response".to_string()),
        None,
    );
    assert_eq!(delta.len(), 2);
    assert_eq!(delta[0].seq, 2);
    let final_text_index = match &delta[0].event {
        RuntimeEvent::TranscriptBlockStart {
            index,
            block: StreamBlock::FinalText { content },
        } => {
            assert!(content.is_empty());
            *index
        }
        other => panic!("expected final-text block start, got {other:?}"),
    };
    assert!(matches!(
        delta[1].event,
        RuntimeEvent::TranscriptBlockDelta {
            index,
            ref delta,
        } if index == final_text_index && delta == "partial model response"
    ));

    let transcript_line = normalizer.normalize_ui_update(
        &UiUpdate::TranscriptLine("[edit loop: running validation]".to_string()),
        None,
    );
    assert_eq!(transcript_line.len(), 2);
    assert!(matches!(
        transcript_line[0].event,
        RuntimeEvent::TranscriptBlockComplete { index } if index == final_text_index
    ));
    assert!(matches!(
        transcript_line[1].event,
        RuntimeEvent::TranscriptLine { ref line }
            if line == "[edit loop: running validation]"
    ));

    let transcript_block_start = normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "provider-call-1".to_string(),
                name: "read_file".to_string(),
                input: json!({"path":"src/lib.rs"}),
                status: crate::state::ToolStatus::Pending,
            },
        },
        None,
    );
    assert_eq!(transcript_block_start.len(), 2);
    assert!(matches!(
        transcript_block_start[0].event,
        RuntimeEvent::TranscriptBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                ref name,
                ref input,
                status: crate::state::ToolStatus::Pending,
                ..
            }
        } if name == "read_file" && input["path"] == "src/lib.rs"
    ));
    assert!(matches!(
        transcript_block_start[1].event,
        RuntimeEvent::ToolCall {
            ref name,
            ref arguments,
            ..
        } if name == "read_file" && arguments["path"] == "src/lib.rs"
    ));

    let transcript_block_delta = normalizer
        .normalize_ui_update(
            &UiUpdate::StreamBlockDelta {
                index: 0,
                delta: "{\"path\":\"src/lib.rs\"}".to_string(),
            },
            None,
        )
        .pop()
        .expect("transcript block delta envelope");
    assert!(matches!(
        transcript_block_delta.event,
        RuntimeEvent::TranscriptBlockDelta {
            index: 0,
            ref delta,
        } if delta == "{\"path\":\"src/lib.rs\"}"
    ));

    let transcript_block_complete = normalizer
        .normalize_ui_update(&UiUpdate::StreamBlockComplete { index: 0 }, None)
        .pop()
        .expect("transcript block complete envelope");
    assert!(matches!(
        transcript_block_complete.event,
        RuntimeEvent::TranscriptBlockComplete { index: 0 }
    ));

    let (response_tx, _response_rx) = oneshot::channel();
    let approval = normalizer
        .normalize_ui_update(
            &UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "apply_patch".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            None,
        )
        .pop()
        .expect("approval envelope");

    assert!(matches!(
        approval.event,
        RuntimeEvent::ApprovalRequest {
            ref capability,
            ref scope,
            tool_name: Some(ref tool_name)
        } if capability == "apply-patch" && scope == "once" && tool_name == "apply_patch"
    ));

    let resolved = normalizer
        .normalize_runtime_request(&RuntimeRequest::ApproveCapability {
            task_id: "task-2".to_string(),
            capability: "apply-patch".to_string(),
            scope: "session".to_string(),
        })
        .pop()
        .expect("approval resolved envelope");
    assert!(matches!(
        resolved.event,
        RuntimeEvent::ApprovalResolved {
            ref capability,
            ref scope,
            approved: true
        } if capability == "apply-patch" && scope == "session"
    ));

    let result = normalizer.normalize_ui_update(
        &UiUpdate::TurnComplete,
        Some(TurnEndContext {
            usage: Some(TokenUsageEnvelope {
                input: 4,
                output: 2,
                estimated: false,
            }),
            changed_files: vec!["src/main.rs".to_string()],
        }),
    );
    assert_eq!(result.len(), 1);
    assert!(matches!(
        result[0].event,
        RuntimeEvent::TurnEnd {
            ref status,
            usage: Some(TokenUsageEnvelope { input: 4, output: 2, estimated: false }),
            ref changed_files,
        } if status == "completed" && changed_files == &vec!["src/main.rs".to_string()]
    ));
}

#[test]
fn test_pi_12_runtime_handoff_round_trips_and_batch_derivation_hold() {
    let mut normalizer = RuntimeEnvelopeNormalizer::new("batch-1741700000000");
    let mut envelopes = Vec::new();

    envelopes.push(normalizer.start_turn(1, Some("inspect src/main.rs".to_string())));
    envelopes
        .extend(normalizer.normalize_ui_update(&UiUpdate::StreamDelta("hello ".to_string()), None));
    envelopes
        .extend(normalizer.normalize_ui_update(&UiUpdate::StreamDelta("world".to_string()), None));
    envelopes.extend(normalizer.normalize_stream_block(&StreamBlock::ToolCall {
        id: "provider-1".to_string(),
        name: "git_commit".to_string(),
        input: json!({"message":"test"}),
        status: crate::state::ToolStatus::Pending,
    }));
    envelopes.extend(normalizer.normalize_stream_block(&StreamBlock::ToolResult {
        tool_call_id: "provider-1".to_string(),
        output: "done".to_string(),
        is_error: false,
    }));
    envelopes.extend(normalizer.normalize_ui_update(
        &UiUpdate::TurnComplete,
        Some(TurnEndContext {
            usage: Some(TokenUsageEnvelope {
                input: 10,
                output: 5,
                estimated: false,
            }),
            changed_files: vec![],
        }),
    ));

    envelopes.push(normalizer.start_turn(2, Some("second".to_string())));
    envelopes.push(RuntimeEnvelope {
        version: 1,
        task_id: "batch-1741700000000".to_string(),
        turn: 2,
        seq: 2,
        event: RuntimeEvent::TranscriptBlockStart {
            index: 0,
            block: StreamBlock::FinalText {
                content: String::new(),
            },
        },
    });
    envelopes.push(RuntimeEnvelope {
        version: 1,
        task_id: "batch-1741700000000".to_string(),
        turn: 2,
        seq: 3,
        event: RuntimeEvent::TranscriptBlockDelta {
            index: 0,
            delta: "fallback".to_string(),
        },
    });
    envelopes.push(RuntimeEnvelope {
        version: 1,
        task_id: "batch-1741700000000".to_string(),
        turn: 2,
        seq: 4,
        event: RuntimeEvent::TranscriptBlockComplete { index: 0 },
    });
    envelopes.push(RuntimeEnvelope {
        version: 1,
        task_id: "batch-1741700000000".to_string(),
        turn: 2,
        seq: 5,
        event: RuntimeEvent::TurnEnd {
            status: "completed".to_string(),
            usage: None,
            changed_files: vec!["src/second.rs".to_string()],
        },
    });

    for envelope in &envelopes {
        let json = serde_json::to_string(envelope).expect("serialize envelope");
        let parsed: RuntimeEnvelope = serde_json::from_str(&json).expect("parse envelope");
        assert_eq!(&parsed, envelope);
    }

    let requests = vec![
        RuntimeRequest::SubmitInput {
            task_id: None,
            input: "go".to_string(),
        },
        RuntimeRequest::Interrupt {
            task_id: "batch-1741700000000".to_string(),
        },
        RuntimeRequest::ApproveCapability {
            task_id: "batch-1741700000000".to_string(),
            capability: "apply-patch".to_string(),
            scope: "session".to_string(),
        },
        RuntimeRequest::DenyCapability {
            task_id: "batch-1741700000000".to_string(),
            capability: "run-command".to_string(),
        },
    ];
    for request in requests {
        let json = serde_json::to_string(&request).expect("serialize request");
        let parsed: RuntimeRequest = serde_json::from_str(&json).expect("parse request");
        assert_eq!(parsed, request);
    }

    let turn_start_seqs = envelopes
        .iter()
        .filter_map(|envelope| match envelope.event {
            RuntimeEvent::TurnStart { .. } => Some((envelope.turn, envelope.seq)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(turn_start_seqs, vec![(1, 1), (2, 1)]);

    let derived = derive_batch_records(&envelopes, Some("AGENTS.md".to_string()));
    assert_eq!(derived.turns.len(), 2);
    assert_eq!(derived.turns[0].response, "hello world");
    assert_eq!(derived.turns[0].changed_files, Vec::<String>::new());
    assert_eq!(derived.turns[0].command_history.len(), 1);
    assert_eq!(derived.turns[0].tokens.input, 10);
    assert_eq!(derived.turns[0].tokens.output, 5);
    assert_eq!(derived.turns[1].response, "fallback");
    assert_eq!(
        derived.turns[1].changed_files,
        vec!["src/second.rs".to_string()]
    );
    assert_eq!(
        derived.summary.expect("summary").status,
        "completed".to_string()
    );
}

#[test]
fn test_pi_12_error_and_max_turn_sequences_follow_contract() {
    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-errors");
    let _ = normalizer.start_turn(1, Some("check".to_string()));

    let recoverable = normalizer.emit_error(
        "warning".to_string(),
        "retry".to_string(),
        true,
        TurnEndContext::default(),
    );
    assert_eq!(recoverable.len(), 1);
    assert!(matches!(
        recoverable[0].event,
        RuntimeEvent::Error {
            ref code,
            ref message,
            recoverable: true
        } if code == "warning" && message == "retry"
    ));

    let result = normalizer.emit_error(
        "fatal".to_string(),
        "boom".to_string(),
        false,
        TurnEndContext::default(),
    );
    assert_eq!(result.len(), 2);
    assert!(matches!(
        result[0].event,
        RuntimeEvent::Error {
            recoverable: false,
            ..
        }
    ));
    assert!(matches!(
        result[1].event,
        RuntimeEvent::TurnEnd { ref status, .. } if status == "failed"
    ));

    let max_turns = normalizer.emit_max_turns_reached(3, TurnEndContext::default());
    assert!(matches!(
        max_turns[0].event,
        RuntimeEvent::MaxTurnsReached { max_turns: 3 }
    ));
    assert!(matches!(
        max_turns[1].event,
        RuntimeEvent::TurnEnd { ref status, .. } if status == "failed"
    ));
}

fn assert_runtime_tool_id(id: &str) {
    let parts: Vec<_> = id.split('_').collect();
    assert_eq!(parts.len(), 3, "runtime tool id must have three segments");
    assert_eq!(parts[0], "call");
    assert!(parts[1].chars().all(|ch| ch.is_ascii_digit()));
    assert_eq!(parts[2].len(), 4);
    assert!(parts[2].chars().all(|ch| ch.is_ascii_hexdigit()));
}
