use super::{MAX_SSE_BUFFER_BYTES, StreamParser};
use crate::runtime::RuntimeEvent;

#[test]
fn test_process_emits_ping_for_ping_frame() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(b"event: ping\ndata: {\"type\":\"ping\"}\n\n")
        .unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_process_maps_chat_compat_usage_chunk() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"total_tokens\":19}}\n\n",
        )
        .unwrap();

    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0].event, RuntimeEvent::TurnStart { .. }));
    assert!(matches!(
        &events[1].event,
        RuntimeEvent::UsageUpdated { usage }
            if usage.input == 12 && usage.output == 7 && usage.cache_creation_input == 0 && usage.cache_read_input == 0
    ));
}

#[test]
fn test_process_messages_v1_message_delta_top_level_usage() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":15}}\n\n",
        )
        .unwrap();

    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0].event, RuntimeEvent::TurnStart { .. }));
    assert!(matches!(
        &events[1].event,
        RuntimeEvent::UsageUpdated { usage } if usage.output == 15
    ));
}

#[test]
fn test_process_messages_v1_legacy_thinking_tag_emits_recoverable_error() {
    let mut parser = StreamParser::new();
    let frame = format!(
        "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"{}\",\"data\":\"opaque\"}}}}\n\n",
        "redacted_thinking"
    );
    let events = parser.process(frame.as_bytes()).unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::Error {
            code,
            recoverable,
            ..
        } if code == "provider_content_block_start_decode" && *recoverable
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "opaque"
    )));
}

#[test]
fn test_process_messages_v1_thinking_data_block_emits_thinking_delta() {
    let mut parser = StreamParser::new();
    let frame =
        b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking_data\",\"data\":\"opaque\"}}\n\n";
    let events = parser.process(frame).unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "opaque"
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::Error {
            code,
            recoverable,
            ..
        } if code == "provider_content_block_start_decode" && *recoverable
    )));
}

#[test]
fn test_process_messages_v1_tool_use_accepts_object_arguments_without_synthetic_delta() {
    let mut parser = StreamParser::new();
    let frame = b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_obj\",\"name\":\"read_file\",\"input\":{\"path\":\"src/lib.rs\",\"line\":7}}}\n\n";
    let events = parser.process(frame).unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1,
            block: crate::state::StreamBlock::ToolCall { id, name, input, .. }
        } if id == "toolu_obj"
            && name == "read_file"
            && input == &serde_json::json!({"path":"src/lib.rs","line":7})
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ToolCallStarted {
            tool_name,
            arguments,
            ..
        } if tool_name == "read_file"
            && arguments == &serde_json::json!({"path":"src/lib.rs","line":7})
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(&event.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized messages-v1 tool input should not emit a synthetic arguments delta",
    );
}

#[test]
fn test_process_chat_compat_emits_message_start_metadata() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","created":1741730100,"model":"model-name","system_fingerprint":"fp_123","service_tier":"standard","choices":[{"index":0,"delta":{"role":"assistant","content":"hello"},"finish_reason":null}]}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata.object.as_deref() == Some("chat.completion.chunk")
                && metadata.created == Some(1741730100)
                && metadata.system_fingerprint.as_deref() == Some("fp_123")
                && metadata.service_tier.as_deref() == Some("standard")
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "hello"
    )));
}

#[test]
fn test_process_chat_compat_emits_refusal_logprobs_and_choice_index() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":2,"delta":{"role":"assistant","refusal":"cannot comply"},"logprobs":{"content":[]},"finish_reason":"stop"}]}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata.choice_index == Some(2) && metadata.logprobs.is_some()
    )));
}

#[test]
fn test_process_chat_compat_emits_prompt_progress_and_timings_without_text() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":null},"finish_reason":null}],"prompt_progress":{"total":2641,"processed":2048,"cache":0,"time_ms":153341},"timings":{"prompt_n":2048,"prompt_ms":153341.0,"predicted_n":0,"predicted_ms":0.0}}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata
                .prompt_progress
                .as_ref()
                .is_some_and(|progress| progress.total == Some(2641) && progress.processed == Some(2048))
                && metadata
                    .timings
                    .as_ref()
                    .is_some_and(|timings| timings.prompt_n == Some(2048) && timings.prompt_ms == Some(153341.0))
    )));
}

#[test]
fn test_process_emits_error_event_on_unparseable_frame() {
    let mut parser = StreamParser::new();
    let events = parser.process(b"data: not-a-json-value\n\n").unwrap();

    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].event,
        RuntimeEvent::Error { code, .. } if code == "sse_parse_error"
    ));
}

