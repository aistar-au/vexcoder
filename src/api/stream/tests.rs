use super::{StreamParser, MAX_SSE_BUFFER_BYTES};
use crate::types::StreamEvent;

#[test]
fn test_process_emits_ping_for_ping_frame() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(b"event: ping\ndata: {\"type\":\"ping\"}\n\n")
        .unwrap();

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], StreamEvent::Ping));
}

#[test]
fn test_process_maps_chat_compat_usage_chunk() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"total_tokens\":19}}\n\n",
        )
        .unwrap();

    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::MessageDelta { usage, .. } => {
            let usage = usage.as_ref().expect("usage should be present");
            assert_eq!(usage.input_tokens, Some(12));
            assert_eq!(usage.output_tokens, Some(7));
            assert_eq!(usage.total_tokens, Some(19));
        }
        other => panic!("expected MessageDelta event, got {other:?}"),
    }
}

#[test]
fn test_process_messages_v1_message_delta_top_level_usage() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":15}}\n\n",
        )
        .unwrap();

    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::MessageDelta { delta, usage } => {
            assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
            let usage = usage.as_ref().expect("top-level usage must be present");
            assert_eq!(usage.output_tokens, Some(15));
        }
        other => panic!("expected MessageDelta event, got {other:?}"),
    }
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

    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::MessageStart { message } => {
            assert_eq!(message.id, "chatcmpl-1");
            assert_eq!(message.role, "assistant");
            assert_eq!(message.model, "model-name");
            let metadata = message
                .metadata
                .as_ref()
                .expect("metadata should be present");
            assert_eq!(metadata.object.as_deref(), Some("chat.completion.chunk"));
            assert_eq!(metadata.created, Some(1741730100));
            assert_eq!(metadata.system_fingerprint.as_deref(), Some("fp_123"));
            assert_eq!(metadata.service_tier.as_deref(), Some("standard"));
        }
        other => panic!("expected MessageStart event, got {other:?}"),
    }
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

    assert_eq!(events.len(), 2);
    match &events[1] {
        StreamEvent::MessageDelta { delta, usage } => {
            assert!(usage.is_none());
            assert_eq!(delta.stop_reason.as_deref(), Some("stop"));
            assert_eq!(delta.role.as_deref(), Some("assistant"));
            assert_eq!(delta.refusal.as_deref(), Some("cannot comply"));
            let metadata = delta.metadata.as_ref().expect("metadata should be present");
            assert_eq!(metadata.choice_index, Some(2));
            assert!(metadata.logprobs.is_some());
        }
        other => panic!("expected MessageDelta event, got {other:?}"),
    }
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

    assert_eq!(events.len(), 2);
    match &events[1] {
        StreamEvent::MessageDelta { delta, usage } => {
            assert!(usage.is_none());
            let metadata = delta.metadata.as_ref().expect("metadata should be present");
            let prompt_progress = metadata
                .prompt_progress
                .as_ref()
                .expect("prompt progress should be present");
            assert_eq!(prompt_progress.total, Some(2641));
            assert_eq!(prompt_progress.processed, Some(2048));
            let timings = metadata
                .timings
                .as_ref()
                .expect("timings should be present");
            assert_eq!(timings.prompt_n, Some(2048));
            assert_eq!(timings.prompt_ms, Some(153341.0));
        }
        other => panic!("expected MessageDelta event, got {other:?}"),
    }
}

#[test]
fn test_process_accepts_raw_json_frame_without_sse_delimiter() {
    let mut parser = StreamParser::new();
    let events = parser
        .process(
            br#"{"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"hello"},"finish_reason":"stop"}]}"#,
        )
        .unwrap();

    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
    assert!(matches!(events[1], StreamEvent::ContentBlockDelta { .. }));
    assert!(matches!(events[2], StreamEvent::MessageDelta { .. }));
}

#[test]
fn test_process_emits_error_event_on_unparseable_frame() {
    let mut parser = StreamParser::new();
    let events = parser.process(b"data: not-a-json-value\n\n").unwrap();

    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::Error { error } => {
            assert_eq!(error.error_type, "sse_parse_error");
        }
        other => panic!("expected StreamEvent::Error, got {other:?}"),
    }
}

