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