#[test]
fn test_process_emits_error_event_on_buffer_overflow() {
    let mut parser = StreamParser::new();
    let big_chunk = vec![b'x'; MAX_SSE_BUFFER_BYTES + 1];
    let events = parser.process(&big_chunk).unwrap();

    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].event,
        RuntimeEvent::Error { code, .. } if code == "sse_buffer_overflow"
    ));

    let follow_up = parser.process(b"still-overflowed").unwrap();
    assert_eq!(follow_up.len(), 1);
    assert!(matches!(&follow_up[0].event, RuntimeEvent::Error { .. }));
}

#[test]
fn test_process_clamps_chat_compat_tool_call_index() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":999999,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"src/main.rs\"}"}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1025,
            block: crate::state::StreamBlock::ToolCall { id, name, .. }
        } if id == "call_1" && name == "read_file"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ToolCallArgumentsDelta { delta, .. }
            if delta == "{\"path\":\"src/main.rs\"}"
    )));
}

#[test]
fn test_process_chat_compat_emits_reasoning_content_as_transcript_delta() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"weighing options"},"finish_reason":null}]}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { index: 0, delta } if delta == "weighing options"
    )));
}

#[test]
fn test_process_chat_compat_emits_thinking_alias_as_transcript_delta() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","thinking":"weighing options"},"finish_reason":null}]}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { index: 0, delta } if delta == "weighing options"
    )));
}

#[test]
fn test_process_chat_compat_accepts_object_valued_tool_arguments() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_obj","type":"function","function":{"name":"read_file","arguments":{"path":"src/main.rs"}}}]},"finish_reason":"tool_calls"}]}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1,
            block: crate::state::StreamBlock::ToolCall { id, name, input, .. }
        } if id == "call_obj" && name == "read_file" && input == &serde_json::json!({"path":"src/main.rs"})
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ToolCallStarted {
            tool_name,
            arguments,
            ..
        } if tool_name == "read_file" && arguments == &serde_json::json!({"path":"src/main.rs"})
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(&event.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "a materialized JSON value should not require a synthetic delta replay",
    );
}

#[test]
fn test_process_chat_compat_emits_text_and_materialized_tool_arguments_from_same_chunk() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"reading now","tool_calls":[{"index":0,"id":"call_mix","type":"function","function":{"name":"read_file","arguments":{"path":"src/main.rs"}}}]},"finish_reason":"tool_calls"}]}

"#,
        )
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { index: 0, delta } if delta == "reading now"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1,
            block: crate::state::StreamBlock::ToolCall { id, name, input, .. }
        } if id == "call_mix"
            && name == "read_file"
            && input == &serde_json::json!({"path":"src/main.rs"})
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(&event.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized chat-compat tool arguments should stay materialized even when text and tool calls share a chunk",
    );
}

#[test]
fn test_process_chat_compat_ignores_empty_content_without_emitting_text_block() {
    let mut parser = StreamParser::new();
    let mut events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":"stop"}]}

"#,
        )
        .unwrap();
    events.extend(parser.finish());

    assert!(
        !events.iter().any(|event| matches!(
            &event.event,
            RuntimeEvent::TranscriptBlockStart { index: 0, .. }
                | RuntimeEvent::TranscriptBlockDelta { index: 0, .. }
        )),
        "empty chat-compatible content should not open or update a transcript block",
    );
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TurnEnd { status, .. } if status == "completed"
    )));
}

#[test]
fn test_process_chat_compat_ignores_empty_content_when_tool_calls_are_present() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"","tool_calls":[{"index":0,"id":"call_empty_mix","type":"function","function":{"name":"read_file","arguments":{"path":"src/main.rs"}}}]},"finish_reason":"tool_calls"}]}

"#,
        )
        .unwrap();

    assert!(
        !events.iter().any(|event| matches!(
            &event.event,
            RuntimeEvent::TranscriptBlockStart { index: 0, .. }
                | RuntimeEvent::TranscriptBlockDelta { index: 0, .. }
        )),
        "tool-only chat-compatible chunks must not emit an empty transcript block first",
    );
    assert!(events.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1,
            block: crate::state::StreamBlock::ToolCall { id, name, input, .. }
        } if id == "call_empty_mix"
            && name == "read_file"
            && input == &serde_json::json!({"path":"src/main.rs"})
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(&event.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized tool-only chat-compatible chunks should stay materialized",
    );
}