#[test]
fn test_process_emits_error_event_on_buffer_overflow() {
    let mut parser = StreamParser::new();
    let big_chunk = vec![b'x'; MAX_SSE_BUFFER_BYTES + 1];
    let events = parser.process(&big_chunk).unwrap();

    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::Error { error } => {
            assert_eq!(error.error_type, "sse_buffer_overflow");
        }
        other => panic!("expected StreamEvent::Error on overflow, got {other:?}"),
    }
}

use super::{NormalisedChunk, StreamTextNormaliser};

fn collect_text(chunks: &[NormalisedChunk]) -> String {
    chunks
        .iter()
        .filter_map(|chunk| match chunk {
            NormalisedChunk::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_transcript_lines(chunks: &[NormalisedChunk]) -> Vec<String> {
    chunks
        .iter()
        .filter_map(|chunk| match chunk {
            NormalisedChunk::TranscriptLine(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn test_normaliser_passes_clean_text_through() {
    let mut normaliser = StreamTextNormaliser::new();
    let chunks = normaliser.normalise("Hello world");
    assert_eq!(collect_text(&chunks), "Hello world");
    assert!(collect_transcript_lines(&chunks).is_empty());
}

#[test]
fn test_normaliser_detects_embedded_tool_call() {
    let mut normaliser = StreamTextNormaliser::new();
    let input =
        "function=runshellcommand>\nparameter=command>\ncat file.txt\nparameter>\nfunction>";
    let chunks = normaliser.normalise(input);
    let lines = collect_transcript_lines(&chunks);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[tool]") && line.contains("runshellcommand")),
        "should emit tool transcript line: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[detail]") && line.contains("command")),
        "should emit detail transcript line: {lines:?}"
    );
    let text = collect_text(&chunks);
    assert!(
        !text.contains("function="),
        "raw markup must not leak to text: {text:?}"
    );
}

#[test]
fn test_normaliser_detects_angle_bracket_tool_call() {
    let mut normaliser = StreamTextNormaliser::new();
    let input = "<function=read_file>\n<parameter=path>\nsrc/main.rs\n</parameter>\n</function>";
    let chunks = normaliser.normalise(input);
    let lines = collect_transcript_lines(&chunks);
    assert!(
        lines.iter().any(|line| line.contains("read_file")),
        "should detect angle-bracket format: {lines:?}"
    );
}

#[test]
fn test_normaliser_ignores_tool_call_wrapper_lines() {
    let mut normaliser = StreamTextNormaliser::new();
    let input = "<tool_call>\n<function=read_file>\n<parameter=path>\nsrc/main.rs\n</parameter>\n</function>\n</tool_call>";
    let chunks = normaliser.normalise(input);
    let lines = collect_transcript_lines(&chunks);
    let text = collect_text(&chunks);

    assert!(
        lines.iter().any(|line| line.contains("read_file")),
        "wrapper should not block function detection: {lines:?}"
    );
    assert!(
        !text.contains("<tool_call>") && !text.contains("</tool_call>"),
        "wrapper markers must not leak into text output: {text:?}"
    );
}

#[test]
fn test_normaliser_buffers_chunk_split_tool_call_wrapper_and_function_open() {
    let mut normaliser = StreamTextNormaliser::new();

    let first = normaliser.normalise("Let me inspect it.\n<tool");
    assert_eq!(collect_text(&first), "Let me inspect it.");
    assert!(collect_transcript_lines(&first).is_empty());

    let second = normaliser.normalise("_call>\n<function=search");
    assert_eq!(collect_text(&second), "");
    assert!(collect_transcript_lines(&second).is_empty());

    let third = normaliser
        .normalise("_files>\n<parameter=path>\n.github/workflows/\n</parameter>\n</function>");
    let lines = collect_transcript_lines(&third);
    let text = collect_text(&third);

    assert!(
        lines
            .iter()
            .any(|line| line.contains("[tool] search_files · processing")),
        "chunked function open must produce a tool transcript line: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[detail] path: .github/workflows/")),
        "chunked parameter body must be preserved: {lines:?}"
    );
    assert!(
        !text.contains("<tool_call>") && !text.contains("<function="),
        "chunked control fragments must stay hidden from plain text: {text:?}"
    );

    let flushed = normaliser.flush();
    let flushed_lines = collect_transcript_lines(&flushed);
    let saw_dispatched = lines
        .iter()
        .chain(flushed_lines.iter())
        .any(|line| line.contains("[tool] search_files · dispatched"));
    assert!(
        saw_dispatched,
        "chunked closing tags must dispatch by the end of the stream; third={lines:?} flush={flushed_lines:?}"
    );
}

#[test]
fn test_normaliser_collapses_consecutive_blank_lines() {
    let mut normaliser = StreamTextNormaliser::new();
    let input = "line1\n\n\n\n\n\nline2";
    let chunks = normaliser.normalise(input);
    let text = collect_text(&chunks);
    let blank_run = text
        .split("line1")
        .nth(1)
        .unwrap()
        .split("line2")
        .next()
        .unwrap();
    let blank_count = blank_run.matches('\n').count();
    assert!(
        blank_count <= 3,
        "should collapse blank lines, got {blank_count} newlines between content"
    );
}

#[test]
fn test_normaliser_handles_mixed_tool_and_text() {
    let mut normaliser = StreamTextNormaliser::new();
    let input = "Here is the result:\nfunction=read_file>\nparameter=path>\nsrc/lib.rs\nparameter>\nfunction>\nI found the file.";
    let chunks = normaliser.normalise(input);
    let text = collect_text(&chunks);
    assert!(
        text.contains("Here is the result:"),
        "pre-tool text preserved"
    );
    assert!(
        text.contains("I found the file."),
        "post-tool text preserved"
    );
    assert!(!text.contains("function="), "tool markup stripped");
    let lines = collect_transcript_lines(&chunks);
    assert!(
        lines.iter().any(|line| line.contains("read_file")),
        "tool call detected: {lines:?}"
    );
}

#[test]
fn test_normaliser_reset_clears_state() {
    let mut normaliser = StreamTextNormaliser::new();
    normaliser.normalise("function=some_tool>");
    assert!(normaliser.in_tool_block);
    normaliser.reset();
    assert!(!normaliser.in_tool_block);
    assert!(normaliser.current_tool_name.is_none());
}

#[test]
fn test_normaliser_recovers_from_unterminated_tool_before_next_tool() {
    let mut normaliser = StreamTextNormaliser::new();

    let first = normaliser.normalise("function=read_file>\nparameter=path>\nsrc/main.rs");
    assert_eq!(
        collect_transcript_lines(&first),
        vec!["[tool] read_file · processing"]
    );

    let second = normaliser.normalise(
        "function=runshellcommand>\nparameter=command>\npwd\nparameter>\nfunction>\nRecovered answer.",
    );
    assert_eq!(
        collect_transcript_lines(&second),
        vec![
            "[detail] path: src/main.rs",
            "[tool] read_file · dispatched",
            "[tool] runshellcommand · processing",
            "[detail] command: pwd",
            "[tool] runshellcommand · dispatched",
        ]
    );
    assert_eq!(collect_text(&second), "Recovered answer.");
}

#[test]
fn test_normaliser_flush_emits_orphaned_tool_and_resets_for_follow_up_text() {
    let mut normaliser = StreamTextNormaliser::new();

    normaliser.normalise("function=read_file>\nparameter=path>\nsrc/lib.rs");

    let flushed = normaliser.flush();
    assert_eq!(
        collect_transcript_lines(&flushed),
        vec!["[detail] path: src/lib.rs", "[tool] read_file · dispatched"]
    );
    assert!(!normaliser.in_tool_block);

    let follow_up = normaliser.normalise("Recovered answer.");
    assert_eq!(collect_text(&follow_up), "Recovered answer.");
    assert!(collect_transcript_lines(&follow_up).is_empty());
}

#[test]
fn test_normaliser_compact_param_value_short() {
    let result = super::text_normaliser::compact_param_value("short value");
    assert_eq!(result, "short value");
}

#[test]
fn test_normaliser_compact_param_value_long() {
    let long = "a".repeat(100);
    let result = super::text_normaliser::compact_param_value(&long);
    assert!(result.len() <= 81, "should be truncated: {}", result.len());
    assert!(result.ends_with('\u{2026}'));
}

#[test]
fn test_normaliser_compact_param_value_long_unicode_is_boundary_safe() {
    let long = format!("{}{}", "你".repeat(78), "\nsecond line");
    let result = super::text_normaliser::compact_param_value(&long);

    assert_eq!(result.chars().count(), 78);
    assert!(result.ends_with('\u{2026}'));
    assert!(result.starts_with(&"你".repeat(77)));
}

// ── Normaliser malformed-markup hardening (ADR-043 gate 2 fixtures) ─

#[test]
fn test_normaliser_parameter_without_preceding_function_is_passthrough() {
    let mut normaliser = StreamTextNormaliser::new();
    let chunks = normaliser.normalise("parameter=path>\nsrc/main.rs\nparameter>");
    let text = collect_text(&chunks);
    // With no preceding function= tag the normaliser is not in a tool block,
    // so parameter-like lines pass through as plain text.
    assert!(
        text.contains("parameter=path"),
        "bare parameter should pass through: {text:?}"
    );
}

#[test]
fn test_normaliser_nested_function_tags_auto_close_first() {
    let mut normaliser = StreamTextNormaliser::new();
    let input = concat!(
        "function=read_file>\n",
        "parameter=path>\nfoo.rs\nparameter>\n",
        "function=write_file>\n",
        "parameter=path>\nbar.rs\nparameter>\n",
        "function>"
    );
    let chunks = normaliser.normalise(input);
    let lines = collect_transcript_lines(&chunks);
    // The first tool block must be auto-closed when the second function= appears.
    assert!(
        lines
            .iter()
            .any(|l| l.contains("read_file") && l.contains("dispatched")),
        "first tool should be auto-closed: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("write_file") && l.contains("dispatched")),
        "second tool should complete normally: {lines:?}"
    );
}

#[test]
fn test_normaliser_empty_function_name_is_passthrough() {
    let mut normaliser = StreamTextNormaliser::new();
    let chunks = normaliser.normalise("function=>");
    let text = collect_text(&chunks);
    assert!(
        text.contains("function="),
        "empty function name should pass through: {text:?}"
    );
    assert!(
        collect_transcript_lines(&chunks).is_empty(),
        "no transcript lines for empty function name"
    );
}

#[test]
fn test_normaliser_malformed_close_without_open_is_passthrough() {
    let mut normaliser = StreamTextNormaliser::new();
    let chunks = normaliser.normalise("</function>\nsome text");
    let text = collect_text(&chunks);
    // A close tag with no preceding open should not disrupt normal text.
    assert!(
        text.contains("some text"),
        "normal text after stale close should survive: {text:?}"
    );
}

#[test]
fn test_normaliser_mixed_angle_and_bare_formats() {
    let mut normaliser = StreamTextNormaliser::new();
    // First call: angle-bracket format
    let chunks1 = normaliser
        .normalise("<function=read_file>\n<parameter=path>\nsrc/lib.rs\n</parameter>\n</function>");
    let lines1 = collect_transcript_lines(&chunks1);
    assert!(
        lines1.iter().any(|l| l.contains("read_file")),
        "angle-bracket format detected: {lines1:?}"
    );
    // Second call: bare format (no angle brackets)
    let chunks2 = normaliser
        .normalise("function=write_file>\nparameter=path>\nout.rs\nparameter>\nfunction>");
    let lines2 = collect_transcript_lines(&chunks2);
    assert!(
        lines2.iter().any(|l| l.contains("write_file")),
        "bare format detected after angle-bracket: {lines2:?}"
    );
}

#[test]
fn test_normaliser_blank_lines_inside_tool_block_are_suppressed() {
    let mut normaliser = StreamTextNormaliser::new();
    let input = "function=read_file>\nparameter=path>\n\n\n\nsrc/main.rs\nparameter>\nfunction>";
    let chunks = normaliser.normalise(input);
    let lines = collect_transcript_lines(&chunks);
    // Tool blocks produce structured transcript lines; blank lines inside
    // the parameter value should be preserved in the param value itself,
    // not leaked as separate NormalisedChunk::Text entries.
    let text_chunks: Vec<_> = chunks
        .iter()
        .filter_map(|c| match c {
            NormalisedChunk::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert!(
        text_chunks
            .iter()
            .all(|t| t.is_empty() || !t.trim().is_empty()),
        "no non-empty text should leak from tool block: {text_chunks:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("[detail]") && l.contains("path")),
        "parameter detail should be emitted: {lines:?}"
    );
}

#[test]
fn test_normaliser_consecutive_flushes_are_idempotent() {
    let mut normaliser = StreamTextNormaliser::new();
    normaliser.normalise("function=read_file>\nparameter=path>\nsrc/main.rs");
    let first = normaliser.flush();
    assert!(
        !first.is_empty(),
        "first flush should emit pending tool block"
    );
    let second = normaliser.flush();
    assert!(
        second.is_empty(),
        "repeated flush with no new input should be empty"
    );
    assert!(!normaliser.in_tool_block);
}
