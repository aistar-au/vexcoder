use super::*;
use std::sync::RwLock;

fn assert_local_runtime_client(
    model_url: &str,
    configured_protocol: ModelProtocol,
    expected_api_protocol: ApiProtocol,
    expected_request_url: &str,
) {
    let config = local_stream_test_config(model_url.to_string(), configured_protocol);
    let client = ApiClient::new(&config).expect("client should build");

    assert_eq!(client.api_protocol(), expected_api_protocol);
    assert_eq!(client.request_url(), expected_request_url);
    assert_eq!(client.protocol(), configured_protocol);
}

fn client_with_server_info(
    api_url: &str,
    configured_protocol: ModelProtocol,
    native_protocol: Option<ModelProtocol>,
) -> ApiClient {
    ApiClient {
        http: reqwest::Client::new(),
        api_key: None,
        model: Arc::new(RwLock::new("test".to_string())),
        supplementary_system_prompt: Arc::new(RwLock::new(None)),
        api_url: api_url.to_string(),
        api_client_explicit_protocol: None,
        probe_timeout_ms: 2000,
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: configured_protocol,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_headers: reqwest::header::HeaderMap::new(),
        temperature: 0.3,
        top_p: 1.0,
        max_tokens: 4096,
        stop_sequences: Vec::new(),
        reasoning_budget: 0,
        project_instructions: None,
        notes_content: None,
        extra_tool_definitions: Vec::new(),
        server_info: Arc::new(RwLock::new(Some(ServerInfo {
            n_ctx: 65536,
            n_batch: 2048,
            model: "test".to_string(),
            native_protocol,
        }))),
        tls_verification_disabled: false,
        #[cfg(test)]
        mock_stream_producer: None,
    }
}

fn assert_message_contains_all(message: &str, expected_substrings: &[&str]) {
    for expected in expected_substrings {
        assert!(
            message.contains(expected),
            "expected {message:?} to contain {expected:?}"
        );
    }
}

#[test]
fn test_protocol_inference_and_url_adaptation_cover_supported_local_shapes() {
    let inference_cases = [
        ("http://localhost:8000/v1/messages", ApiProtocol::MessagesV1),
        (
            "http://localhost:8000/v1/chat/completions",
            ApiProtocol::ChatCompat,
        ),
        ("http://127.0.0.1:8000/messages/v1", ApiProtocol::MessagesV1),
        ("http://localhost:8000/v1", ApiProtocol::MessagesV1),
    ];
    for (url, expected_protocol) in inference_cases {
        assert_eq!(
            infer_api_protocol(url),
            expected_protocol,
            "unexpected protocol inference for {url}"
        );
    }

    let adaptation_cases = [
        (
            "http://127.0.0.1:8000/messages/v1",
            "http://127.0.0.1:8000/v1/messages",
            "http://127.0.0.1:8000/v1/chat/completions",
        ),
        (
            "http://127.0.0.1:8000/messages/v1/",
            "http://127.0.0.1:8000/v1/messages",
            "http://127.0.0.1:8000/v1/chat/completions",
        ),
        (
            "http://localhost:8000/v1/chat/completions",
            "http://localhost:8000/v1/messages",
            "http://localhost:8000/v1/chat/completions",
        ),
        (
            "http://localhost:8000/v1",
            "http://localhost:8000/v1/messages",
            "http://localhost:8000/v1/chat/completions",
        ),
        (
            "http://localhost:8000/v1/messages",
            "http://localhost:8000/v1/messages",
            "http://localhost:8000/v1/chat/completions",
        ),
    ];
    for (input, expected_messages_url, expected_chat_url) in adaptation_cases {
        assert_eq!(
            adapt_to_messages_v1_url(input),
            expected_messages_url,
            "unexpected messages/v1 adaptation for {input}"
        );
        assert_eq!(
            adapt_to_chat_compat_url(input),
            expected_chat_url,
            "unexpected chat-compat adaptation for {input}"
        );
    }
}

#[test]
fn test_local_runtime_request_url_resolution_respects_configured_protocol_and_explicit_paths() {
    let cases = [
        (
            "http://localhost:8000/v1/messages",
            ModelProtocol::MessagesV1,
            ApiProtocol::MessagesV1,
            "http://localhost:8000/v1/messages",
        ),
        (
            "http://localhost:8000/v1",
            ModelProtocol::MessagesV1,
            ApiProtocol::MessagesV1,
            "http://localhost:8000/v1/messages",
        ),
        (
            "http://localhost:8000/v1",
            ModelProtocol::ChatCompat,
            ApiProtocol::ChatCompat,
            "http://localhost:8000/v1/chat/completions",
        ),
    ];

    for (model_url, configured_protocol, expected_api_protocol, expected_request_url) in cases {
        assert_local_runtime_client(
            model_url,
            configured_protocol,
            expected_api_protocol,
            expected_request_url,
        );
    }
}

