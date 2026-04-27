use super::{MAX_SSE_BUFFER_BYTES, StreamParser};
use crate::runtime::RuntimeSignal;

mod messages_v1;

#[test]
fn stream_parser_ignores_ping_frames() {
    let frames = StreamParser::new()
        .process(b"event: ping\ndata: {\"type\":\"ping\"}\n\n")
        .unwrap();
    assert!(frames.is_empty());
}

#[test]
fn stream_parser_chat_compat_yields_metadata_and_text_delta() {
    let frames = StreamParser::new()
        .process(br#"data: {"id":"c1","object":"chat.completion.chunk","created":1741730100,"model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"hello"},"finish_reason":null}]}

"#)
        .unwrap();
    assert!(frames.iter().any(|e| matches!(
        &e.signal,
        RuntimeSignal::ServerMetadata { metadata }
            if metadata.object.as_deref() == Some("chat.completion.chunk")
    )));
    assert!(frames.iter().any(|e| matches!(
        &e.signal,
        RuntimeSignal::TranscriptBlockDelta { delta, .. } if delta == "hello"
    )));
}

#[test]
fn stream_parser_returns_error_on_buffer_overflow() {
    let mut parser = StreamParser::new();
    let big = vec![b'x'; MAX_SSE_BUFFER_BYTES + 1];
    let frames = parser.process(&big).unwrap();
    assert!(matches!(
        &frames[0].signal,
        RuntimeSignal::Error { code, .. } if code == "sse_buffer_overflow"
    ));
    let follow = parser.process(b"still-overflowed").unwrap();
    assert!(matches!(&follow[0].signal, RuntimeSignal::Error { .. }));
}

#[test]
fn stream_parser_returns_error_on_unparseable_frame() {
    let frames = StreamParser::new().process(b"data: not-json\n\n").unwrap();
    assert!(matches!(
        &frames[0].signal,
        RuntimeSignal::Error { code, .. } if code == "sse_parse_error"
    ));
}
