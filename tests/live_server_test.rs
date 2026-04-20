//! Live local server integration tests.
//!
//! These tests exercise the API client configuration and protocol detection
//! against a real inference server. They require a running server and are
//! skipped when no server is reachable. No inference calls are made — only
//! the `/v1/models` endpoint is probed.
//!
//! Configuration:
//!   VEX_LIVE_SERVER_URL — base URL of the local server (default: http://localhost:8000)
//!
//! Run with:
//!   VEX_LIVE_SERVER_URL=http://localhost:8000 cargo nextest run -p vexcoder --test live_server_test

use axum::{
    Json, Router,
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use futures::StreamExt;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use vexcoder::batch_mode::{BatchRunOpts, OutputFormat, run_batch};
use vexcoder::config::Config;
use vexcoder::runtime::{
    ModelBackend, ModelBackendKind, ModelProtocol, RuntimeEvent, ToolCallMode, ToolPolicy,
};
use vexcoder::types::{ApiMessage, Content, ModelProfile};

/// Resolve the live server URL from the environment or use the default.
fn live_server_url() -> String {
    std::env::var("VEX_LIVE_SERVER_URL").unwrap_or_else(|_| "http://localhost:8000".to_string())
}

fn single_user_message(text: &str) -> Vec<ApiMessage> {
    vec![ApiMessage {
        role: "user".to_string(),
        content: Content::Text(text.to_string()),
    }]
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

    body.get("data")
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
        .map(|s| s.to_string())
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

fn build_chat_compat_config(base_url: &str, model_name: &str) -> Config {
    Config {
        model_token: None,
        model_name: model_name.to_string(),
        model_url: format!("{}/v1/chat/completions", base_url.trim_end_matches('/')),
        model_url_skip_tls_check: false,
        working_dir: std::env::temp_dir(),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::ChatCompat,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: ModelProfile {
            max_tokens: 256,
            temperature: 0.1,
            ..ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime)
        },
        max_project_instructions_tokens: 0,
        max_memory_tokens: 0,
        sandbox: vexcoder::runtime::SandboxConfig::default(),
        model_headers: HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: vexcoder::config::CompactionConfig::default(),
        undo: vexcoder::config::UndoConfig::default(),
        search: vexcoder::config::SearchConfig::default(),
        notes_path: None,
        api: vexcoder::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: vexcoder::config::AutoMemoryConfig::default(),
        api_client: vexcoder::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
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
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: ModelProfile {
            max_tokens: 256,
            temperature: 0.1,
            ..ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime)
        },
        max_project_instructions_tokens: 0,
        max_memory_tokens: 0,
        sandbox: vexcoder::runtime::SandboxConfig::default(),
        model_headers: HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: vexcoder::config::CompactionConfig::default(),
        undo: vexcoder::config::UndoConfig::default(),
        search: vexcoder::config::SearchConfig::default(),
        notes_path: None,
        api: vexcoder::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: vexcoder::config::AutoMemoryConfig::default(),
        api_client: vexcoder::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    }
}

fn build_auto_detect_batch_config(base_url: &str, model_name: &str) -> Config {
    let mut config = build_messages_v1_config(base_url, model_name);
    config.model_url.clear();
    config.api_client.base_url = base_url.trim_end_matches('/').to_string();
    config.search.auto_index = false;
    config
}

fn fallback_chat_text_response(text: &str) -> Value {
    json!({
        "id": "chatcmpl-batch-fallback",
        "object": "chat.completion",
        "created": 1,
        "model": "stalled-test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": text
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 9,
            "completion_tokens": 3,
            "total_tokens": 12
        }
    })
}

fn fallback_messages_text_response(text: &str) -> Value {
    json!({
        "id": "msg-batch-fallback",
        "type": "message",
        "role": "assistant",
        "model": "stalled-test-model",
        "content": [{
            "type": "text",
            "text": text
        }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 8,
            "output_tokens": 2
        }
    })
}

