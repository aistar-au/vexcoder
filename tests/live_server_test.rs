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
    ModelBackend, ModelBackendKind, ModelProtocol, RuntimeSignal, ToolCallMode, ToolPolicy,
};
use vexcoder::types::{ApiMessage, Content, ModelProfile};

mod test_support {
    pub struct EnvLock(tokio::sync::Mutex<()>);
    impl EnvLock {
        pub const fn new() -> Self {
            Self(tokio::sync::Mutex::const_new(()))
        }
        pub async fn lock(&self) -> EnvLockGuard<'_> {
            EnvLockGuard {
                _guard: self.0.lock().await,
            }
        }
    }
    pub struct EnvLockGuard<'a> {
        _guard: tokio::sync::MutexGuard<'a, ()>,
    }
    impl EnvLockGuard<'_> {
        #[allow(unsafe_code)]
        pub fn set_var(&self, key: &str, val: impl AsRef<std::ffi::OsStr>) {
            unsafe { std::env::set_var(key, val) }
        }
        #[allow(unsafe_code)]
        pub fn remove_var(&self, key: &str) {
            unsafe { std::env::remove_var(key) }
        }
    }
    pub struct EnvRestore<'a> {
        _guard: &'a EnvLockGuard<'a>,
        key: &'static str,
        value: Option<std::ffi::OsString>,
    }
    impl<'a> EnvRestore<'a> {
        pub fn capture(guard: &'a EnvLockGuard<'a>, key: &'static str) -> Self {
            Self {
                _guard: guard,
                key,
                value: std::env::var_os(key),
            }
        }
    }
    impl Drop for EnvRestore<'_> {
        fn drop(&mut self) {
            match &self.value {
                Some(value) => self._guard.set_var(self.key, value),
                None => self._guard.remove_var(self.key),
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
            Some(model) => { eprintln!("[live-server: connected to {} — model: {}]", $base_url, model); model }
            None => { eprintln!("[live-server: connection refused on {} — skipping test, set VEX_LIVE_SERVER_URL to override]", $base_url); return; }
        }
    };
}

#[path = "live_server_test/logging.rs"]
mod logging;
#[path = "live_server_test/protocol.rs"]
mod protocol;
#[path = "live_server_test/tool_calls.rs"]
mod tool_calls;

fn make_config(
    base_url: &str,
    model_name: &str,
    endpoint: &str,
    protocol: ModelProtocol,
) -> Config {
    Config {
        model_token: None,
        model_name: model_name.to_string(),
        model_url: format!("{}/{}", base_url.trim_end_matches('/'), endpoint),
        model_url_skip_tls_check: false,
        working_dir: std::env::temp_dir(),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: protocol,
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

pub(crate) fn build_chat_compat_config(base_url: &str, model_name: &str) -> Config {
    make_config(
        base_url,
        model_name,
        "v1/chat/completions",
        ModelProtocol::ChatCompat,
    )
}

pub(crate) fn build_messages_v1_config(base_url: &str, model_name: &str) -> Config {
    make_config(
        base_url,
        model_name,
        "v1/messages",
        ModelProtocol::MessagesV1,
    )
}

pub(crate) fn build_auto_detect_batch_config(base_url: &str, model_name: &str) -> Config {
    let mut config = build_messages_v1_config(base_url, model_name);
    config.model_url.clear();
    config.api_client.base_url = base_url.trim_end_matches('/').to_string();
    config.search.auto_index = false;
    config
}

fn stream_requested(payload: &Value) -> bool {
    payload.get("stream").and_then(Value::as_bool) == Some(true)
}

fn no_initial_sse_response() -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        "",
    )
        .into_response()
}