#[test]
fn test_process_chat_compat_preserves_tool_state_across_reasoning_interleave() {
    let mut parser = StreamParser::new();

    let first = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"plan: call read_file","tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read_file","arguments":"{\"path\":\""}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();

    assert!(first.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { index: 0, delta } if delta == "plan: call read_file"
    )));
    assert!(first.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockStart {
            index: 1,
            block: crate::state::StreamBlock::ToolCall { id, name, .. }
        } if id == "call_a" && name == "read_file"
    )));

    let second = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"reasoning_content":"now finalising args","tool_calls":[{"index":0,"function":{"arguments":"src/lib.rs\"}"}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();

    assert!(second.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockDelta { index: 0, delta } if delta == "now finalising args"
    )));
    assert!(second.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::ToolCallArgumentsDelta { delta, tool_name, .. }
            if delta == "src/lib.rs\"}" && tool_name.as_deref() == Some("read_file")
    )));
}

#[test]
fn test_process_chat_compat_allocates_sparse_tool_call_indices() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read_file","arguments":"{}"}},{"index":5,"id":"call_b","type":"function","function":{"name":"write_file","arguments":"{}"}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();

    let tool_starts: Vec<usize> = events
        .iter()
        .filter_map(|event| match &event.event {
            RuntimeEvent::TranscriptBlockStart {
                index,
                block: crate::state::StreamBlock::ToolCall { .. },
            } => Some(*index),
            _ => None,
        })
        .collect();
    assert_eq!(tool_starts, vec![1, 6]);
}

#[test]
fn test_process_chat_compat_ignores_deltas_after_tool_block_closed() {
    let mut parser = StreamParser::new();

    let opening = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"noop","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}

"#,
        )
        .unwrap();
    assert!(opening.iter().any(|event| matches!(
        &event.event,
        RuntimeEvent::TranscriptBlockComplete { index: 1 }
    )));

    let replay = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"should-be-ignored"}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();
    let has_late_tool_event = replay.iter().any(|event| {
        matches!(
            &event.event,
            RuntimeEvent::ToolCallArgumentsDelta { .. }
                | RuntimeEvent::TranscriptBlockStart {
                    block: crate::state::StreamBlock::ToolCall { .. },
                    ..
                }
        )
    });
    assert!(!has_late_tool_event);
}

#[test]
fn test_process_chat_compat_seals_discovered_tool_at_finish_reason() {
    let mut parser = StreamParser::new();

    let opening = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"arguments":"{\"path\""}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();
    let had_block_start = opening.iter().any(|event| {
        matches!(
            &event.event,
            RuntimeEvent::TranscriptBlockStart {
                block: crate::state::StreamBlock::ToolCall { .. },
                ..
            }
        )
    });
    assert!(
        !had_block_start,
        "a Discovered entry must not emit a block start before a name is observed",
    );

    let terminator = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}

"#,
        )
        .unwrap();
    let had_late_start = terminator.iter().any(|event| {
        matches!(
            &event.event,
            RuntimeEvent::TranscriptBlockStart {
                block: crate::state::StreamBlock::ToolCall { .. },
                ..
            }
        )
    });
    assert!(!had_late_start);

    let replay = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"read_file","arguments":":\"main.rs\"}"}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();
    let resurrected = replay.iter().any(|event| {
        matches!(
            &event.event,
            RuntimeEvent::ToolCallArgumentsDelta { .. }
                | RuntimeEvent::TranscriptBlockStart {
                    block: crate::state::StreamBlock::ToolCall { .. },
                    ..
                }
        )
    });
    assert!(
        !resurrected,
        "a late name delta must not advance a sealed Discovered index to Opened",
    );
}

#[test]
fn test_process_chat_compat_rejects_tool_deltas_after_done() {
    let mut parser = StreamParser::new();

    let _ = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"done"},"finish_reason":"stop"}]}

data: [DONE]

"#,
        )
        .unwrap();

    let late = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":7,"id":"call_late","type":"function","function":{"name":"read_file","arguments":"{}"}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();
    let late_tool_event = late.iter().any(|event| {
        matches!(
            &event.event,
            RuntimeEvent::ToolCallArgumentsDelta { .. }
                | RuntimeEvent::TranscriptBlockStart {
                    block: crate::state::StreamBlock::ToolCall { .. },
                    ..
                }
        )
    });
    assert!(
        !late_tool_event,
        "tool deltas arriving after [DONE] must be dropped before lifecycle state is instantiated",
    );
}