fn stalled_chat_response() -> Value {
    json!({
        "id": "chatcmpl-stalled-fallback",
        "object": "chat.completion",
        "created": 1,
        "model": "stalled-test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "index": 0,
                    "id": "call_stalled_1",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": {
                            "path": "src/lib.rs"
                        }
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 9,
            "completion_tokens": 3,
            "total_tokens": 12
        }
    })
}

fn stalled_messages_response() -> Value {
    json!({
        "id": "msg-stalled-fallback",
        "type": "message",
        "role": "assistant",
        "model": "stalled-test-model",
        "content": [{
            "type": "tool_use",
            "id": "toolu_stalled_1",
            "name": "read_file",
            "input": {
                "path": "src/lib.rs"
            }
        }],
        "stop_reason": "tool_use",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 9,
            "output_tokens": 3
        }
    })
}

fn stalled_sse_response() -> Response {
    Sse::new(futures::stream::pending::<Result<Event, Infallible>>()).into_response()
}

fn stream_requested(payload: &Value) -> bool {
    payload.get("stream").and_then(Value::as_bool) == Some(true)
}

async fn stalled_chat_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return stalled_sse_response();
    }

    Json(stalled_chat_response()).into_response()
}

async fn stalled_messages_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return stalled_sse_response();
    }

    Json(stalled_messages_response()).into_response()
}

async fn spawn_stalled_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("stalled test server listener should bind");
    let addr = listener
        .local_addr()
        .expect("stalled test server should expose a local address");

    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(stalled_chat_handler))
                .route("/v1/messages", post(stalled_messages_handler)),
        )
        .await
        .expect("stalled test server should stay alive until aborted");
    });

    (format!("http://{addr}"), server)
}

async fn probe_messages_handler() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        "event: ping\ndata: {\"type\":\"ping\"}\n\n",
    )
}

async fn probe_chat_handler() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        "data: {\"choices\":[]}\n\n",
    )
}

async fn missing_probe_handler() -> impl IntoResponse {
    axum::http::StatusCode::NOT_FOUND
}

async fn stalled_chat_text_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return stalled_sse_response();
    }

    Json(fallback_chat_text_response("chat-compat batch output")).into_response()
}

async fn stalled_messages_text_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return stalled_sse_response();
    }

    Json(fallback_messages_text_response("messages-v1 batch output")).into_response()
}

async fn spawn_batch_chat_compat_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("batch chat-compat test server listener should bind");
    let addr = listener
        .local_addr()
        .expect("batch chat-compat test server should expose a local address");

    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", get(missing_probe_handler))
                .route(
                    "/v1/chat/completions",
                    get(probe_chat_handler).post(stalled_chat_text_handler),
                ),
        )
        .await
        .expect("batch chat-compat test server should stay alive until aborted");
    });

    (format!("http://{addr}"), server)
}

async fn spawn_batch_messages_v1_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("batch messages-v1 test server listener should bind");
    let addr = listener
        .local_addr()
        .expect("batch messages-v1 test server should expose a local address");

    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route(
                    "/v1/messages",
                    get(probe_messages_handler).post(stalled_messages_text_handler),
                )
                .route("/v1/chat/completions", get(missing_probe_handler)),
        )
        .await
        .expect("batch messages-v1 test server should stay alive until aborted");
    });

    (format!("http://{addr}"), server)
}

fn assert_batch_jsonl_response(output_lines: &[String], expected_response: &str) {
    assert_eq!(
        output_lines.len(),
        2,
        "expected one turn record and one summary"
    );

    let turn_record: Value =
        serde_json::from_str(&output_lines[0]).expect("turn record should be valid JSON");
    assert_eq!(
        turn_record.get("response").and_then(Value::as_str),
        Some(expected_response)
    );
    assert_eq!(turn_record.get("turn").and_then(Value::as_u64), Some(1));

    let summary_record: Value =
        serde_json::from_str(&output_lines[1]).expect("summary record should be valid JSON");
    assert_eq!(
        summary_record.get("summary").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        summary_record.get("status").and_then(Value::as_str),
        Some("Completed")
    );
    assert_eq!(
        summary_record.get("total_turns").and_then(Value::as_u64),
        Some(1)
    );
}

// ---------------------------------------------------------------------------
// Tests — server probe
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

