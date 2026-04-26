use super::*;
use tempfile::NamedTempFile;

#[tokio::test]
async fn fallback_writes_protocol_discovery_and_shape_debug_records_for_both_protocols() {
    for (label, spawn_fn, expected_protocol, expected_count_key) in [
        (
            "chat-compat",
            "chat",
            "chat-compat",
            "\"tool_argument_object_count\":1",
        ),
        (
            "messages-v1",
            "messages",
            "messages-v1",
            "\"tool_use_count\":1",
        ),
    ] {
        let _env_lock = test_support::ENV_LOCK.lock().await;
        let _payload_restore = test_support::EnvRestore::capture(&_env_lock, "VEX_DEBUG_PAYLOAD");
        let _path_restore = test_support::EnvRestore::capture(&_env_lock, "VEX_API_LOG_PATH");
        let log_file = NamedTempFile::new().expect("temp log file");
        _env_lock.set_var("VEX_DEBUG_PAYLOAD", "1");
        _env_lock.set_var("VEX_API_LOG_PATH", log_file.path());

        let (base_url, server) = if spawn_fn == "chat" {
            spawn_tool_calls_json_args_server().await
        } else {
            spawn_auto_detect_messages_v1_tool_calls_server().await
        };
        let model_name = if spawn_fn == "chat" {
            "json-args-model"
        } else {
            "tool-detect-model"
        };
        let prompt = if spawn_fn == "chat" {
            "Read main.rs."
        } else {
            "Read lib.rs."
        };
        let config = build_auto_detect_batch_config(&base_url, model_name);
        let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
        client.populate_server_info().await;

        let mut stream = client
            .create_stream(&single_user_message(prompt))
            .await
            .expect("must not error");
        while let Some(envelope) = stream.next().await {
            envelope.expect("envelope must not error");
        }

        let log = std::fs::read_to_string(log_file.path()).expect("debug log must be readable");
        for expected in [
            "\"event\":\"api.protocol_discovery\"",
            &format!("\"selected_protocol\":\"{expected_protocol}\""),
            "\"event\":\"api.non_stream_fallback_request\"",
            "\"event\":\"api.non_stream_fallback_response\"",
            "\"event\":\"api.non_stream_fallback_shape\"",
            expected_count_key,
        ] {
            assert!(
                log.contains(expected),
                "{label}: missing {expected} in log: {log}"
            );
        }
        server.abort();
    }
}
