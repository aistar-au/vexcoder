use super::*;
use std::sync::RwLock;

#[test]
fn test_protocol_inference_defaults_to_messages_v1() {
    let protocol = infer_api_protocol("http://localhost:8000/v1/messages");
    assert_eq!(protocol, ApiProtocol::MessagesV1);
}

#[test]
fn test_protocol_inference_detects_chat_compat() {
    let protocol = infer_api_protocol("http://localhost:8000/v1/chat/completions");
    assert_eq!(protocol, ApiProtocol::ChatCompat);
}

#[test]
fn test_local_messages_endpoint_keeps_messages_v1_wire_protocol() {
    let config = crate::config::Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: crate::types::ModelProfile::default_for_backend(
            ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
    assert_eq!(client.protocol(), ModelProtocol::MessagesV1);
}

#[test]
fn test_local_bare_v1_endpoint_resolves_messages_v1_url() {
    let config = crate::config::Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:8000/v1".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: crate::types::ModelProfile::default_for_backend(
            ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
    assert_eq!(client.protocol(), ModelProtocol::MessagesV1);
}

#[test]
fn test_local_bare_v1_endpoint_resolves_chat_compat_url() {
    let config = crate::config::Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:8000/v1".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::ChatCompat,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: crate::types::ModelProfile::default_for_backend(
            ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert_eq!(client.api_protocol(), ApiProtocol::ChatCompat);
    assert_eq!(
        client.request_url(),
        "http://localhost:8000/v1/chat/completions"
    );
    assert_eq!(client.protocol(), ModelProtocol::ChatCompat);
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
fn test_map_api_status_error_429_with_retry_after_header() {
    let err = map_api_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "rate limit exceeded",
        "https://api.example.com/v1/messages",
        Some("5"),
    );
    let msg = format!("{}", err);
    assert!(msg.contains("rate limited"), "got: {msg}");
    assert!(msg.contains("5.0s"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_429_with_retry_after_http_date_header() {
    let retry_after =
        httpdate::fmt_http_date(std::time::SystemTime::now() + std::time::Duration::from_secs(3));
    let err = map_api_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "rate limit exceeded",
        "https://api.example.com/v1/messages",
        Some(&retry_after),
    );
    let msg = format!("{}", err);
    assert!(msg.contains("rate limited"), "got: {msg}");
    assert!(msg.contains("Retry suggested after"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_429_body_fallback() {
    let err = map_api_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "try again in 30 seconds",
        "https://api.example.com/v1/messages",
        None,
    );
    let msg = format!("{}", err);
    assert!(msg.contains("rate limited"), "got: {msg}");
    assert!(msg.contains("30.0s"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_429_no_hint() {
    let err = map_api_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "too many requests",
        "https://api.example.com/v1/messages",
        None,
    );
    let msg = format!("{}", err);
    assert!(msg.contains("rate limited"), "got: {msg}");
    assert!(!msg.contains("Retry suggested"), "got: {msg}");
}

#[test]
fn test_adapt_chat_compat_url_from_transposed_messages_v1() {
    let adapted = adapt_to_chat_compat_url("http://127.0.0.1:8000/messages/v1");
    assert_eq!(adapted, "http://127.0.0.1:8000/v1/chat/completions");
}

#[test]
fn test_adapt_messages_v1_url_from_transposed_messages_v1() {
    let adapted = adapt_to_messages_v1_url("http://127.0.0.1:8000/messages/v1");
    assert_eq!(adapted, "http://127.0.0.1:8000/v1/messages");
}

#[test]
fn test_adapt_chat_compat_url_from_transposed_messages_v1_with_trailing_slash() {
    let adapted = adapt_to_chat_compat_url("http://127.0.0.1:8000/messages/v1/");
    assert_eq!(adapted, "http://127.0.0.1:8000/v1/chat/completions");
}

#[test]
fn test_adapt_messages_v1_url_from_transposed_messages_v1_with_trailing_slash() {
    let adapted = adapt_to_messages_v1_url("http://127.0.0.1:8000/messages/v1/");
    assert_eq!(adapted, "http://127.0.0.1:8000/v1/messages");
}

#[test]
fn test_protocol_inference_transposed_messages_v1_is_messages() {
    let protocol = infer_api_protocol("http://127.0.0.1:8000/messages/v1");
    assert_eq!(protocol, ApiProtocol::MessagesV1);
}

#[test]
fn test_protocol_inference_standard_v1_messages_is_messages() {
    let protocol = infer_api_protocol("http://localhost:8000/v1/messages");
    assert_eq!(protocol, ApiProtocol::MessagesV1);
}

#[test]
fn test_protocol_inference_bare_v1_is_messages_v1() {
    let protocol = infer_api_protocol("http://localhost:8000/v1");
    assert_eq!(protocol, ApiProtocol::MessagesV1);
}

#[test]
fn test_adapt_messages_v1_url_from_chat_completions() {
    let adapted = adapt_to_messages_v1_url("http://localhost:8000/v1/chat/completions");
    assert_eq!(adapted, "http://localhost:8000/v1/messages");
}

#[test]
fn test_adapt_messages_v1_url_from_bare_v1() {
    let adapted = adapt_to_messages_v1_url("http://localhost:8000/v1");
    assert_eq!(adapted, "http://localhost:8000/v1/messages");
}

#[test]
fn test_adapt_messages_v1_url_already_correct() {
    let adapted = adapt_to_messages_v1_url("http://localhost:8000/v1/messages");
    assert_eq!(adapted, "http://localhost:8000/v1/messages");
}

#[test]
fn test_adapt_chat_compat_url_already_correct() {
    let adapted = adapt_to_chat_compat_url("http://localhost:8000/v1/chat/completions");
    assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
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

#[tokio::test]
async fn test_live_server_chat_completions_reachable() {
    let url = std::env::var("VEX_TEST_LIVE_SERVER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
    let endpoint = format!("{}/v1/chat/completions", url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("http client");

    let payload = serde_json::json!({
        "model": "test",
        "max_tokens": 4,
        "temperature": 0.0,
        "stream": false,
        "messages": [{"role": "user", "content": "Reply OK"}]
    });

    match client.post(&endpoint).json(&payload).send().await {
        Ok(resp) => {
            assert_ne!(
                resp.status().as_u16(),
                404,
                "connected server returned 404 on native chat/completions endpoint"
            );
        }
        Err(_) => {
            eprintln!(
                "SKIP: server at {} not reachable, skipping connectivity check",
                endpoint
            );
        }
    }
}

#[tokio::test]
async fn test_live_server_messages_v1_reachable() {
    let url = std::env::var("VEX_TEST_LIVE_SERVER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
    let endpoint = format!("{}/v1/messages", url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("http client");

    let payload = serde_json::json!({
        "model": "test",
        "max_tokens": 4,
        "stream": false,
        "system": "Reply OK",
        "messages": [{"role": "user", "content": "OK"}]
    });

    match client.post(&endpoint).json(&payload).send().await {
        Ok(resp) => {
            assert_ne!(
                resp.status().as_u16(),
                404,
                "connected server returned 404 on messages/v1 endpoint"
            );
        }
        Err(_) => {
            eprintln!(
                "SKIP: server at {} not reachable, skipping connectivity check",
                endpoint
            );
        }
    }
}

#[test]
fn test_native_protocol_overrides_configured_protocol() {
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
        "client must use native ChatCompat for bare base URLs when server reports it"
    );
}

#[test]
fn test_explicit_chat_compat_model_url_path_overrides_discovered_messages_protocol() {
    let config = local_stream_test_config(
        "http://localhost:8000/v1/chat/completions".to_string(),
        ModelProtocol::MessagesV1,
    );
    let client = ApiClient::new(&config).expect("client should build");
    client.set_server_info(ServerInfo {
        native_protocol: Some(ModelProtocol::MessagesV1),
        ..ServerInfo::default()
    });

    assert_eq!(client.api_protocol(), ApiProtocol::ChatCompat);
    assert_eq!(
        client.request_url(),
        "http://localhost:8000/v1/chat/completions"
    );
}

#[test]
fn test_local_explicit_messages_model_url_remains_messages_v1_despite_discovery() {
    let config = local_stream_test_config(
        "http://localhost:8000/v1/messages".to_string(),
        ModelProtocol::ChatCompat,
    );
    let client = ApiClient::new(&config).expect("client should build");
    client.set_server_info(ServerInfo {
        native_protocol: Some(ModelProtocol::ChatCompat),
        ..ServerInfo::default()
    });

    assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
}

#[test]
fn test_no_native_protocol_falls_back_to_configured() {
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
            native_protocol: None,
        }))),
        tls_verification_disabled: false,
        #[cfg(test)]
        mock_stream_producer: None,
    };

    assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
}
