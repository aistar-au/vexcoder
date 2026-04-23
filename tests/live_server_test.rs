use axum::{
    Json, Router,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures::StreamExt;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use vexcoder::batch_mode::{BatchRunOpts, OutputFormat, run_batch};
use vexcoder::config::Config;
use vexcoder::runtime::{
    ModelBackend, ModelBackendKind, ModelProtocol, RuntimeEvent, ToolCallMode, ToolPolicy,
};
use vexcoder::types::{ApiMessage, Content, ModelProfile};

#[allow(unused)]
mod test_support {
    pub struct EnvLock(tokio::sync::Mutex<()>);
    impl EnvLock {
        pub const fn new() -> Self {
            Self(tokio::sync::Mutex::const_new(()))
        }
        pub fn blocking_lock(&self) -> EnvLockGuard<'_> {
            EnvLockGuard(self.0.blocking_lock())
        }
        pub async fn lock(&self) -> EnvLockGuard<'_> {
            EnvLockGuard(self.0.lock().await)
        }
    }
    pub struct EnvLockGuard<'a>(tokio::sync::MutexGuard<'a, ()>);
    impl EnvLockGuard<'_> {
        #[allow(unsafe_code)]
        pub fn set_var(&self, key: &str, val: impl AsRef<std::ffi::OsStr>) {
            // SAFETY: the guard proves exclusive ownership of ENV_LOCK.
            unsafe { std::env::set_var(key, val) }
        }
        #[allow(unsafe_code)]
        pub fn remove_var(&self, key: &str) {
            // SAFETY: the guard proves exclusive ownership of ENV_LOCK.
            unsafe { std::env::remove_var(key) }
        }
    }
    pub struct EnvRestore<'a> {
        _guard: &'a EnvLockGuard<'a>,
        key: &'static str,
        value: Option<String>,
    }
    impl<'a> EnvRestore<'a> {
        pub fn capture(guard: &'a EnvLockGuard<'a>, key: &'static str) -> Self {
            Self {
                _guard: guard,
                key,
                value: std::env::var(key).ok(),
            }
        }
    }
    impl Drop for EnvRestore<'_> {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            match &self.value {
                // SAFETY: EnvRestore cannot outlive the EnvLockGuard it was created from.
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }
    pub static ENV_LOCK: EnvLock = EnvLock::new();
}

fn live_server_url() -> String {
    std::env::var("VEX_LIVE_SERVER_URL").unwrap_or_else(|_| "http://localhost:8000".to_string())
}

fn single_user_message(text: &str) -> Vec<ApiMessage> {
    vec![ApiMessage {
        role: "user".to_string(),
        content: Content::Text(text.to_string()),
        cache_hint: None,
    }]
}

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

#[path = "live_server_test/logging.rs"]
mod logging;
#[path = "live_server_test/protocol.rs"]
mod protocol;
#[path = "live_server_test/tool_calls.rs"]
mod tool_calls;

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

fn no_initial_sse_response() -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        "",
    )
        .into_response()
}

fn stream_requested(payload: &Value) -> bool {
    payload.get("stream").and_then(Value::as_bool) == Some(true)
}

async fn stalled_chat_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }

    Json(stalled_chat_response()).into_response()
}

async fn stalled_messages_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
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
        return no_initial_sse_response();
    }

    Json(fallback_chat_text_response("chat-compat batch output")).into_response()
}

async fn stalled_messages_text_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
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
        elapsed < Duration::from_secs(2),
        "expected a prompt no-initial-event fallback instead of the long startup timeout; elapsed={elapsed:?}"
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
        elapsed < Duration::from_secs(2),
        "expected a prompt no-initial-event fallback instead of the long startup timeout; elapsed={elapsed:?}"
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

fn empty_string_content_chat_response() -> Value {
    json!({
        "id": "chatcmpl-empty-str",
        "object": "chat.completion",
        "created": 1,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": ""
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 5,
            "completion_tokens": 1,
            "total_tokens": 6
        }
    })
}

fn null_content_no_tools_chat_response() -> Value {
    json!({
        "id": "chatcmpl-null-no-tools",
        "object": "chat.completion",
        "created": 1,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 5,
            "completion_tokens": 0,
            "total_tokens": 5
        }
    })
}

fn tool_calls_json_arguments_chat_response() -> Value {
    json!({
        "id": "chatcmpl-json-args",
        "object": "chat.completion",
        "created": 1,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "index": 0,
                    "id": "call_json_args_1",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": {
                            "path": "src/main.rs",
                            "offset": 1,
                            "limit": 50
                        }
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    })
}

fn tool_calls_messages_v1_response() -> Value {
    json!({
        "id": "msg-tool-fallback",
        "type": "message",
        "role": "assistant",
        "model": "test-model",
        "content": [{
            "type": "tool_use",
            "id": "toolu_auto_1",
            "name": "read_file",
            "input": {
                "path": "src/lib.rs"
            }
        }],
        "stop_reason": "tool_use",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5
        }
    })
}

async fn empty_string_content_chat_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(empty_string_content_chat_response()).into_response()
}

