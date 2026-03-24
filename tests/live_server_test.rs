//! Live local server integration tests.
//!
//! These tests exercise the API client against a real inference server.
//! They require a running server and are skipped when no server is reachable.
//!
//! Configuration:
//!   VEX_LIVE_SERVER_URL — base URL of the local server (default: http://localhost:8000)
//!
//! Run with:
//!   VEX_LIVE_SERVER_URL=http://localhost:8000 cargo nextest run -p vexcoder --test live_server_test

use reqwest::header::HeaderMap;
use std::time::Duration;
use vexcoder::config::Config;
use vexcoder::runtime::{ModelBackendKind, ModelProtocol, ToolCallMode};
use vexcoder::types::ModelProfile;

/// Resolve the live server URL from the environment or use the default.
fn live_server_url() -> String {
    std::env::var("VEX_LIVE_SERVER_URL").unwrap_or_else(|_| "http://localhost:8000".to_string())
}

/// Check whether the live server is reachable. Returns the model name if
/// available, or None if the server is unreachable.
async fn probe_server(base_url: &str) -> Option<String> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    // Support both standard {"data":[{"id":"..."}]} and local-runtime {"models":[{"model":"..."}]} formats
    let model_name = body
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|arr| arr.first())
        .and_then(|m| m.get("id"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("models")
                .and_then(|m| m.as_array())
                .and_then(|arr| arr.first())
                .and_then(|m| m.get("model"))
                .and_then(|v| v.as_str())
        })
        .map(|s| s.to_string());
    model_name
}

/// Helper macro: skip the test with an overlay-style notice when no server
/// is available instead of failing.
macro_rules! require_live_server {
    ($base_url:expr) => {
        match probe_server($base_url).await {
            Some(model) => {
                eprintln!(
                    "[live-server: connected to {} — model: {}]",
                    $base_url, model
                );
                model
            }
            None => {
                eprintln!(
                    "[live-server: connection refused on {} — skipping test, set VEX_LIVE_SERVER_URL to override]",
                    $base_url
                );
                return;
            }
        }
    };
}

fn build_live_config(base_url: &str, model_name: &str) -> Config {
    Config {
        model_token: None,
        model_name: model_name.to_string(),
        model_url: format!("{}/v1/chat/completions", base_url.trim_end_matches('/')),
        model_url_skip_tls_check: false,
        working_dir: std::env::temp_dir(),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::ChatCompat,
        tool_call_mode: ToolCallMode::TaggedFallback,
        model_profile: ModelProfile {
            max_tokens: 256,
            temperature: 0.1,
            ..ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime)
        },
        max_project_instructions_tokens: 0,
        max_memory_tokens: 0,
        model_headers: HeaderMap::new(),
        notes_path: None,
        api: vexcoder::config::ApiConfig::default(),
        hooks: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_live_server_model_listing() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);
    assert!(
        !model.is_empty(),
        "model listing must return at least one model name"
    );
}

#[tokio::test]
async fn test_live_server_chat_completion_returns_response() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("http client");

    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": "Reply with exactly: PONG"}
        ],
        "max_tokens": 32,
        "temperature": 0.0,
        "stream": false
    });

    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .expect("chat completion request");

    assert!(
        resp.status().is_success(),
        "chat completion must return 2xx, got {}",
        resp.status()
    );

    let body: serde_json::Value = resp.json().await.expect("json response");
    let content = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        !content.is_empty(),
        "chat completion must return non-empty content"
    );
}

#[tokio::test]
async fn test_live_server_streaming_chat_returns_deltas() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("http client");

    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": "Count from 1 to 5, one number per line."}
        ],
        "max_tokens": 64,
        "temperature": 0.0,
        "stream": true
    });

    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .expect("streaming request");

    assert!(
        resp.status().is_success(),
        "streaming request must return 2xx, got {}",
        resp.status()
    );

    let body_text = resp.text().await.expect("stream body");
    let data_lines: Vec<&str> = body_text
        .lines()
        .filter(|l| l.starts_with("data: ") && !l.contains("[DONE]"))
        .collect();
    assert!(
        !data_lines.is_empty(),
        "streaming response must contain at least one SSE data line"
    );

    // Verify at least one chunk has delta content.
    let has_content = data_lines.iter().any(|line| {
        let json_str = line.trim_start_matches("data: ");
        serde_json::from_str::<serde_json::Value>(json_str)
            .ok()
            .and_then(|v| {
                v.get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|c| c.get("delta"))
                    .and_then(|d| d.get("content"))
                    .and_then(|c| c.as_str())
                    .map(|s| !s.is_empty())
            })
            .unwrap_or(false)
    });
    assert!(
        has_content,
        "streaming response must contain content deltas"
    );
}

#[tokio::test]
async fn test_live_server_config_builds_valid_api_client() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_live_config(&base_url, &model);
    let result = vexcoder::api::ApiClient::new(&config);
    assert!(
        result.is_ok(),
        "ApiClient::new must succeed for local server config: {:?}",
        result.err()
    );

    let client = result.unwrap();
    assert!(
        client.is_local_endpoint(),
        "live server URL must be detected as local endpoint"
    );
    assert!(
        client.https_local_startup_warning().is_none(),
        "plain HTTP local server must not trigger HTTPS warning"
    );
}

#[tokio::test]
async fn test_live_server_handles_empty_message_gracefully() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("http client");

    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": ""}
        ],
        "max_tokens": 16,
        "temperature": 0.0,
        "stream": false
    });

    let resp = client.post(&url).json(&payload).send().await;
    // The server should either return a valid response or a clean error,
    // never hang or crash.
    assert!(
        resp.is_ok(),
        "server must respond to empty-content message without hanging"
    );
}