async fn stalled_chat_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(json!({"id":"chatcmpl-stalled-fallback","object":"chat.completion","created":1,"model":"stalled-test-model","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"index":0,"id":"call_stalled_1","type":"function","function":{"name":"read_file","arguments":{"path":"src/lib.rs"}}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}})).into_response()
}

async fn stalled_messages_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(json!({"id":"msg-stalled-fallback","type":"message","role":"assistant","model":"stalled-test-model","content":[{"type":"tool_use","id":"toolu_stalled_1","name":"read_file","input":{"path":"src/lib.rs"}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":9,"output_tokens":3}})).into_response()
}

pub(crate) async fn spawn_stalled_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(stalled_chat_handler))
                .route("/v1/messages", post(stalled_messages_handler)),
        )
        .await
        .unwrap();
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
    Json(json!({"id":"chatcmpl-batch-fallback","object":"chat.completion","created":1,"model":"stalled-test-model","choices":[{"index":0,"message":{"role":"assistant","content":"chat-compat batch output"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}})).into_response()
}

async fn stalled_messages_text_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(json!({"id":"msg-batch-fallback","type":"message","role":"assistant","model":"stalled-test-model","content":[{"type":"text","text":"messages-v1 batch output"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}})).into_response()
}

pub(crate) async fn spawn_batch_chat_compat_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
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
        .unwrap();
    });
    (format!("http://{addr}"), server)
}

pub(crate) async fn spawn_batch_messages_v1_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
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
        .unwrap();
    });
    (format!("http://{addr}"), server)
}

pub(crate) async fn spawn_tool_calls_json_args_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
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
        .unwrap();
    });
    (format!("http://{addr}"), server)
}

pub(crate) async fn spawn_auto_detect_messages_v1_tool_calls_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
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
        .unwrap();
    });
    (format!("http://{addr}"), server)
}

pub(crate) async fn spawn_auto_detect_chat_tagged_tool_call_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", get(missing_probe_handler))
                .route(
                    "/v1/chat/completions",
                    get(probe_chat_handler).post(tagged_tool_call_chat_handler),
                ),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}"), server)
}

pub(crate) async fn spawn_auto_detect_messages_tagged_tool_call_server() -> (String, JoinHandle<()>)
{
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route(
                    "/v1/messages",
                    get(probe_messages_handler).post(tagged_tool_call_messages_v1_handler),
                )
                .route("/v1/chat/completions", get(missing_probe_handler)),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}"), server)
}

async fn streaming_messages_v1_text_handler(Json(payload): Json<Value>) -> Response {
    if !stream_requested(&payload) {
        return Json(json!({"id":"msg-stream-text","type":"message","role":"assistant","model":"stream-test-model","content":[{"type":"text","text":"fast acknowledgement"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":8,"output_tokens":2}})).into_response();
    }

    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-stream-text\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"stream-test-model\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":8,\"output_tokens\":1}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"fast acknowledgement\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":2}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ),
    )
        .into_response()
}

async fn spawn_messages_v1_stream_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/messages", post(streaming_messages_v1_text_handler)),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}"), server)
}

async fn tool_calls_json_args_chat_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(json!({"id":"chatcmpl-json-args","object":"chat.completion","created":1,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"index":0,"id":"call_json_args_1","type":"function","function":{"name":"read_file","arguments":{"path":"src/main.rs","offset":1,"limit":50}}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})).into_response()
}

