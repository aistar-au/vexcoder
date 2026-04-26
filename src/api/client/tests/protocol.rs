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

#[test]
fn protocol_inference_and_url_adaptation_for_local_shapes() {
    for (url, expected) in [
        ("http://localhost:8000/v1/messages", ApiProtocol::MessagesV1),
        (
            "http://localhost:8000/v1/chat/completions",
            ApiProtocol::ChatCompat,
        ),
        ("http://127.0.0.1:8000/messages/v1", ApiProtocol::MessagesV1),
        ("http://localhost:8000/v1", ApiProtocol::MessagesV1),
    ] {
        assert_eq!(
            infer_api_protocol(url),
            expected,
            "inference mismatch for {url}"
        );
    }

    for (input, messages_url, chat_url) in [
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
    ] {
        assert_eq!(adapt_to_messages_v1_url(input), messages_url);
        assert_eq!(adapt_to_chat_compat_url(input), chat_url);
    }
}

#[test]
fn local_runtime_request_url_respects_configured_protocol() {
    for (url, proto, api_proto, req_url) in [
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
    ] {
        assert_local_runtime_client(url, proto, api_proto, req_url);
    }
}

#[test]
fn native_protocol_overrides_configured_for_bare_base_url() {
    let client = ApiClient {
        http: reqwest::Client::new(),
        api_key: None,
        model: Arc::new(RwLock::new("test".to_string())),
        supplementary_system_prompt: Arc::new(RwLock::new(None)),
        api_url: "http://localhost:8000/v1".to_string(),
        api_client_explicit_protocol: None,
        probe_timeout_ms: 2000,
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
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
            native_protocol: Some(ModelProtocol::ChatCompat),
        }))),
        tls_verification_disabled: false,
        #[cfg(test)]
        mock_stream_producer: None,
    };
    assert_eq!(
        client.api_protocol(),
        ApiProtocol::ChatCompat,
        "native protocol must override configured"
    );
    assert_eq!(
        client.request_url(),
        "http://localhost:8000/v1/chat/completions"
    );
}