#[test]
fn test_api_client_base_url_explicit_protocol_controls_request_url() {
    let mut config = crate::config::Config::default_for_tui();
    config.model_name = "local/test-model".to_string();
    config.model_url.clear();
    config.api_client.base_url = "http://127.0.0.1:8787".to_string();
    config.api_client.explicit_protocol = Some(ModelProtocol::ChatCompat);

    let client = ApiClient::new(&config).expect("client should build");

    assert_eq!(client.protocol(), ModelProtocol::ChatCompat);
    assert_eq!(
        client.request_url(),
        "http://127.0.0.1:8787/v1/chat/completions"
    );
}

#[test]
fn test_map_api_status_error_429_variants_surface_available_retry_hints() {
    let retry_after_seconds = map_api_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "rate limit exceeded",
        "https://api.example.com/v1/messages",
        Some("5"),
    )
    .to_string();
    assert_message_contains_all(&retry_after_seconds, &["rate limited", "5.0s"]);

    let retry_after_http_date =
        httpdate::fmt_http_date(std::time::SystemTime::now() + std::time::Duration::from_secs(3));
    let retry_after_date_message = map_api_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "rate limit exceeded",
        "https://api.example.com/v1/messages",
        Some(&retry_after_http_date),
    )
    .to_string();
    assert_message_contains_all(
        &retry_after_date_message,
        &["rate limited", "Retry suggested after"],
    );

    let body_hint_message = map_api_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "try again in 30 seconds",
        "https://api.example.com/v1/messages",
        None,
    )
    .to_string();
    assert_message_contains_all(&body_hint_message, &["rate limited", "30.0s"]);

    let no_hint_message = map_api_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "too many requests",
        "https://api.example.com/v1/messages",
        None,
    )
    .to_string();
    assert_message_contains_all(&no_hint_message, &["rate limited"]);
    assert!(
        !no_hint_message.contains("Retry suggested"),
        "unexpected retry hint in {no_hint_message:?}"
    );
}

#[test]
fn test_apply_local_chat_compat_stream_flags_adds_progress_fields() {
    let mut payload = serde_json::Map::new();

    apply_local_chat_compat_stream_flags(&mut payload);

    assert_eq!(payload.get("return_progress"), Some(&json!(true)));
    assert_eq!(payload.get("timings_per_token"), Some(&json!(true)));
    assert_eq!(
        payload.get("cache_prompt"),
        Some(&json!(true)),
        "cache_prompt must be enabled for local servers to allow batch prompt evaluation"
    );
}

#[test]
fn test_native_protocol_overrides_configured_protocol_for_bare_base_url() {
    let client = client_with_server_info(
        "http://localhost:8000/v1",
        ModelProtocol::MessagesV1,
        Some(ModelProtocol::ChatCompat),
    );

    assert_eq!(
        client.api_protocol(),
        ApiProtocol::ChatCompat,
        "client must prefer the discovered native chat-compat protocol for bare base URLs"
    );
    assert_eq!(
        client.request_url(),
        "http://localhost:8000/v1/chat/completions"
    );
}

#[test]
fn test_explicit_model_url_paths_remain_authoritative_over_discovery() {
    let chat_compat_config = local_stream_test_config(
        "http://localhost:8000/v1/chat/completions".to_string(),
        ModelProtocol::MessagesV1,
    );
    let chat_compat_client = ApiClient::new(&chat_compat_config).expect("client should build");
    chat_compat_client.set_server_info(ServerInfo {
        native_protocol: Some(ModelProtocol::MessagesV1),
        ..ServerInfo::default()
    });
    assert_eq!(chat_compat_client.api_protocol(), ApiProtocol::ChatCompat);
    assert_eq!(
        chat_compat_client.request_url(),
        "http://localhost:8000/v1/chat/completions"
    );

    let messages_config = local_stream_test_config(
        "http://localhost:8000/v1/messages".to_string(),
        ModelProtocol::ChatCompat,
    );
    let messages_client = ApiClient::new(&messages_config).expect("client should build");
    messages_client.set_server_info(ServerInfo {
        native_protocol: Some(ModelProtocol::ChatCompat),
        ..ServerInfo::default()
    });
    assert_eq!(messages_client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(
        messages_client.request_url(),
        "http://localhost:8000/v1/messages"
    );
}

#[test]
fn test_no_native_protocol_falls_back_to_configured_protocol() {
    let client =
        client_with_server_info("http://localhost:8000/v1", ModelProtocol::MessagesV1, None);

    assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
}