async fn null_content_no_tools_chat_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(null_content_no_tools_chat_response()).into_response()
}

async fn tool_calls_json_args_chat_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(tool_calls_json_arguments_chat_response()).into_response()
}

async fn tool_calls_messages_v1_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(tool_calls_messages_v1_response()).into_response()
}

async fn slow_non_stream_handler(_payload: Json<Value>) -> Response {
    tokio::time::sleep(Duration::from_millis(400)).await;
    Json(json!({"error": "too slow"})).into_response()
}

async fn spawn_empty_string_content_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("empty-content test server should bind");
    let addr = listener.local_addr().expect("must have local address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/v1/chat/completions",
                post(empty_string_content_chat_handler),
            ),
        )
        .await
        .expect("empty-content server should stay alive until aborted");
    });
    (format!("http://{addr}"), server)
}

async fn spawn_null_content_no_tools_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("null-content test server should bind");
    let addr = listener.local_addr().expect("must have local address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/v1/chat/completions",
                post(null_content_no_tools_chat_handler),
            ),
        )
        .await
        .expect("null-content server should stay alive until aborted");
    });
    (format!("http://{addr}"), server)
}

async fn spawn_slow_fallback_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("slow-fallback test server should bind");
    let addr = listener.local_addr().expect("must have local address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(slow_non_stream_handler))
                .route("/v1/messages", post(slow_non_stream_handler)),
        )
        .await
        .expect("slow-fallback server should stay alive until aborted");
    });
    (format!("http://{addr}"), server)
}

async fn spawn_tool_calls_json_args_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("json-args test server should bind");
    let addr = listener.local_addr().expect("must have local address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", get(missing_probe_handler))
                .route(
                    "/v1/chat/completions",
                    get(probe_chat_handler).post(tool_calls_json_args_chat_handler),
                ),
        )
        .await
        .expect("json-args server should stay alive until aborted");
    });
    (format!("http://{addr}"), server)
}

async fn spawn_auto_detect_messages_v1_tool_calls_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("messages-v1 tool-calls test server should bind");
    let addr = listener.local_addr().expect("must have local address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route(
                    "/v1/messages",
                    get(probe_messages_handler).post(tool_calls_messages_v1_handler),
                )
                .route("/v1/chat/completions", get(missing_probe_handler)),
        )
        .await
        .expect("messages-v1 tool-calls server should stay alive until aborted");
    });
    (format!("http://{addr}"), server)
}

#[tokio::test]
async fn test_local_fallback_post_slow_response_surfaces_error() {
    let (base_url, server) = spawn_slow_fallback_server().await;
    let config = build_chat_compat_config(&base_url, "timeout-test-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");

    let started = Instant::now();
    let result = client.create_stream(&single_user_message("test")).await;
    let elapsed = started.elapsed();

    assert!(
        result.is_err(),
        "slow fallback POST must surface an error instead of a bogus success result"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "slow fallback path should fail promptly instead of hanging; elapsed={elapsed:?}"
    );

    server.abort();
}

#[tokio::test]
async fn test_stalled_stream_chat_compat_empty_string_content_produces_turn_end() {
    let (base_url, server) = spawn_empty_string_content_server().await;
    let config = build_chat_compat_config(&base_url, "empty-content-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");

    let mut stream = client
        .create_stream(&single_user_message("Say nothing."))
        .await
        .expect("empty-content fallback must not error");

    let mut envelopes = Vec::new();
    while let Some(envelope) = stream.next().await {
        envelopes.push(envelope.expect("envelope must not error"));
    }

    assert!(
        envelopes
            .iter()
            .any(|e| matches!(&e.event, RuntimeEvent::TurnEnd { .. })),
        "empty-string content fallback must emit TurnEnd; got {:?}",
        envelopes.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert!(
        !envelopes
            .iter()
            .any(|e| matches!(&e.event, RuntimeEvent::ToolCallStarted { .. })),
        "empty-content fallback must not emit ToolCallStarted"
    );

    server.abort();
}

#[tokio::test]
async fn test_stalled_stream_chat_compat_null_content_no_tools_produces_turn_end() {
    let (base_url, server) = spawn_null_content_no_tools_server().await;
    let config = build_chat_compat_config(&base_url, "null-no-tools-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");

    let mut stream = client
        .create_stream(&single_user_message("Say nothing."))
        .await
        .expect("null-no-tools fallback must not error");

    let mut envelopes = Vec::new();
    while let Some(envelope) = stream.next().await {
        envelopes.push(envelope.expect("envelope must not error"));
    }

    assert!(
        envelopes
            .iter()
            .any(|e| matches!(&e.event, RuntimeEvent::TurnEnd { .. })),
        "null-no-tools fallback must emit TurnEnd; got {:?}",
        envelopes.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert!(
        !envelopes
            .iter()
            .any(|e| matches!(&e.event, RuntimeEvent::ToolCallStarted { .. })),
        "null-no-tools fallback must not emit ToolCallStarted"
    );

    server.abort();
}