// ---------------------------------------------------------------------------
// Tests — ChatCompat (/v1/chat/completions) config and protocol
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_chat_compat_config_builds_valid_api_client() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_chat_compat_config(&base_url, &model);
    let result = vexcoder::api::ApiClient::new(&config);
    assert!(
        result.is_ok(),
        "ApiClient::new must succeed for chat-compat config: {:?}",
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
    assert_eq!(
        client.protocol(),
        ModelProtocol::ChatCompat,
        "client must use ChatCompat protocol"
    );
}

// ---------------------------------------------------------------------------
// Tests — MessagesV1 (/v1/messages) config and protocol
// ---------------------------------------------------------------------------

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
    assert!(
        client.https_local_startup_warning().is_none(),
        "plain HTTP messages-v1 server must not trigger HTTPS warning"
    );
    assert_eq!(
        client.protocol(),
        ModelProtocol::MessagesV1,
        "client must use MessagesV1 protocol"
    );
}

// ---------------------------------------------------------------------------
// Tests — protocol detection across both wire formats
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_chat_compat_and_messages_v1_use_different_protocols() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let chat_config = build_chat_compat_config(&base_url, &model);
    let msg_config = build_messages_v1_config(&base_url, &model);

    let chat_client = vexcoder::api::ApiClient::new(&chat_config).unwrap();
    let msg_client = vexcoder::api::ApiClient::new(&msg_config).unwrap();

    assert_eq!(chat_client.protocol(), ModelProtocol::ChatCompat);
    assert_eq!(msg_client.protocol(), ModelProtocol::MessagesV1);
}

#[tokio::test]
async fn test_messages_v1_url_resolves_correctly() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_messages_v1_config(&base_url, &model);
    let expected_url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    assert_eq!(config.model_url, expected_url);
    assert_eq!(config.model_protocol, ModelProtocol::MessagesV1);
}

#[tokio::test]
async fn test_chat_compat_url_resolves_correctly() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_chat_compat_config(&base_url, &model);
    let expected_url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    assert_eq!(config.model_url, expected_url);
    assert_eq!(config.model_protocol, ModelProtocol::ChatCompat);
}

#[tokio::test]
async fn test_stalled_stream_chat_compat_falls_back_to_non_stream_json() {
    let (base_url, server) = spawn_stalled_server().await;
    let config = build_chat_compat_config(&base_url, "stalled-test-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    let started = Instant::now();

    let mut stream = client
        .create_stream(&single_user_message("Inspect src/lib.rs."))
        .await
        .expect("stalled chat-compat stream should fall back to non-stream JSON");

    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_secs(5),
        "expected the initial SSE timeout-driven fallback, got elapsed={elapsed:?}"
    );

    let mut envelopes = Vec::new();
    while let Some(event) = stream.next().await {
        envelopes.push(event.expect("runtime envelope"));
    }

    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TranscriptBlockStart {
            block: vexcoder::state::StreamBlock::ToolCall { id, name, input, .. },
            ..
        } if id == "call_stalled_1"
            && name == "read_file"
            && input == &serde_json::json!({"path":"src/lib.rs"})
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::ToolCallStarted { tool_call_id, tool_name, arguments, .. }
            if tool_call_id.starts_with("tx_")
                && tool_name == "read_file"
                && arguments == &serde_json::json!({"path":"src/lib.rs"})
    )));
    assert!(
        !envelopes
            .iter()
            .any(|envelope| matches!(&envelope.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized stalled-fallback tool calls should not emit argument deltas",
    );
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::UsageUpdated { usage } if usage.input == 9 && usage.output == 3
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TurnEnd { status, usage: Some(usage), .. }
            if status == "completed" && usage.input == 9 && usage.output == 3
    )));

    server.abort();
}

