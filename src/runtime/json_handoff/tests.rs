use super::*;
use crate::api::stream::provider::{ProviderDelta, ProviderStreamEvent};
use crate::runtime::delta_accumulator::DeltaAccumulator;
use crate::state::ToolApprovalRequest;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::oneshot;

#[test]
fn test_pi_09_anchor_runtime_envelope_serde_shape() {
    let envelope = RuntimeEnvelope {
        version: 1,
        task_id: "task-1741700000000".to_string(),
        turn: 1,
        seq: 3,
        event_id: "evt:task-1741700000000:1:3".to_string(),
        emitted_at: "2026-04-16T00:00:00.000Z".to_string(),
        source: RuntimeEnvelopeSource::Model,
        request_id: Some("req-1".to_string()),
        parent_event_id: None,
        event: RuntimeEvent::ToolCallStarted {
            tool_call_id: "tx_1_9a2f".to_string(),
            tool_name: "read_file".to_string(),
            arguments: json!({
                "path": "src/app.rs"
            }),
            status: crate::state::ToolStatus::Pending,
            started_at: "2026-04-16T00:00:00.000Z".to_string(),
        },
    };

    let value = serde_json::to_value(&envelope).expect("runtime envelope must serialize");
    assert_eq!(value["version"], 1);
    assert_eq!(value["task_id"], "task-1741700000000");
    assert_eq!(value["turn"], 1);
    assert_eq!(value["seq"], 3);
    assert_eq!(value["event_id"], "evt:task-1741700000000:1:3");
    assert_eq!(value["emitted_at"], "2026-04-16T00:00:00.000Z");
    assert_eq!(value["source"], "model");
    assert_eq!(value["request_id"], "req-1");
    assert_eq!(value["event"]["type"], "tool_call_started");
    assert_eq!(value["event"]["tool_call_id"], "tx_1_9a2f");
    assert_eq!(value["event"]["tool_name"], "read_file");
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
        "https://vexcoder.com/schemas/runtime_envelope_v1.json"
    );
    assert_eq!(
        request_schema["$id"],
        "https://vexcoder.com/schemas/runtime_request_v1.json"
    );
    assert_eq!(envelope_schema["properties"]["version"]["const"], 1);
    assert_eq!(
        envelope_schema["properties"]["source"]["$ref"],
        "#/$defs/envelope_source"
    );
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
        RuntimeEvent::ToolCallStarted {
            tool_call_id,
            tool_name,
            arguments,
            status,
            ..
        } => {
            assert_ne!(tool_call_id, "provider-call-1");
            assert_runtime_tool_id(tool_call_id);
            assert_eq!(tool_name, "write_file");
            assert_eq!(arguments["path"], "src/main.rs");
            assert_eq!(status, &crate::state::ToolStatus::Pending);
            tool_call_id.clone()
        }
        other => panic!("expected tool_call_started event, got {other:?}"),
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
        RuntimeEvent::ToolCallCompleted {
            tool_call_id,
            tool_name,
            status,
            output,
            ..
        } => {
            assert_eq!(tool_call_id, runtime_call_id);
            assert_eq!(tool_name.as_deref(), Some("write_file"));
            assert_eq!(status, crate::state::ToolStatus::Complete);
            assert_eq!(output, "ok");
        }
        other => panic!("expected tool_call_completed event, got {other:?}"),
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
    assert_eq!(delta[0].source, RuntimeEnvelopeSource::Model);
    assert_eq!(delta[1].source, RuntimeEnvelopeSource::Model);

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
    assert_eq!(transcript_line[0].source, RuntimeEnvelopeSource::Model);
    assert_eq!(transcript_line[1].source, RuntimeEnvelopeSource::Runtime);

    let transcript_block_start = normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "provider-call-1".to_string(),
                name: "read_file".to_string(),
                input: json!({}),
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
        } if name == "read_file" && input == &json!({})
    ));
    assert!(matches!(
        transcript_block_start[1].event,
        RuntimeEvent::ToolCallStarted {
            ref tool_name,
            ref arguments,
            ..
        } if tool_name == "read_file" && arguments == &json!({})
    ));
    assert_eq!(
        transcript_block_start[0].source,
        RuntimeEnvelopeSource::Model
    );
    assert_eq!(
        transcript_block_start[1].source,
        RuntimeEnvelopeSource::Model
    );

    // TranscriptBlockDelta is emitted first (accepted protocol event),
    // followed by ToolCallArgumentsDelta for tool blocks.
    let block_delta_envelopes = normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockDelta {
            index: 0,
            delta: "{\"path\":\"src/lib.rs\"}".to_string(),
        },
        None,
    );
    assert_eq!(block_delta_envelopes.len(), 2);
    let transcript_block_delta = &block_delta_envelopes[0];
    let tool_call_arguments_delta = &block_delta_envelopes[1];
    assert!(matches!(
        transcript_block_delta.event,
        RuntimeEvent::TranscriptBlockDelta {
            index: 0,
            ref delta,
        } if delta == "{\"path\":\"src/lib.rs\"}"
    ));
    assert_eq!(transcript_block_delta.source, RuntimeEnvelopeSource::Model);
    assert!(matches!(
        tool_call_arguments_delta.event,
        RuntimeEvent::ToolCallArgumentsDelta {
            ref tool_name,
            ref delta,
            arguments: Some(ref arguments),
            ..
        } if tool_name.as_deref() == Some("read_file")
            && delta == "{\"path\":\"src/lib.rs\"}"
            && arguments["path"] == "src/lib.rs"
    ));

    let transcript_block_complete = normalizer
        .normalize_ui_update(&UiUpdate::StreamBlockComplete { index: 0 }, None)
        .pop()
        .expect("transcript block complete envelope");
    assert!(matches!(
        transcript_block_complete.event,
        RuntimeEvent::TranscriptBlockComplete { index: 0 }
    ));
    assert_eq!(
        transcript_block_complete.source,
        RuntimeEnvelopeSource::Model
    );

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
            request_id: "req-approve-1".to_string(),
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
                cache_creation_input: 0,
                cache_read_input: 0,
            }),
            changed_files: vec!["src/main.rs".to_string()],
        }),
    );
    assert_eq!(result.len(), 1);
    assert!(matches!(
        result[0].event,
        RuntimeEvent::TurnEnd {
            ref status,
            usage: Some(TokenUsageEnvelope {
                input: 4,
                output: 2,
                estimated: false,
                ..
            }),
            ref changed_files,
        } if status == "completed" && changed_files == &vec!["src/main.rs".to_string()]
    ));
}

