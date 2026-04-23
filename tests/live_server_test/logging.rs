#![allow(unsafe_code)]

use super::*;
use tempfile::NamedTempFile;

struct EnvVarRestore {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvVarRestore {
    fn capture(key: &'static str) -> Self {
        Self {
            key,
            original: std::env::var_os(key),
        }
    }
}

impl Drop for EnvVarRestore {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            // SAFETY: Test env-var changes are serialized via

            unsafe { std::env::set_var(self.key, value) };
        } else {
            // SAFETY: Test env-var changes are serialized via

            unsafe { std::env::remove_var(self.key) };
        }
    }
}

#[tokio::test]
async fn test_chat_compat_fallback_writes_protocol_and_shape_debug_records() {
    let _env_lock = test_support::ENV_LOCK.lock().await;
    let _payload_restore = EnvVarRestore::capture("VEX_DEBUG_PAYLOAD");
    let _path_restore = EnvVarRestore::capture("VEX_API_LOG_PATH");
    let log_file = NamedTempFile::new().expect("temp log file");

    // SAFETY: `test_support::ENV_LOCK` is held for the duration of this test,

    unsafe { std::env::set_var("VEX_DEBUG_PAYLOAD", "1") };
    // SAFETY: `test_support::ENV_LOCK` is held for the duration of this test,

    unsafe { std::env::set_var("VEX_API_LOG_PATH", log_file.path()) };

    let (base_url, server) = spawn_tool_calls_json_args_server().await;
    let config = build_auto_detect_batch_config(&base_url, "json-args-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    let mut stream = client
        .create_stream(&single_user_message("Read main.rs."))
        .await
        .expect("json-args fallback must not error");
    while let Some(envelope) = stream.next().await {
        envelope.expect("envelope must not error");
    }

    let log = std::fs::read_to_string(log_file.path()).expect("debug log must be readable");
    assert!(
        log.contains("\"event\":\"api.protocol_discovery\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"selected_protocol\":\"chat-compat\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"event\":\"api.non_stream_fallback_request\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"event\":\"api.non_stream_fallback_response\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"event\":\"api.non_stream_fallback_shape\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"tool_argument_object_count\":1"),
        "log: {log}"
    );

    server.abort();
}

#[tokio::test]
async fn test_messages_v1_fallback_writes_protocol_and_shape_debug_records() {
    let _env_lock = test_support::ENV_LOCK.lock().await;
    let _payload_restore = EnvVarRestore::capture("VEX_DEBUG_PAYLOAD");
    let _path_restore = EnvVarRestore::capture("VEX_API_LOG_PATH");
    let log_file = NamedTempFile::new().expect("temp log file");

    // SAFETY: `test_support::ENV_LOCK` is held for the duration of this test,

    unsafe { std::env::set_var("VEX_DEBUG_PAYLOAD", "1") };
    // SAFETY: `test_support::ENV_LOCK` is held for the duration of this test,

    unsafe { std::env::set_var("VEX_API_LOG_PATH", log_file.path()) };

    let (base_url, server) = spawn_auto_detect_messages_v1_tool_calls_server().await;
    let config = build_auto_detect_batch_config(&base_url, "tool-detect-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    let mut stream = client
        .create_stream(&single_user_message("Read lib.rs."))
        .await
        .expect("messages-v1 tool-call fallback must not error");
    while let Some(envelope) = stream.next().await {
        envelope.expect("envelope must not error");
    }

    let log = std::fs::read_to_string(log_file.path()).expect("debug log must be readable");
    assert!(
        log.contains("\"event\":\"api.protocol_discovery\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"selected_protocol\":\"messages-v1\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"event\":\"api.non_stream_fallback_request\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"event\":\"api.non_stream_fallback_response\""),
        "log: {log}"
    );
    assert!(
        log.contains("\"event\":\"api.non_stream_fallback_shape\""),
        "log: {log}"
    );
    assert!(log.contains("\"tool_use_count\":1"), "log: {log}");
    assert!(
        log.contains("\"tool_use_input_object_count\":1"),
        "log: {log}"
    );

    server.abort();
}
