use super::{MAX_SSE_BUFFER_BYTES, StreamParser};
use crate::runtime::RuntimeEvent;

mod messages_v1;

#[test]
fn stream_parser_ignores_ping_frames() {
    let events = StreamParser::new()
        .process(b"event: ping\ndata: {\"type\":\"ping\"}\n\n")
        .unwrap();
    assert!(events.is_empty());
}

#[test]
fn stream_parser_chat_compat_yields_metadata_and_text_delta() {
    let events = StreamParser::new()
        .process(br#"data: {"id":"c1","object":"chat.completion.chunk","created":1741730100,"model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"hello"},"finish_reason":null}]}

"#)
        .unwrap();
    assert!(events.iter().any(|e| matches!(
        &e.event,
        RuntimeEvent::ServerMetadata { metadata }
            if metadata.object.as_deref() == Some("chat.completion.chunk")
    )));
    assert!(events.iter().any(|e| matches!(
        &e.event,
        RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "hello"
    )));
}

#[test]
fn stream_parser_returns_error_on_buffer_overflow() {
    let mut parser = StreamParser::new();
    let big = vec![b'x'; MAX_SSE_BUFFER_BYTES + 1];
    let events = parser.process(&big).unwrap();
    assert!(matches!(
        &events[0].event,
        RuntimeEvent::Error { code, .. } if code == "sse_buffer_overflow"
    ));
    let follow = parser.process(b"still-overflowed").unwrap();
    assert!(matches!(&follow[0].event, RuntimeEvent::Error { .. }));
}

#[test]
fn stream_parser_returns_error_on_unparseable_frame() {
    let events = StreamParser::new().process(b"data: not-json\n\n").unwrap();
    assert!(matches!(
        &events[0].event,
        RuntimeEvent::Error { code, .. } if code == "sse_parse_error"
    ));
}