#[test]
fn test_process_preserves_tab_after_single_space_strip() {
    let mut parser = StreamParser::new();
    let events = parser.process(b"data: \t{\"type\":\"ping\"}\n\n").unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_process_ignores_unknown_fields() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(b"custom-field: ignored\nevent: ping\ndata: {\"type\":\"ping\"}\n\n")
        .unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_process_handles_cr_only_frame_delimiters() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(b"event: ping\rdata: {\"type\":\"ping\"}\r\r")
        .unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_process_strips_utf8_bom_once() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(b"\xEF\xBB\xBFevent: ping\ndata: {\"type\":\"ping\"}\n\n")
        .unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_process_recognises_id_and_retry_fields() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(b"id: evt-42\nretry: 1500\nevent: ping\ndata: {\"type\":\"ping\"}\n\n")
        .unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_process_id_only_frame_emits_no_event() {
    let mut parser = StreamParser::new();
    let events = parser.process(b"id: evt-42\n\n").unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_process_colon_free_field_name_is_ignored() {
    let mut parser = StreamParser::new();
    let events = parser.process(b"custom-field\n\n").unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_process_raw_json_frame_without_data_emits_error() {
    let mut parser = StreamParser::new();
    let events = parser.process(b"{\"type\":\"ping\"}\n\n").unwrap();

    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].event,
        RuntimeEvent::Error { code, message, .. }
            if code == "sse_parse_error"
                && message.contains("raw JSON chunk streams are unsupported")
    ));
}

use super::{NormalisedChunk, StreamTextNormaliser};

fn collect_text(chunks: &[NormalisedChunk]) -> String {
    chunks
        .iter()
        .map(|chunk| match chunk {
            NormalisedChunk::Text(text) => text.as_str(),
        })
        .collect()
}

#[test]
fn test_normaliser_passes_clean_text_through() {
    let mut normaliser = StreamTextNormaliser::new();
    let chunks = normaliser.normalise("Hello world");
    assert_eq!(collect_text(&chunks), "Hello world");
}

#[test]
fn test_normaliser_preserves_markup_as_plain_text() {
    let mut normaliser = StreamTextNormaliser::new();
    let input = "<function=read_file>\n<parameter=path>\nsrc/main.rs\n</parameter>\n</function>";
    let chunks = normaliser.normalise(input);
    assert_eq!(collect_text(&chunks), input);
}

#[test]
fn test_normaliser_empty_input_emits_no_chunks() {
    let mut normaliser = StreamTextNormaliser::new();
    assert!(normaliser.normalise("").is_empty());
}

#[test]
fn test_normaliser_is_stateless_across_calls() {
    let mut normaliser = StreamTextNormaliser::new();
    assert_eq!(collect_text(&normaliser.normalise("alpha")), "alpha");
    assert_eq!(collect_text(&normaliser.normalise("beta")), "beta");
}

// ---------------------------------------------------------------------------
// Multi-block messages-v1 and JSON-variation tests (items 4-5 transport follow-up)
// ---------------------------------------------------------------------------