async fn tool_calls_messages_v1_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return no_initial_sse_response();
    }
    Json(json!({"id":"msg-tool-fallback","type":"message","role":"assistant","model":"test-model","content":[{"type":"tool_use","id":"toolu_auto_1","name":"read_file","input":{"path":"src/lib.rs"}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":5}})).into_response()
}

async fn tagged_tool_call_chat_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return (
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"<function=read_file>\\n<parameter=path>src/main.rs</parameter>\\n</function>\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            ),
        )
            .into_response();
    }

    Json(json!({"id":"chatcmpl-tagged-tool","object":"chat.completion","created":1,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"<function=read_file>\n<parameter=path>src/main.rs</parameter>\n</function>"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})).into_response()
}

async fn tagged_tool_call_messages_v1_handler(Json(payload): Json<Value>) -> Response {
    if stream_requested(&payload) {
        return (
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            concat!(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-tagged-tool\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"test-model\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"<function=read_file>\\n<parameter=path>src/lib.rs</parameter>\\n</function>\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":5}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            ),
        )
            .into_response();
    }

    Json(json!({"id":"msg-tagged-tool","type":"message","role":"assistant","model":"test-model","content":[{"type":"text","text":"<function=read_file>\n<parameter=path>src/lib.rs</parameter>\n</function>"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":5}})).into_response()
}

async fn slow_non_stream_handler(_payload: Json<Value>) -> Response {
    tokio::time::sleep(Duration::from_millis(400)).await;
    Json(json!({"error": "too slow"})).into_response()
}

async fn spawn_slow_fallback_server() -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(slow_non_stream_handler))
                .route("/v1/messages", post(slow_non_stream_handler)),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}"), server)
}

fn assert_batch_jsonl_response(output_lines: &[String], expected_response: &str) {
    assert_eq!(
        output_lines.len(),
        2,
        "expected one pulse record and one summary"
    );
    let turn_record: Value = serde_json::from_str(&output_lines[0]).unwrap();
    assert_eq!(
        turn_record.get("response").and_then(Value::as_str),
        Some(expected_response)
    );
    assert_eq!(turn_record.get("pulse").and_then(Value::as_u64), Some(1));
    let summary_record: Value = serde_json::from_str(&output_lines[1]).unwrap();
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
async fn messages_v1_stream_emits_text_and_completes() {
    let (base_url, server) = spawn_messages_v1_stream_server().await;
    let config = build_messages_v1_config(&base_url, "stream-test-model");

    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    let mut stream = client
        .create_stream(&single_user_message(
            "Reply with a short plain-text acknowledgement.",
        ))
        .await
        .expect("messages-v1 stream should start");

    let mut transcript = String::new();
    let mut completed = false;

    while let Some(envelope) = stream.next().await {
        let envelope = envelope.expect("messages-v1 envelope");
        match envelope.signal {
            RuntimeSignal::TranscriptBlockDelta { delta, .. } => transcript.push_str(&delta),
            RuntimeSignal::TranscriptBlockStart {
                block: vexcoder::state::StreamBlock::FinalText { content },
                ..
            } => transcript.push_str(&content),
            RuntimeSignal::PulseEnd { status, .. } if status == "completed" => completed = true,
            _ => {}
        }
    }

    assert!(completed, "messages-v1 stream must complete successfully");
    assert!(
        !transcript.trim().is_empty(),
        "messages-v1 stream must emit assistant text"
    );
    server.abort();
}

#[tokio::test]
async fn stalled_stream_falls_back_to_non_stream_json_for_both_protocols() {
    for protocol in ["chat-compat", "messages-v1"] {
        let (base_url, server) = spawn_stalled_server().await;
        let config = if protocol == "chat-compat" {
            build_chat_compat_config(&base_url, "stalled-test-model")
        } else {
            build_messages_v1_config(&base_url, "stalled-test-model")
        };
        let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
        let started = Instant::now();
        let mut stream = client
            .create_stream(&single_user_message("Inspect src/lib.rs."))
            .await
            .expect("stalled stream should fall back");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{protocol}: fallback should be prompt; elapsed={:?}",
            started.elapsed()
        );

        let envelopes: Vec<_> = {
            let mut v = Vec::new();
            while let Some(e) = stream.next().await {
                v.push(e.expect("envelope"));
            }
            v
        };
        let expected_id = if protocol == "chat-compat" {
            "call_stalled_1"
        } else {
            "toolu_stalled_1"
        };
        assert!(envelopes.iter().any(|e| matches!(&e.signal, RuntimeSignal::TranscriptBlockStart { block: vexcoder::state::StreamBlock::ToolCall { id, name, .. }, .. } if id == expected_id && name == "read_file")), "{protocol}: must emit tool call block");
        assert!(envelopes.iter().any(|e| matches!(&e.signal, RuntimeSignal::ToolCallStarted { tool_call_id, tool_name, .. } if tool_call_id.starts_with("tx_") && tool_name == "read_file")), "{protocol}: must emit ToolCallStarted");
        assert!(
            !envelopes
                .iter()
                .any(|e| matches!(&e.signal, RuntimeSignal::ToolCallArgumentsDelta { .. })),
            "{protocol}: materialized fallback must not emit deltas"
        );
        assert!(envelopes.iter().any(|e| matches!(&e.signal, RuntimeSignal::PulseEnd { status, .. } if status == "completed")), "{protocol}: must emit PulseEnd");
        server.abort();
    }
}

#[tokio::test]
async fn batch_auto_detects_protocol_and_produces_correct_output_for_text_and_jsonl() {
    for (label, jsonl) in [
        ("chat-compat text", false),
        ("messages-v1 text", false),
        ("chat-compat jsonl", true),
        ("messages-v1 jsonl", true),
    ] {
        let is_chat = label.starts_with("chat-compat");
        let (base_url, server) = if is_chat {
            spawn_batch_chat_compat_server().await
        } else {
            spawn_batch_messages_v1_server().await
        };
        let config = build_auto_detect_batch_config(&base_url, "stalled-test-model");
        let expected = if is_chat {
            "chat-compat batch output"
        } else {
            "messages-v1 batch output"
        };
        let result = run_batch(
            "Reply with exactly the fallback text.".to_string(),
            BatchRunOpts {
                max_turns: Some(1),
                format: if jsonl {
                    OutputFormat::Jsonl
                } else {
                    OutputFormat::Text
                },
                ..Default::default()
            },
            &config,
        )
        .await
        .expect("batch run should succeed");
        if jsonl {
            assert_batch_jsonl_response(&result.output_lines, expected);
        } else {
            assert_eq!(
                result.output_lines,
                vec![expected.to_string()],
                "{label}: unexpected output"
            );
        }
        server.abort();
    }
}

#[tokio::test]
async fn slow_fallback_post_surfaces_error_promptly() {
    let (base_url, server) = spawn_slow_fallback_server().await;
    let config = build_chat_compat_config(&base_url, "timeout-test-model");
    let client = vexcoder::api::ApiClient::new(&config).expect("client should build");
    let started = Instant::now();
    let result = client.create_stream(&single_user_message("test")).await;
    assert!(result.is_err(), "slow fallback POST must surface an error");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "slow fallback path should fail promptly; elapsed={:?}",
        started.elapsed()
    );
    server.abort();
}

#[tokio::test]
async fn empty_and_null_content_fallback_emit_pulse_end_without_tool_calls() {
    for (label, content_field) in [("empty-string", json!("")), ("null-no-tools", json!(null))] {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let response = json!({"id":"chatcmpl-edge","object":"chat.completion","created":1,"model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content": content_field},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":1,"total_tokens":6}});
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/v1/chat/completions",
                    post(move |Json(p): Json<Value>| {
                        let resp = response.clone();
                        async move {
                            if stream_requested(&p) {
                                no_initial_sse_response()
                            } else {
                                Json(resp).into_response()
                            }
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let config = build_chat_compat_config(&format!("http://{addr}"), "edge-model");
        let client = vexcoder::api::ApiClient::new(&config).expect("client");
        let mut stream = client
            .create_stream(&single_user_message("Say nothing."))
            .await
            .expect("must not error");
        let envelopes: Vec<_> = {
            let mut v = Vec::new();
            while let Some(e) = stream.next().await {
                v.push(e.expect("envelope"));
            }
            v
        };
        assert!(
            envelopes
                .iter()
                .any(|e| matches!(&e.signal, RuntimeSignal::PulseEnd { .. })),
            "{label}: must emit PulseEnd"
        );
        assert!(
            !envelopes
                .iter()
                .any(|e| matches!(&e.signal, RuntimeSignal::ToolCallStarted { .. })),
            "{label}: must not emit ToolCallStarted"
        );
        server.abort();
    }
}
