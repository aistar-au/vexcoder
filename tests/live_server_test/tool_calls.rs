use super::*;

#[tokio::test]
async fn test_stalled_stream_chat_compat_json_form_arguments_normalizes_correctly() {
    let (base_url, server) = spawn_tool_calls_json_args_server().await;
    let config = build_auto_detect_batch_config(&base_url, "json-args-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    assert_eq!(
        client.server_info().and_then(|info| info.native_protocol),
        Some(ModelProtocol::ChatCompat),
        "auto-detect should select chat-compat before issuing the request"
    );

    let mut stream = client
        .create_stream(&single_user_message("Read main.rs."))
        .await
        .expect("json-args fallback must not error");

    let mut envelopes = Vec::new();
    while let Some(envelope) = stream.next().await {
        envelopes.push(envelope.expect("envelope must not error"));
    }

    assert!(
        envelopes.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::ToolCallStarted { tool_name, arguments, .. }
                if tool_name == "read_file"
                    && arguments.get("path").and_then(Value::as_str) == Some("src/main.rs")
        )),
        "json-form arguments must materialize as ToolCallStarted with correct input; got {:?}",
        envelopes.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert!(
        !envelopes
            .iter()
            .any(|e| matches!(&e.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "json-form arguments must not emit argument deltas"
    );

    server.abort();
}

#[tokio::test]
async fn test_run_batch_auto_detects_messages_v1_and_normalizes_fallback_tool_calls() {
    let (base_url, server) = spawn_auto_detect_messages_v1_tool_calls_server().await;
    let config = build_auto_detect_batch_config(&base_url, "tool-detect-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    assert_eq!(
        client.server_info().and_then(|info| info.native_protocol),
        Some(ModelProtocol::MessagesV1),
        "auto-detect should select messages-v1 before issuing the request"
    );

    let mut stream = client
        .create_stream(&single_user_message("Read lib.rs."))
        .await
        .expect("messages-v1 tool-call fallback must not error");

    let mut envelopes = Vec::new();
    while let Some(envelope) = stream.next().await {
        envelopes.push(envelope.expect("envelope must not error"));
    }

    assert!(
        envelopes.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::ToolCallStarted { tool_name, arguments, .. }
                if tool_name == "read_file"
                    && arguments.get("path").and_then(Value::as_str) == Some("src/lib.rs")
        )),
        "auto-detected messages-v1 tool call must materialize; got {:?}",
        envelopes.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert!(
        !envelopes
            .iter()
            .any(|e| matches!(&e.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized messages-v1 tool calls must not emit argument deltas"
    );

    server.abort();
}