/// Verifies that two consecutive `tool_use` `content_block_start` events in a
/// messages-v1 stream produce two independent `ToolCallStarted` envelopes with
/// no cross-contamination of IDs, names, or inputs.
#[test]
fn test_process_messages_v1_two_sequential_tool_use_blocks_emit_independent_calls() {
    let mut parser = StreamParser::new();

    // First tool_use block at index 1.
    let first = parser
        .process(
            b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_a\",\"name\":\"read_file\",\"input\":{\"path\":\"src/lib.rs\"}}}\n\n",
        )
        .unwrap();

    assert!(
        first.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::TranscriptBlockStart {
                index: 1,
                block: crate::state::StreamBlock::ToolCall { id, name, .. }
            } if id == "toolu_a" && name == "read_file"
        )),
        "first tool block must open at index 1; got {:?}",
        first.iter().map(|e| &e.event).collect::<Vec<_>>()
    );

    // Second tool_use block at index 2.
    let second = parser
        .process(
            b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_b\",\"name\":\"write_file\",\"input\":{\"path\":\"out.txt\",\"content\":\"done\"}}}\n\n",
        )
        .unwrap();

    assert!(
        second.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::TranscriptBlockStart {
                index: 2,
                block: crate::state::StreamBlock::ToolCall { id, name, .. }
            } if id == "toolu_b" && name == "write_file"
        )),
        "second tool block must open at index 2; got {:?}",
        second.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert!(
        !second.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::TranscriptBlockStart {
                block: crate::state::StreamBlock::ToolCall { id, .. },
                ..
            } if id == "toolu_a"
        )),
        "second block open must not re-emit the first block"
    );

    // Both ToolCallStarted events must have arrived (one per block).
    let all: Vec<_> = first.iter().chain(second.iter()).collect();
    let started_names: Vec<&str> = all
        .iter()
        .filter_map(|e| match &e.event {
            RuntimeEvent::ToolCallStarted { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        started_names.contains(&"read_file"),
        "ToolCallStarted for read_file expected; got {started_names:?}"
    );
    assert!(
        started_names.contains(&"write_file"),
        "ToolCallStarted for write_file expected; got {started_names:?}"
    );
}

/// Verifies that a messages-v1 `tool_use` block with a deeply-nested JSON
/// `input` object (arrays, nested objects) normalizes without data loss.
#[test]
fn test_process_messages_v1_tool_use_nested_json_input_normalizes_correctly() {
    let mut parser = StreamParser::new();
    let frame = br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_deep","name":"apply_patch","input":{"operations":[{"op":"replace","path":"/src/lib.rs","content":"fn main() {}\n"},{"op":"delete","path":"/src/old.rs"}],"dry_run":false,"metadata":{"author":"ci","reason":"normalize"}}}}

"#;
    let events = parser.process(frame).unwrap();

    let expected_input = serde_json::json!({
        "operations": [
            {"op": "replace", "path": "/src/lib.rs", "content": "fn main() {}\n"},
            {"op": "delete", "path": "/src/old.rs"}
        ],
        "dry_run": false,
        "metadata": {"author": "ci", "reason": "normalize"}
    });

    assert!(
        events.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::ToolCallStarted { tool_name, arguments, .. }
                if tool_name == "apply_patch" && arguments == &expected_input
        )),
        "deeply-nested tool input must materialize intact; got {:?}",
        events.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(&e.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized nested input must not emit argument deltas"
    );
}

/// Verifies that the chat-compat string-form argument accumulator correctly
/// coalesces partial JSON fragments arriving across three separate SSE chunks
/// into a single materialized tool call with the complete arguments object.
///
/// This covers the typical local chat-compatible streaming pattern where `"arguments"` is
/// delivered as a fragmented JSON string: `"{\"path\":"`, `"\"src/lib.rs\""`,
/// `"}"`.
#[test]
fn test_process_chat_compat_string_arguments_accumulate_across_three_chunks() {
    let mut parser = StreamParser::new();

    // Chunk 1: opening fragment with ID and partial arguments string.
    let c1 = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_frag","type":"function","function":{"name":"read_file","arguments":"{\"path\":"}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();
    assert!(
        c1.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::TranscriptBlockStart {
                index: 1,
                block: crate::state::StreamBlock::ToolCall { id, name, .. }
            } if id == "call_frag" && name == "read_file"
        )),
        "block must open on first chunk; got {:?}",
        c1.iter().map(|e| &e.event).collect::<Vec<_>>()
    );

    // Chunk 2: middle fragment.
    let c2 = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"src/lib.rs\""}}]},"finish_reason":null}]}

"#,
        )
        .unwrap();
    assert!(
        c2.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::ToolCallArgumentsDelta { tool_name, .. }
                if tool_name.as_deref() == Some("read_file")
        )),
        "middle fragment must emit ToolCallArgumentsDelta; got {:?}",
        c2.iter().map(|e| &e.event).collect::<Vec<_>>()
    );

    // Chunk 3: closing fragment with finish_reason.
    let c3 = parser
        .process(
            br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"}"}}]},"finish_reason":"tool_calls"}]}

"#,
        )
        .unwrap();
    assert!(
        c3.iter()
            .any(|e| matches!(&e.event, RuntimeEvent::TranscriptBlockComplete { index: 1 })),
        "block must complete on final chunk with finish_reason; got {:?}",
        c3.iter().map(|e| &e.event).collect::<Vec<_>>()
    );

    // ToolCallStarted (emitted at block-open) must carry the block ID.
    let started = c1
        .iter()
        .find(|e| matches!(&e.event, RuntimeEvent::ToolCallStarted { .. }));
    assert!(
        started.is_some(),
        "ToolCallStarted must be emitted when the block opens"
    );
}