#[tokio::test]
async fn test_stalled_stream_messages_v1_falls_back_to_non_stream_json() {
    let (base_url, server) = spawn_stalled_server().await;
    let config = build_messages_v1_config(&base_url, "stalled-test-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    let started = Instant::now();

    let mut stream = client
        .create_stream(&single_user_message("Inspect src/lib.rs."))
        .await
        .expect("stalled MessagesV1 stream should fall back to non-stream JSON");

    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_secs(5),
        "expected the initial SSE timeout-driven fallback, got elapsed={elapsed:?}"
    );

    let mut envelopes = Vec::new();
    while let Some(event) = stream.next().await {
        envelopes.push(event.expect("runtime envelope"));
    }

    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TranscriptBlockStart {
            block: vexcoder::state::StreamBlock::ToolCall { id, name, input, .. },
            ..
        } if id == "toolu_stalled_1"
            && name == "read_file"
            && input == &serde_json::json!({"path":"src/lib.rs"})
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::ToolCallStarted { tool_call_id, tool_name, arguments, .. }
            if tool_call_id.starts_with("tx_")
                && tool_name == "read_file"
                && arguments == &serde_json::json!({"path":"src/lib.rs"})
    )));
    assert!(
        !envelopes
            .iter()
            .any(|envelope| matches!(&envelope.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized stalled-fallback tool calls should not emit argument deltas",
    );
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::UsageUpdated { usage } if usage.input == 9 && usage.output == 3
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TurnEnd { status, usage: Some(usage), .. }
            if status == "completed" && usage.input == 9 && usage.output == 3
    )));

    server.abort();
}

#[tokio::test]
async fn test_run_batch_auto_detects_chat_compat_and_preserves_fallback_text_output() {
    let (base_url, server) = spawn_batch_chat_compat_server().await;
    let config = build_auto_detect_batch_config(&base_url, "stalled-test-model");

    let result = run_batch(
        "Reply with exactly the fallback text.".to_string(),
        BatchRunOpts {
            max_turns: Some(1),
            format: OutputFormat::Text,
            ..Default::default()
        },
        &config,
    )
    .await
    .expect("batch run should succeed against auto-detected chat-compat server");

    assert_eq!(
        result.output_lines,
        vec!["chat-compat batch output".to_string()]
    );

    server.abort();
}

#[tokio::test]
async fn test_run_batch_auto_detects_messages_v1_and_preserves_fallback_text_output() {
    let (base_url, server) = spawn_batch_messages_v1_server().await;
    let config = build_auto_detect_batch_config(&base_url, "stalled-test-model");

    let result = run_batch(
        "Reply with exactly the fallback text.".to_string(),
        BatchRunOpts {
            max_turns: Some(1),
            format: OutputFormat::Text,
            ..Default::default()
        },
        &config,
    )
    .await
    .expect("batch run should succeed against auto-detected messages-v1 server");

    assert_eq!(
        result.output_lines,
        vec!["messages-v1 batch output".to_string()]
    );

    server.abort();
}

#[tokio::test]
async fn test_run_batch_auto_detects_chat_compat_and_preserves_fallback_jsonl_output() {
    let (base_url, server) = spawn_batch_chat_compat_server().await;
    let config = build_auto_detect_batch_config(&base_url, "stalled-test-model");

    let result = run_batch(
        "Reply with exactly the fallback text.".to_string(),
        BatchRunOpts {
            max_turns: Some(1),
            format: OutputFormat::Jsonl,
            ..Default::default()
        },
        &config,
    )
    .await
    .expect("batch run should succeed against auto-detected chat-compat server");

    assert_batch_jsonl_response(&result.output_lines, "chat-compat batch output");

    server.abort();
}

#[tokio::test]
async fn test_run_batch_auto_detects_messages_v1_and_preserves_fallback_jsonl_output() {
    let (base_url, server) = spawn_batch_messages_v1_server().await;
    let config = build_auto_detect_batch_config(&base_url, "stalled-test-model");

    let result = run_batch(
        "Reply with exactly the fallback text.".to_string(),
        BatchRunOpts {
            max_turns: Some(1),
            format: OutputFormat::Jsonl,
            ..Default::default()
        },
        &config,
    )
    .await
    .expect("batch run should succeed against auto-detected messages-v1 server");

    assert_batch_jsonl_response(&result.output_lines, "messages-v1 batch output");

    server.abort();
}
