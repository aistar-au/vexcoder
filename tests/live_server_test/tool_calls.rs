use super::*;
use rstest::rstest;

#[rstest]
#[case::chat_compat(true)]
#[case::messages_v1(false)]
#[tokio::test]
async fn auto_detected_protocol_normalizes_tool_call_fallback_for_both_protocols(
    #[case] is_chat: bool,
) {
    let (base_url, server) = if is_chat {
        spawn_tool_calls_json_args_server().await
    } else {
        spawn_auto_detect_messages_v1_tool_calls_server().await
    };
    let model_name = if is_chat {
        "json-args-model"
    } else {
        "tool-detect-model"
    };
    let prompt = if is_chat {
        "Read main.rs."
    } else {
        "Read lib.rs."
    };
    let expected_path = if is_chat { "src/main.rs" } else { "src/lib.rs" };
    let expected_protocol = if is_chat {
        ModelProtocol::ChatCompat
    } else {
        ModelProtocol::MessagesV1
    };

    let config = build_auto_detect_batch_config(&base_url, model_name);
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    assert_eq!(
        client.server_info().and_then(|info| info.native_protocol),
        Some(expected_protocol),
        "auto-detect should select the correct protocol"
    );

    let mut stream = client
        .create_stream(&single_user_message(prompt))
        .await
        .expect("must not error");
    let envelopes: Vec<_> = {
        let mut v = Vec::new();
        while let Some(e) = stream.next().await {
            v.push(e.expect("envelope"));
        }
        v
    };

    let found_tool_call = envelopes.iter().any(|e| {
        matches!(
            &e.event,
            RuntimeEvent::ToolCallStarted { tool_name, arguments, .. }
                if tool_name == "read_file"
                    && arguments.get("path").and_then(Value::as_str) == Some(expected_path)
        )
    });
    assert!(
        found_tool_call,
        "must emit materialized ToolCallStarted; got {:?}",
        envelopes.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert!(
        !envelopes
            .iter()
            .any(|e| matches!(&e.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized tool call must not emit argument deltas"
    );
    server.abort();
}