#[test]
fn test_pi_10_stream_block_deltas_feed_delta_accumulator() {
    let accumulator = Arc::new(DeltaAccumulator::new(8 * 1_024));
    let mut normalizer = RuntimeEnvelopeNormalizer::new_with_delta_accumulator(
        "task-delta",
        Arc::clone(&accumulator),
    );
    let _ = normalizer.start_turn(1, Some("stream tool".to_string()));

    let start = normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "provider-call-1".to_string(),
                name: "read_file".to_string(),
                input: json!({}),
                status: crate::state::ToolStatus::Pending,
            },
        },
        None,
    );
    let runtime_call_id = match &start[1].event {
        RuntimeEvent::ToolCallStarted { tool_call_id, .. } => tool_call_id.clone(),
        other => panic!("expected tool_call_started event, got {other:?}"),
    };

    let first_delta = normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockDelta {
            index: 0,
            delta: r#"{"path":"src/"#.to_string(),
        },
        None,
    );
    let second_delta = normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockDelta {
            index: 0,
            delta: r#"lib.rs"}"#.to_string(),
        },
        None,
    );
    normalizer.normalize_ui_update(&UiUpdate::StreamBlockComplete { index: 0 }, None);

    let snapshot = accumulator.snapshot();
    let map = snapshot.lock().unwrap_or_else(|e| e.into_inner());
    let tool_state = map.get(&runtime_call_id).expect("tool state present");
    assert!(matches!(
        first_delta[1].event,
        RuntimeEvent::ToolCallArgumentsDelta {
            arguments: None,
            ref delta,
            ..
        } if delta == r#"{"path":"src/"#
    ));
    assert!(matches!(
        second_delta[1].event,
        RuntimeEvent::ToolCallArgumentsDelta {
            arguments: Some(ref arguments),
            ref delta,
            ..
        } if delta == r#"lib.rs"}"# && arguments["path"] == "src/lib.rs"
    ));
    assert_eq!(tool_state.partial_args, r#"{"path":"src/lib.rs"}"#);
    assert_eq!(
        tool_state.delta_queue.iter().cloned().collect::<Vec<_>>(),
        vec![r#"{"path":"src/"#.to_string(), r#"lib.rs"}"#.to_string()]
    );
    assert!(tool_state.finished);
}

#[test]
fn test_pi_10_runtime_origin_block_sources_are_preserved() {
    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-source");
    let _ = normalizer.start_turn(1, Some("runtime block".to_string()));

    let start = normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockStart {
            index: 7,
            block: StreamBlock::ToolResult {
                tool_call_id: "provider-call-1".to_string(),
                output: "done".to_string(),
                is_error: false,
            },
        },
        None,
    );

    assert_eq!(start[0].source, RuntimeEnvelopeSource::Runtime);
    assert!(matches!(
        start[0].event,
        RuntimeEvent::TranscriptBlockStart {
            index: 7,
            block: StreamBlock::ToolResult { .. }
        }
    ));
    assert_eq!(start[1].source, RuntimeEnvelopeSource::Runtime);
    assert!(matches!(
        start[1].event,
        RuntimeEvent::ToolCallCompleted {
            ref tool_call_id,
            tool_name: None,
            status: crate::state::ToolStatus::Complete,
            ..
        } if tool_call_id == "provider-call-1"
    ));

    let complete = normalizer
        .normalize_ui_update(&UiUpdate::StreamBlockComplete { index: 7 }, None)
        .pop()
        .expect("tool-result block complete envelope");
    assert_eq!(complete.source, RuntimeEnvelopeSource::Runtime);
    assert!(matches!(
        complete.event,
        RuntimeEvent::TranscriptBlockComplete { index: 7 }
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
                cache_creation_input: 0,
                cache_read_input: 0,
            }),
            changed_files: vec![],
        }),
    ));

    envelopes.push(normalizer.start_turn(2, Some("second".to_string())));
    envelopes.push(normalizer.emit_event(RuntimeEvent::TranscriptBlockStart {
        index: 0,
        block: StreamBlock::FinalText {
            content: String::new(),
        },
    }));
    envelopes.push(normalizer.emit_event(RuntimeEvent::TranscriptBlockDelta {
        index: 0,
        delta: "fallback".to_string(),
    }));
    envelopes.push(normalizer.emit_event(RuntimeEvent::TranscriptBlockComplete { index: 0 }));
    envelopes.push(normalizer.emit_event(RuntimeEvent::TurnEnd {
        status: "completed".to_string(),
        usage: None,
        changed_files: vec!["src/second.rs".to_string()],
    }));

    for envelope in &envelopes {
        let json = serde_json::to_string(envelope).expect("serialize envelope");
        let parsed: RuntimeEnvelope = serde_json::from_str(&json).expect("parse envelope");
        assert_eq!(&parsed, envelope);
    }

    let requests = vec![
        RuntimeRequest::SubmitInput {
            request_id: "req-submit-1".to_string(),
            task_id: None,
            input: "go".to_string(),
        },
        RuntimeRequest::Interrupt {
            request_id: "req-interrupt-1".to_string(),
            task_id: "batch-1741700000000".to_string(),
        },
        RuntimeRequest::ApproveCapability {
            request_id: "req-approve-2".to_string(),
            task_id: "batch-1741700000000".to_string(),
            capability: "apply-patch".to_string(),
            scope: "session".to_string(),
        },
        RuntimeRequest::DenyCapability {
            request_id: "req-deny-1".to_string(),
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
    // seq is process-lifetime monotonic: turn 1 starts at seq 1; turn 2
    // continues without a reset so its TurnStart seq > 1.
    assert_eq!(turn_start_seqs.len(), 2);
    assert_eq!(turn_start_seqs[0], (1, 1));
    assert_eq!(turn_start_seqs[1].0, 2);
    assert!(
        turn_start_seqs[1].1 > turn_start_seqs[0].1,
        "turn 2 TurnStart seq must be greater than turn 1 TurnStart seq (global monotonic counter)"
    );

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

#[test]
fn test_pi_12_finish_protocol_ingress_turn_closes_blocks_in_index_order() {
    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-order");

    normalizer.normalize_provider_stream_event(ProviderStreamEvent::ContentBlockDelta {
        index: 3,
        delta: ProviderDelta {
            _delta_type: None,
            text: Some("later".to_string()),
            partial_json: None,
            thinking: None,
            _signature: None,
            _choice_index: None,
        },
    });
    normalizer.normalize_provider_stream_event(ProviderStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ProviderDelta {
            _delta_type: None,
            text: None,
            partial_json: None,
            thinking: Some("earlier".to_string()),
            _signature: None,
            _choice_index: None,
        },
    });

    let finish = normalizer.finish_protocol_ingress_turn();
    let closed_indices = finish
        .iter()
        .filter_map(|envelope| match envelope.event {
            RuntimeEvent::TranscriptBlockComplete { index } => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(closed_indices, vec![1, 3]);
    assert!(matches!(
        finish.last().map(|envelope| &envelope.event),
        Some(RuntimeEvent::TurnEnd { status, .. }) if status == "completed"
    ));
}

fn assert_runtime_tool_id(id: &str) {
    let parts: Vec<_> = id.split('_').collect();
    assert_eq!(parts.len(), 3, "runtime tool id must have three segments");
    assert_eq!(parts[0], "tx");
    assert!(parts[1].chars().all(|ch| ch.is_ascii_digit()));
    assert_eq!(parts[2].len(), 4);
    assert!(parts[2].chars().all(|ch| ch.is_ascii_hexdigit()));
}
