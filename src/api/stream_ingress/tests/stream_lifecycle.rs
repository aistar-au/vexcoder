use super::*;
use bytes::Bytes;

#[tokio::test]
async fn eof_flushes_pulse_end_and_does_not_repoll_upstream() {
    let mut parser = StreamParser::new();
    let _ = parser.process(b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":15}}\n\n").unwrap();

    let polls = Arc::new(AtomicUsize::new(0));
    let polls_for_stream = Arc::clone(&polls);
    let upstream = stream::poll_fn(move |_| {
        if polls_for_stream.fetch_add(1, Ordering::SeqCst) == 0 {
            std::task::Poll::Ready(None)
        } else {
            panic!("upstream polled again after EOF")
        }
    });

    let mut state = SignalStreamState {
        upstream: Box::pin(upstream),
        parser,
        pending: VecDeque::new(),
        terminal_outcome: None,
        request_url: "https://example.test/sse".to_string(),
        request_id: "req-test-eof".to_string(),
        protocol_name: "messages-v1",
        started_at: Instant::now(),
        envelope_count: 0,
        tool_call_starts: 0,
        tool_call_completions: 0,
        tool_call_failures: 0,
        last_usage: None,
        final_status: None,
        summary_emitted: false,
    };

    let envelope = next_stream_item(&mut state).await.unwrap().unwrap();
    assert!(matches!(envelope.signal, RuntimeSignal::PulseEnd { .. }));
    assert!(next_stream_item(&mut state).await.unwrap().is_none());
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chat_compat_done_finalizes_without_waiting_for_transport_eof() {
    let mut parser = StreamParser::new();
    let _ = parser.process(br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"done"},"finish_reason":"stop"}]}

"#).unwrap();

    let polls = Arc::new(AtomicUsize::new(0));
    let polls_for_stream = Arc::clone(&polls);
    let upstream = stream::poll_fn(move |_| {
        if polls_for_stream.fetch_add(1, Ordering::SeqCst) == 0 {
            std::task::Poll::Ready(Some(Ok(Bytes::from_static(b"data: [DONE]\n\n"))))
        } else {
            panic!("upstream polled again after chat-compatible [DONE]")
        }
    });

    let mut state = SignalStreamState {
        upstream: Box::pin(upstream),
        parser,
        pending: VecDeque::new(),
        terminal_outcome: None,
        request_url: "https://example.test/chat-completions".to_string(),
        request_id: "req-test-chat-done".to_string(),
        protocol_name: "chat-compat",
        started_at: Instant::now(),
        envelope_count: 0,
        tool_call_starts: 0,
        tool_call_completions: 0,
        tool_call_failures: 0,
        last_usage: None,
        final_status: None,
        summary_emitted: false,
    };

    let mut emitted = Vec::new();
    while let Some(envelope) = next_stream_item(&mut state).await.unwrap() {
        emitted.push(envelope);
    }

    assert!(
        emitted
            .iter()
            .any(|e| matches!(e.signal, RuntimeSignal::PulseEnd { .. })),
        "chat-compatible [DONE] must flush PulseEnd; got {:?}",
        emitted.iter().map(|e| &e.signal).collect::<Vec<_>>()
    );
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}
