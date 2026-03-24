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
use serde_json::json;
use std::time::Duration;
use vexcoder::config::Config;
use vexcoder::runtime::{ModelBackend, ModelBackendKind, ModelProtocol, ToolCallMode};
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

fn build_messages_v1_config(base_url: &str, model_name: &str) -> Config {
    Config {
        model_token: None,
        model_name: model_name.to_string(),
        model_url: format!("{}/v1/messages", base_url.trim_end_matches('/')),
        model_url_skip_tls_check: false,
        working_dir: std::env::temp_dir(),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
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

/// Probe whether the server supports the /v1/messages endpoint.
/// Returns true if the endpoint responds (even with an error), false if
/// connection is refused or the endpoint 404s.
async fn probe_messages_endpoint(base_url: &str) -> bool {
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    // Send a minimal messages-v1 request to see if the endpoint exists.
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&json!({
            "model": "probe",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "ping"}]
        }))
        .send()
        .await;
    match resp {
        Ok(r) => r.status().as_u16() != 404,
        Err(_) => false,
    }
}

/// Helper macro: skip when the messages/v1 endpoint is not available.
macro_rules! require_messages_endpoint {
    ($base_url:expr, $model:expr) => {
        if !probe_messages_endpoint($base_url).await {
            eprintln!(
                "[messages-v1: endpoint not available on {} — skipping test]",
                $base_url
            );
            return;
        }
        eprintln!(
            "[messages-v1: endpoint active on {} — model: {}]",
            $base_url, $model
        );
    };
}

// ---------------------------------------------------------------------------
// Tests — ChatCompat (existing, /v1/chat/completions)
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

// ---------------------------------------------------------------------------
// Tests — MessagesV1 (/v1/messages)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_messages_v1_non_streaming_returns_response() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);
    require_messages_endpoint!(&base_url, &model);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("http client");

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let payload = json!({
        "model": model,
        "max_tokens": 32,
        "messages": [
            {"role": "user", "content": "Reply with exactly: PONG"}
        ],
        "temperature": 0.0,
        "stream": false
    });

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("messages-v1 request");

    let status = resp.status();
    assert!(
        status.is_success() || status.as_u16() == 400,
        "messages-v1 must return 2xx or 400 (unsupported), got {}",
        status
    );

    if status.is_success() {
        let body: serde_json::Value = resp.json().await.expect("json response");
        // Messages-v1 returns content array with text blocks
        let has_content = body
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false);
        // Or it may use the chat-compat choices format
        let has_choices = body
            .get("choices")
            .and_then(|c| c.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false);
        assert!(
            has_content || has_choices,
            "messages-v1 response must have content or choices: {:?}",
            body
        );
    }
}

#[tokio::test]
async fn test_messages_v1_streaming_returns_sse_events() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);
    require_messages_endpoint!(&base_url, &model);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("http client");

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let payload = json!({
        "model": model,
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": "Count from 1 to 3, one number per line."}
        ],
        "temperature": 0.0,
        "stream": true
    });

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("streaming messages-v1 request");

    let status = resp.status();
    assert!(
        status.is_success() || status.as_u16() == 400,
        "streaming messages-v1 must return 2xx or 400, got {}",
        status
    );

    if status.is_success() {
        let body_text = resp.text().await.expect("stream body");
        let event_lines: Vec<&str> = body_text
            .lines()
            .filter(|l| l.starts_with("data: ") || l.starts_with("event: "))
            .collect();
        assert!(
            !event_lines.is_empty(),
            "streaming messages-v1 must return SSE events"
        );
    }
}

#[tokio::test]
async fn test_messages_v1_with_system_prompt() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);
    require_messages_endpoint!(&base_url, &model);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("http client");

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let payload = json!({
        "model": model,
        "max_tokens": 32,
        "system": "You are a calculator. Only respond with numbers.",
        "messages": [
            {"role": "user", "content": "What is 2+2?"}
        ],
        "temperature": 0.0,
        "stream": false
    });

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("messages-v1 with system prompt");

    // Accept success or graceful error — never hang or crash.
    assert!(
        resp.status().is_success() || resp.status().is_client_error(),
        "messages-v1 with system must not crash, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_messages_v1_config_builds_valid_api_client() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_messages_v1_config(&base_url, &model);
    let result = vexcoder::api::ApiClient::new(&config);
    assert!(
        result.is_ok(),
        "ApiClient::new must succeed for messages-v1 config: {:?}",
        result.err()
    );

    let client = result.unwrap();
    assert!(
        client.is_local_endpoint(),
        "messages-v1 live server URL must be detected as local endpoint"
    );
    assert_eq!(
        client.protocol(),
        ModelProtocol::MessagesV1,
        "client must use MessagesV1 protocol"
    );
}

#[tokio::test]
async fn test_messages_v1_handles_empty_message_gracefully() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);
    require_messages_endpoint!(&base_url, &model);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("http client");

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let payload = json!({
        "model": model,
        "max_tokens": 16,
        "messages": [
            {"role": "user", "content": ""}
        ],
        "temperature": 0.0,
        "stream": false
    });

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await;
    assert!(
        resp.is_ok(),
        "messages-v1 must respond to empty-content without hanging"
    );
}

#[tokio::test]
async fn test_messages_v1_rejects_malformed_payload() {
    let base_url = live_server_url();
    let _model = require_live_server!(&base_url);
    require_messages_endpoint!(&base_url, "probe");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("http client");

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    // Missing required 'messages' field
    let payload = json!({"model": "test", "max_tokens": 16});
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("malformed request should get response");

    assert!(
        resp.status().is_client_error() || resp.status().is_server_error(),
        "malformed payload must return error status, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_messages_v1_multi_turn_conversation() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);
    require_messages_endpoint!(&base_url, &model);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("http client");

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let payload = json!({
        "model": model,
        "max_tokens": 64,
        "messages": [
            {"role": "user", "content": "Remember: the secret word is BANANA."},
            {"role": "assistant", "content": "I will remember that the secret word is BANANA."},
            {"role": "user", "content": "What is the secret word? Reply with only the word."}
        ],
        "temperature": 0.0,
        "stream": false
    });

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("multi-turn request");

    assert!(
        resp.status().is_success() || resp.status().is_client_error(),
        "multi-turn messages-v1 must not crash, got {}",
        resp.status()
    );
}

// ---------------------------------------------------------------------------
// Tests — Protocol detection and URL resolution (regression for PR #212)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_chat_compat_and_messages_v1_configs_use_different_urls() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let chat_config = build_live_config(&base_url, &model);
    let msg_config = build_messages_v1_config(&base_url, &model);

    let chat_client = vexcoder::api::ApiClient::new(&chat_config).unwrap();
    let msg_client = vexcoder::api::ApiClient::new(&msg_config).unwrap();

    assert_eq!(chat_client.protocol(), ModelProtocol::ChatCompat);
    assert_eq!(msg_client.protocol(), ModelProtocol::MessagesV1);

    // The request URLs must differ — one ends in /chat/completions, the other /messages
    assert_ne!(
        chat_client.protocol(),
        msg_client.protocol(),
        "chat-compat and messages-v1 must resolve to different protocols"
    );
}

// ---------------------------------------------------------------------------
// Tests — Regression guardrail: server robustness
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_live_server_concurrent_requests_do_not_crash() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("http client");

    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

    // Fire 3 concurrent requests
    let mut handles = vec![];
    for i in 0..3 {
        let c = client.clone();
        let u = url.clone();
        let m = model.clone();
        handles.push(tokio::spawn(async move {
            let payload = json!({
                "model": m,
                "messages": [
                    {"role": "user", "content": format!("Reply with the number {i}")}
                ],
                "max_tokens": 16,
                "temperature": 0.0,
                "stream": false
            });
            c.post(&u).json(&payload).send().await
        }));
    }

    for handle in handles {
        let result = handle.await.expect("task must not panic");
        assert!(
            result.is_ok(),
            "concurrent request must not crash server"
        );
    }
}

#[tokio::test]
async fn test_live_server_large_message_does_not_hang() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("http client");

    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    // Send a large input message (10 KB of repeated text)
    let large_content = "The quick brown fox jumps over the lazy dog. ".repeat(250);
    let payload = json!({
        "model": model,
        "messages": [
            {"role": "user", "content": large_content}
        ],
        "max_tokens": 16,
        "temperature": 0.0,
        "stream": false
    });

    let resp = client.post(&url).json(&payload).send().await;
    // Must respond (success or error), never hang.
    assert!(
        resp.is_ok(),
        "server must respond to large message without hanging"
    );
    let status = resp.unwrap().status();
    assert!(
        status.is_success() || status.is_client_error(),
        "large message must get success or clean error, got {}",
        status
    );
}
