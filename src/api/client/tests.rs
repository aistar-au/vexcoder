use super::*;
use crate::config::CompactionConfig;
use crate::runtime::RuntimeEvent;
use crate::runtime::backend::{ModelBackendKind, ModelProtocol, ToolCallMode};
use crate::test_support::ENV_LOCK;
use crate::types::{ApiMessage, Content};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use futures::StreamExt;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn with_vex_max_tokens_env<T>(value: Option<&str>, run: impl FnOnce() -> T) -> T {
    let env_lock = ENV_LOCK.blocking_lock();
    let _guard = crate::test_support::EnvRestore::capture(&env_lock, "VEX_MAX_TOKENS");

    match value {
        Some(value) => crate::test_support::test_set_var(&env_lock, "VEX_MAX_TOKENS", value),
        None => crate::test_support::test_remove_var(&env_lock, "VEX_MAX_TOKENS"),
    }

    run()
}

fn local_stream_test_config(url: String, protocol: ModelProtocol) -> crate::config::Config {
    let mut config = crate::config::Config::default_for_tui();
    config.model_name = "local/test-model".to_string();
    config.model_url = url;
    config.model_token = None;
    config.model_backend = ModelBackendKind::LocalRuntime;
    config.model_protocol = protocol;
    config.tool_call_mode = ToolCallMode::Structured;
    config
}

fn single_user_message(text: &str) -> Vec<ApiMessage> {
    vec![ApiMessage {
        role: "user".to_string(),
        content: Content::Text(text.to_string()),
        cache_hint: None,
    }]
}

mod protocol;

#[tokio::test]
async fn test_create_stream_falls_back_to_non_streaming_chat_compat_response() {
    type RequestLog = Arc<Mutex<Vec<Value>>>;

    async fn handler(
        State(log): State<RequestLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        log.lock().unwrap().push(payload.clone());

        if payload.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"late\"},\"finish_reason\":\"stop\"}]}\n\n",
            )
                .into_response();
        }

        Json(json!({
            "id": "chatcmpl-fallback",
            "object": "chat.completion",
            "created": 1,
            "model": "local/test-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "OK"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 7,
                "completion_tokens": 2,
                "total_tokens": 9
            }
        }))
        .into_response()
    }

    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let request_log = requests.clone();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .with_state(requests),
        )
        .await
        .unwrap();
    });

    let config = local_stream_test_config(
        format!("http://{addr}/v1/chat/completions"),
        ModelProtocol::ChatCompat,
    );
    let client = ApiClient::new(&config).expect("client should build");
    let mut stream = client
        .create_stream(&single_user_message("Reply exactly OK."))
        .await
        .expect("stream should build");

    let mut envelopes = Vec::new();
    while let Some(event) = stream.next().await {
        envelopes.push(event.expect("runtime envelope"));
    }

    server.abort();

    let requests = request_log.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "expected streaming request plus fallback retry"
    );
    assert_eq!(requests[0].get("stream"), Some(&Value::Bool(true)));
    assert_eq!(requests[1].get("stream"), Some(&Value::Bool(false)));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "OK"
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TurnEnd { status, .. } if status == "completed"
    )));
}

#[tokio::test]
async fn test_create_stream_falls_back_when_local_stream_closes_without_initial_sse_event() {
    type RequestLog = Arc<Mutex<Vec<Value>>>;

    async fn handler(
        State(log): State<RequestLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        log.lock().unwrap().push(payload.clone());

        if payload.get("stream").and_then(Value::as_bool) == Some(true) {
            return Json(json!({
                "error": "stream route returned JSON instead of SSE"
            }))
            .into_response();
        }

        Json(json!({
            "id": "chatcmpl-fallback-no-sse",
            "object": "chat.completion",
            "created": 1,
            "model": "local/test-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "OK"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 7,
                "completion_tokens": 2,
                "total_tokens": 9
            }
        }))
        .into_response()
    }

    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let request_log = requests.clone();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .with_state(requests),
        )
        .await
        .unwrap();
    });

    let config = local_stream_test_config(
        format!("http://{addr}/v1/chat/completions"),
        ModelProtocol::ChatCompat,
    );
    let client = ApiClient::new(&config).expect("client should build");
    let mut stream = client
        .create_stream(&single_user_message("Reply exactly OK."))
        .await
        .expect("stream should build");

    let mut envelopes = Vec::new();
    while let Some(event) = stream.next().await {
        envelopes.push(event.expect("runtime envelope"));
    }

    server.abort();

    let requests = request_log.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "expected streaming request plus fallback retry"
    );
    assert_eq!(requests[0].get("stream"), Some(&Value::Bool(true)));
    assert_eq!(requests[1].get("stream"), Some(&Value::Bool(false)));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "OK"
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TurnEnd { status, .. } if status == "completed"
    )));
}

#[tokio::test]
async fn test_create_stream_falls_back_to_non_streaming_chat_compat_parallel_tool_calls() {
    type RequestLog = Arc<Mutex<Vec<Value>>>;

    async fn handler(
        State(log): State<RequestLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        log.lock().unwrap().push(payload.clone());

        if payload.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null},\"finish_reason\":null}]}\n\n",
            )
                .into_response();
        }

        Json(json!({
            "id": "chatcmpl-fallback-tools",
            "object": "chat.completion",
            "created": 1,
            "model": "local/test-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": "call_read_01",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": {
                                    "path": "src/lib.rs"
                                }
                            }
                        },
                        {
                            "index": 1,
                            "id": "call_find_01",
                            "type": "function",
                            "function": {
                                "name": "find_files",
                                "arguments": {
                                    "pattern": "src/**/*.rs"
                                }
                            }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 13,
                "completion_tokens": 4,
                "total_tokens": 17
            }
        }))
        .into_response()
    }

    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let request_log = requests.clone();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .with_state(requests),
        )
        .await
        .unwrap();
    });

    let config = local_stream_test_config(
        format!("http://{addr}/v1/chat/completions"),
        ModelProtocol::ChatCompat,
    );
    let client = ApiClient::new(&config).expect("client should build");
    let mut stream = client
        .create_stream(&single_user_message("Inspect the Rust files."))
        .await
        .expect("stream should build");

    let mut envelopes = Vec::new();
    while let Some(event) = stream.next().await {
        envelopes.push(event.expect("runtime envelope"));
    }

    server.abort();

    let requests = request_log.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "expected streaming request plus fallback retry"
    );
    assert_eq!(requests[0].get("stream"), Some(&Value::Bool(true)));
    assert_eq!(requests[1].get("stream"), Some(&Value::Bool(false)));
    assert_eq!(
        requests[0].get("parallel_tool_calls"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        requests[1].get("parallel_tool_calls"),
        Some(&Value::Bool(true))
    );

    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TranscriptBlockStart {
            block: crate::state::StreamBlock::ToolCall { id, name, input, .. },
            ..
        } if id == "call_read_01"
            && name == "read_file"
            && input == &json!({"path":"src/lib.rs"})
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TranscriptBlockStart {
            block: crate::state::StreamBlock::ToolCall { id, name, input, .. },
            ..
        } if id == "call_find_01"
            && name == "find_files"
            && input == &json!({"pattern":"src/**/*.rs"})
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::ToolCallStarted {
            tool_name,
            arguments,
            tool_call_id,
            ..
        } if tool_call_id.starts_with("tx_")
            && tool_name == "read_file"
            && arguments == &json!({"path":"src/lib.rs"})
    )));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::ToolCallStarted {
            tool_name,
            arguments,
            tool_call_id,
            ..
        } if tool_call_id.starts_with("tx_")
            && tool_name == "find_files"
            && arguments == &json!({"pattern":"src/**/*.rs"})
    )));
    assert!(
        !envelopes
            .iter()
            .any(|envelope| matches!(&envelope.event, RuntimeEvent::ToolCallArgumentsDelta { .. })),
        "materialized non-stream tool calls should not be replayed as synthetic argument deltas",
    );

    let usage_updates: Vec<(u64, u64)> = envelopes
        .iter()
        .filter_map(|envelope| match &envelope.event {
            RuntimeEvent::UsageUpdated { usage } => Some((usage.input, usage.output)),
            _ => None,
        })
        .collect();
    assert_eq!(usage_updates, vec![(13, 4)]);
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TurnEnd {
            status,
            usage: Some(usage),
            ..
        } if status == "completed" && usage.input == 13 && usage.output == 4
    )));
}

#[tokio::test]
async fn test_create_stream_rejects_malformed_chat_compat_non_stream_fallback_response() {
    type RequestLog = Arc<Mutex<Vec<Value>>>;

    async fn handler(
        State(log): State<RequestLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        log.lock().unwrap().push(payload.clone());

        if payload.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[]}\n\n",
            )
                .into_response();
        }

        Json(json!({
            "error": "too slow"
        }))
        .into_response()
    }

    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let request_log = requests.clone();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .with_state(requests),
        )
        .await
        .unwrap();
    });

    let config = local_stream_test_config(
        format!("http://{addr}/v1/chat/completions"),
        ModelProtocol::ChatCompat,
    );
    let client = ApiClient::new(&config).expect("client should build");
    let error = match client
        .create_stream(&single_user_message("Reply exactly OK."))
        .await
    {
        Ok(_) => panic!("malformed non-stream fallback payload must be rejected"),
        Err(error) => error,
    };

    server.abort();

    let requests = request_log.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "expected streaming request plus fallback retry"
    );
    assert_eq!(requests[0].get("stream"), Some(&Value::Bool(true)));
    assert_eq!(requests[1].get("stream"), Some(&Value::Bool(false)));
    let message = error.to_string();
    assert!(
        message.contains("failed to normalize non-streaming fallback response")
            || message.contains("error payload"),
        "error message: {message}"
    );
}

#[tokio::test]
async fn test_create_stream_times_out_when_chat_compat_non_stream_fallback_stalls() {
    type RequestLog = Arc<Mutex<Vec<Value>>>;

    async fn handler(
        State(log): State<RequestLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        log.lock().unwrap().push(payload.clone());

        if payload.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[]}\n\n",
            )
                .into_response();
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
        Json(json!({
            "id": "chatcmpl-too-late",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "late"},
                "finish_reason": "stop"
            }]
        }))
        .into_response()
    }

    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let request_log = requests.clone();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .with_state(requests),
        )
        .await
        .unwrap();
    });

    let config = local_stream_test_config(
        format!("http://{addr}/v1/chat/completions"),
        ModelProtocol::ChatCompat,
    );
    let client = ApiClient::new(&config).expect("client should build");
    let started = Instant::now();
    let error = match client
        .create_stream(&single_user_message("Reply exactly OK."))
        .await
    {
        Ok(_) => panic!("stalled non-stream fallback should time out"),
        Err(error) => error,
    };

    let elapsed = started.elapsed();
    let message = error.to_string();

    server.abort();

    let requests = request_log.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "expected streaming request plus fallback retry"
    );
    assert_eq!(requests[0].get("stream"), Some(&Value::Bool(true)));
    assert_eq!(requests[1].get("stream"), Some(&Value::Bool(false)));
    assert!(message.contains("timed out"), "error message: {message}");
    assert!(
        message.contains("did not answer before the configured timeout"),
        "error message: {message}"
    );
    assert!(
        elapsed >= Duration::from_millis(125) && elapsed < Duration::from_secs(1),
        "expected bounded local fallback timeout, got elapsed={elapsed:?}"
    );
}

#[tokio::test]
async fn test_create_stream_falls_back_to_non_streaming_messages_v1_tool_use() {
    type RequestLog = Arc<Mutex<Vec<Value>>>;

    async fn handler(
        State(log): State<RequestLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        log.lock().unwrap().push(payload.clone());

        if payload.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-late\",\"role\":\"assistant\",\"model\":\"local/test-model\"}}\n\n",
            )
                .into_response();
        }

        Json(json!({
            "id": "msg-fallback",
            "type": "message",
            "role": "assistant",
            "model": "local/test-model",
            "content": [{
                "type": "tool_use",
                "id": "toolu_read_01",
                "name": "read_file",
                "input": {
                    "path": "src/lib.rs"
                }
            }],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 11,
                "output_tokens": 3
            }
        }))
        .into_response()
    }

    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let request_log = requests.clone();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", post(handler))
                .with_state(requests),
        )
        .await
        .unwrap();
    });

    let config = local_stream_test_config(
        format!("http://{addr}/v1/messages"),
        ModelProtocol::MessagesV1,
    );
    let client = ApiClient::new(&config).expect("client should build");
    let mut stream = client
        .create_stream(&single_user_message("Read src/lib.rs"))
        .await
        .expect("stream should build");

    let mut envelopes = Vec::new();
    while let Some(event) = stream.next().await {
        envelopes.push(event.expect("runtime envelope"));
    }

    server.abort();

    let requests = request_log.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "expected streaming request plus fallback retry"
    );
    assert_eq!(requests[0].get("stream"), Some(&Value::Bool(true)));
    assert_eq!(requests[1].get("stream"), Some(&Value::Bool(false)));
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::ToolCallStarted {
            tool_name,
            arguments,
            ..
        } if tool_name == "read_file" && arguments.get("path") == Some(&json!("src/lib.rs"))
    )));
    let usage_updates: Vec<(u64, u64)> = envelopes
        .iter()
        .filter_map(|envelope| match &envelope.event {
            RuntimeEvent::UsageUpdated { usage } => Some((usage.input, usage.output)),
            _ => None,
        })
        .collect();
    assert_eq!(usage_updates, vec![(11, 3)]);
    assert!(envelopes.iter().any(|envelope| matches!(
        &envelope.event,
        RuntimeEvent::TurnEnd {
            status,
            usage: Some(usage),
            ..
        } if status == "completed" && usage.input == 11 && usage.output == 3
    )));
    assert!(
        !envelopes
            .iter()
            .any(|envelope| matches!(&envelope.event, RuntimeEvent::ToolCallArgumentsDelta { .. }))
    );
}

#[tokio::test]
async fn test_create_stream_times_out_when_messages_v1_non_stream_fallback_stalls() {
    type RequestLog = Arc<Mutex<Vec<Value>>>;

    async fn handler(
        State(log): State<RequestLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        log.lock().unwrap().push(payload.clone());

        if payload.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-late\",\"role\":\"assistant\",\"model\":\"local-test\"}}\n\n",
            )
                .into_response();
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
        Json(json!({
            "id": "msg-too-late",
            "type": "message",
            "role": "assistant",
            "model": "local-test",
            "content": [{"type": "text", "text": "late"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 3, "output_tokens": 1}
        }))
        .into_response()
    }

    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let request_log = requests.clone();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", post(handler))
                .with_state(requests),
        )
        .await
        .unwrap();
    });

    let config = local_stream_test_config(
        format!("http://{addr}/v1/messages"),
        ModelProtocol::MessagesV1,
    );
    let client = ApiClient::new(&config).expect("client should build");
    let started = Instant::now();
    let error = match client
        .create_stream(&single_user_message("Reply exactly OK."))
        .await
    {
        Ok(_) => panic!("stalled non-stream fallback should time out"),
        Err(error) => error,
    };

    let elapsed = started.elapsed();
    let message = error.to_string();

    server.abort();

    let requests = request_log.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "expected streaming request plus fallback retry"
    );
    assert_eq!(requests[0].get("stream"), Some(&Value::Bool(true)));
    assert_eq!(requests[1].get("stream"), Some(&Value::Bool(false)));
    assert!(message.contains("timed out"), "error message: {message}");
    assert!(
        message.contains("did not answer before the configured timeout"),
        "error message: {message}"
    );
    assert!(
        elapsed >= Duration::from_millis(125) && elapsed < Duration::from_secs(1),
        "expected bounded local fallback timeout, got elapsed={elapsed:?}"
    );
}

#[tokio::test]
async fn test_populate_server_info_discovers_protocol_from_api_client_base_url() {
    async fn messages_v1_probe() -> impl IntoResponse {
        StatusCode::NOT_FOUND
    }

    async fn chat_compat_probe() -> impl IntoResponse {
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            "data: {\"ok\":true}\n\n",
        )
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", get(messages_v1_probe))
                .route("/v1/chat/completions", get(chat_compat_probe)),
        )
        .await
        .unwrap();
    });

    let mut config = crate::config::Config::default_for_tui();
    config.model_name = "local/test-model".to_string();
    config.model_url.clear();
    config.model_token = None;
    config.api_client.base_url = format!("http://{addr}");

    let client = ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    let info = client
        .server_info()
        .expect("server info should be populated");
    assert_eq!(info.native_protocol, Some(ModelProtocol::ChatCompat));
    assert_eq!(
        client.request_url(),
        format!("http://{addr}/v1/chat/completions")
    );

    server.abort();
}

#[tokio::test]
async fn test_populate_server_info_prefers_messages_v1_when_base_url_exposes_both_protocols() {
    async fn messages_v1_probe() -> impl IntoResponse {
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            "event: ping\ndata: {\"type\":\"ping\"}\n\n",
        )
    }

    async fn chat_compat_probe() -> impl IntoResponse {
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            "data: {\"choices\":[]}\n\n",
        )
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", get(messages_v1_probe))
                .route("/v1/chat/completions", get(chat_compat_probe)),
        )
        .await
        .unwrap();
    });

    let mut config = crate::config::Config::default_for_tui();
    config.model_name = "local/test-model".to_string();
    config.model_url.clear();
    config.model_token = None;
    config.api_client.base_url = format!("http://{addr}");

    let client = ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    let info = client
        .server_info()
        .expect("server info should be populated");
    assert_eq!(info.native_protocol, Some(ModelProtocol::MessagesV1));
    assert_eq!(client.request_url(), format!("http://{addr}/v1/messages"));

    server.abort();
}

#[tokio::test]
async fn test_populate_server_info_discovers_protocol_from_local_model_url_session() {
    async fn messages_v1_probe() -> impl IntoResponse {
        StatusCode::NOT_FOUND
    }

    async fn chat_compat_probe() -> impl IntoResponse {
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            "data: {\"ok\":true}\n\n",
        )
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", get(messages_v1_probe))
                .route("/v1/chat/completions", get(chat_compat_probe)),
        )
        .await
        .unwrap();
    });

    let mut config = crate::config::Config::default_for_tui();
    config.model_name = "local/test-model".to_string();
    config.model_url = format!("http://{addr}/v1/messages");
    config.model_token = None;

    let client = ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    let info = client
        .server_info()
        .expect("server info should be populated");
    assert_eq!(info.native_protocol, Some(ModelProtocol::ChatCompat));
    assert_eq!(client.request_url(), format!("http://{addr}/v1/messages"));

    server.abort();
}

#[tokio::test]
async fn test_populate_server_info_infers_chat_compat_from_server_props_when_sse_probes_fail() {
    async fn missing_probe() -> impl IntoResponse {
        StatusCode::NOT_FOUND
    }

    async fn server_props() -> impl IntoResponse {
        (
            [(header::SERVER, super::chat_compat_runtime_server_header())],
            axum::Json(json!({
                "default_generation_settings": {
                    "n_ctx": 8192,
                    "n_batch": 2048,
                    "model": "local-chat-model"
                },
                "chat_template": "{% for message in messages %}{{ message.role }}{% endfor %}",
                "chat_template_caps": {
                    "supports_tools": true,
                    "supports_tool_calls": true
                }
            })),
        )
    }

    async fn models() -> impl IntoResponse {
        (
            [(header::SERVER, super::chat_compat_runtime_server_header())],
            axum::Json(json!({
                "data": [{
                    "id": "local-chat-model",
                    "owned_by": super::chat_compat_runtime_owner()
                }]
            })),
        )
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/props", get(server_props))
                .route("/v1/models", get(models))
                .route("/v1/messages", get(missing_probe))
                .route("/v1/chat/completions", get(missing_probe)),
        )
        .await
        .unwrap();
    });

    let mut config = crate::config::Config::default_for_tui();
    config.model_name = "local/test-model".to_string();
    config.model_url = format!("http://{addr}/v1/messages");
    config.model_token = None;

    let client = ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    let info = client
        .server_info()
        .expect("server info should be populated");
    assert_eq!(info.native_protocol, Some(ModelProtocol::ChatCompat));
    assert_eq!(info.n_ctx, 8192);
    assert_eq!(info.n_batch, 2048);
    assert_eq!(info.model, "local-chat-model");
    assert_eq!(client.request_url(), format!("http://{addr}/v1/messages"));

    server.abort();
}

#[tokio::test]
async fn test_populate_server_info_respects_probe_timeout_config() {
    async fn slow_probe() -> impl IntoResponse {
        tokio::time::sleep(Duration::from_millis(75)).await;
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            "data: {\"ok\":true}\n\n",
        )
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/messages", get(slow_probe))
                .route("/v1/chat/completions", get(slow_probe)),
        )
        .await
        .unwrap();
    });

    let mut config = crate::config::Config::default_for_tui();
    config.model_name = "local/test-model".to_string();
    config.model_url.clear();
    config.model_token = None;
    config.api_client.base_url = format!("http://{addr}");
    config.api_client.probe_timeout_ms = 10;

    let client = ApiClient::new(&config).expect("client should build");
    client.populate_server_info().await;

    let info = client.server_info();
    assert!(
        info.is_none()
            || info
                .as_ref()
                .is_some_and(|entry| entry.native_protocol.is_none()),
        "probe should time out before protocol discovery succeeds"
    );

    server.abort();
}

#[test]
fn test_remote_messages_endpoint_preserves_messages_wire_protocol() {
    let config = crate::config::Config {
        model_token: Some("test-key".to_string()),
        model_name: "remote-test-model".to_string(),
        model_url: "https://model.example.internal/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::ApiServer,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: crate::types::ModelProfile::default_for_backend(ModelBackendKind::ApiServer),
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
    assert_eq!(
        client.request_url(),
        "https://model.example.internal/v1/messages"
    );
}

#[test]
fn test_https_localhost_messages_endpoint_preserves_full_request_url() {
    let config = crate::config::Config {
        model_token: Some("test-key".to_string()),
        model_name: "remote-test-model".to_string(),
        model_url: "https://localhost:8443/v1/messages".to_string(),
        model_url_skip_tls_check: true,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::ApiServer,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: crate::types::ModelProfile::default_for_backend(ModelBackendKind::ApiServer),
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
    assert_eq!(client.request_url(), "https://localhost:8443/v1/messages");
}

#[test]
fn test_local_plain_http_hint_suggests_plain_http_endpoint() {
    let hint = local_plain_http_hint("https://localhost:8000/v1/messages");
    assert_eq!(hint, " Try 'http://localhost:8000/v1/messages'.");
}

#[test]
fn test_chat_compat_url_adapter_from_messages_endpoint() {
    let adapted = adapt_to_chat_compat_url("http://localhost:8000/v1/messages");
    assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
}

#[test]
fn test_chat_compat_url_adapter_from_v1_base_endpoint() {
    let adapted = adapt_to_chat_compat_url("http://localhost:8000/v1");
    assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
}

#[test]
fn test_resolve_max_tokens_defaults_to_profile_budget() {
    let tokens = with_vex_max_tokens_env(None, || resolve_max_tokens(4096, 0));
    assert_eq!(tokens, 4096);
}

#[test]
fn test_resolve_max_tokens_uses_server_n_ctx() {
    let tokens = with_vex_max_tokens_env(None, || resolve_max_tokens(4096, 65536));
    assert_eq!(tokens, 4096);
}

#[test]
fn test_resolve_max_tokens_caps_at_seventy_five_percent_of_server_n_ctx() {
    let tokens = with_vex_max_tokens_env(None, || resolve_max_tokens(60000, 65536));
    assert_eq!(tokens, 49152);
}

#[test]
fn test_resolve_max_tokens_unknown_server_caps_at_ceiling() {
    let tokens = with_vex_max_tokens_env(None, || resolve_max_tokens(40000, 0));
    assert_eq!(tokens, 16384);
}

#[test]
fn test_resolve_max_tokens_small_n_ctx_does_not_panic() {
    let tokens = with_vex_max_tokens_env(None, || resolve_max_tokens(4096, 100));
    assert_eq!(tokens, 75);
}

#[test]
fn test_resolve_max_tokens_n_ctx_one_returns_zero() {
    let tokens = with_vex_max_tokens_env(None, || resolve_max_tokens(4096, 1));
    assert_eq!(tokens, 0);
}

#[test]
fn test_resolve_max_tokens_keeps_env_override_as_upper_bound_for_small_n_ctx() {
    let tokens = with_vex_max_tokens_env(Some("50"), || resolve_max_tokens(4096, 100));
    assert_eq!(tokens, 50);
}

#[test]
fn test_tool_definitions_cover_execute_tool_routing_names() {
    let expected: BTreeSet<&str> = BTreeSet::from([
        "read_file",
        "write_file",
        "apply_patch",
        "edit_file",
        "rename_file",
        "list_files",
        "list_directory",
        "list_dir",
        "glob_files",
        "search_files",
        "search",
        "git_status",
        "git_diff",
        "git_log",
        "git_show",
        "git_add",
        "git_commit",
        "search_content",
        "find_files",
        "codebase_search",
        "run_command",
    ]);

    let names: BTreeSet<String> = tool_definitions()
        .as_array()
        .expect("tool definitions must be an array")
        .iter()
        .filter_map(|tool: &Value| tool.get("name").and_then(|value: &Value| value.as_str()))
        .map(ToOwned::to_owned)
        .collect();

    let expected_owned: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(names, expected_owned);
}

#[test]
fn test_structured_tool_protocol_env_off_does_not_disable_direct_client_config() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_env_lock, "VEX_STRUCTURED_TOOL_PROTOCOL", "off");
    let config = crate::config::Config {
        model_token: None,
        model_name: "mock-model".to_string(),
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
    assert!(client.supports_structured_tool_protocol());
    crate::test_support::test_remove_var(&_env_lock, "VEX_STRUCTURED_TOOL_PROTOCOL");
}

#[test]
fn test_structured_tool_protocol_defaults_on_for_local_endpoint() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_remove_var(&_env_lock, "VEX_STRUCTURED_TOOL_PROTOCOL");
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
    assert!(client.supports_structured_tool_protocol());
}

#[test]
fn test_structured_tool_protocol_defaults_on_for_remote_endpoint() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_remove_var(&_env_lock, "VEX_STRUCTURED_TOOL_PROTOCOL");
    let config = crate::config::Config {
        model_token: Some("test-key".to_string()),
        model_name: "mistral-7b-instruct".to_string(),
        model_url: "https://model.example.internal/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::ApiServer,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: crate::types::ModelProfile::default_for_backend(ModelBackendKind::ApiServer),
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
    assert!(client.supports_structured_tool_protocol());
}

#[test]
fn test_reserved_header_guard_blocks_auth_headers() {
    assert!(is_reserved_header("authorization"));
    assert!(is_reserved_header("Authorization"));
    assert!(is_reserved_header("x-api-key"));
    assert!(is_reserved_header("X-Api-Key"));
    assert!(is_reserved_header("content-length"));
    assert!(!is_reserved_header("x-custom-header"));
    assert!(!is_reserved_header("x-api-version"));
}

#[test]
fn test_chat_compat_tool_definitions_match_base_tool_names() {
    let base_names: BTreeSet<String> = tool_definitions()
        .as_array()
        .expect("tool definitions must be an array")
        .iter()
        .filter_map(|tool: &Value| tool.get("name").and_then(|value: &Value| value.as_str()))
        .map(ToOwned::to_owned)
        .collect();

    let chat_compat_names: BTreeSet<String> = tool_definitions_chat_compat_with_extra(&[])
        .as_array()
        .expect("chat-compat tool definitions must be an array")
        .iter()
        .filter_map(|tool| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(|name| name.as_str())
        })
        .map(ToOwned::to_owned)
        .collect();

    assert_eq!(chat_compat_names, base_names);
}

#[test]
fn test_messages_v1_tool_definitions_keep_input_schemas_structured() {
    let tools = tool_definitions()
        .as_array()
        .expect("tool definitions must be an array");

    assert!(
        tools
            .iter()
            .all(|tool| matches!(tool.get("input_schema"), Some(Value::Object(_)))),
        "messages-v1 tool definitions must keep input_schema as a JSON object"
    );
}

#[test]
fn test_chat_compat_tool_definitions_keep_parameters_structured() {
    let chat_compat_tools = tool_definitions_chat_compat_with_extra(&[]);
    let tools = chat_compat_tools
        .as_array()
        .expect("chat-compat tool definitions must be an array");

    assert!(
        tools.iter().all(|tool| matches!(
            tool.get("function")
                .and_then(|function| function.get("parameters")),
            Some(Value::Object(_))
        )),
        "chat-compat tool definitions must keep function.parameters as a JSON object"
    );
}

#[test]
fn test_system_prompt_includes_memory_notes() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )))
    .with_notes_content(Some("remember this".to_string()));
    let prompt = client.system_prompt();
    assert!(prompt.starts_with(BASE_SYSTEM_PROMPT));
    assert!(prompt.contains("<memory>\nremember this\n</memory>"));
}

#[test]
fn test_system_prompt_omits_blank_memory_notes() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )))
    .with_notes_content(Some("   \n".to_string()));
    assert_eq!(client.system_prompt(), BASE_SYSTEM_PROMPT);
}

#[test]
fn test_system_prompt_restricts_git_tool_capability_claims() {
    assert!(BASE_SYSTEM_PROMPT.contains("only list built-in git tools"));
    assert!(
        BASE_SYSTEM_PROMPT.contains("git_status, git_diff, git_log, git_show, git_add, git_commit")
    );
    assert!(BASE_SYSTEM_PROMPT.contains("Do not claim unsupported git tools"));
}

#[test]
fn test_system_prompt_includes_large_file_edit_guidance() {
    assert!(BASE_SYSTEM_PROMPT.contains(
            "use write_file only for smaller full-file rewrites that stay under the write-file guard thresholds"
        ));
    assert!(
        BASE_SYSTEM_PROMPT
            .contains("For large files, prefer apply_patch or edit_file over write_file")
    );
    assert!(
        BASE_SYSTEM_PROMPT
            .contains("escalating to apply_patch when the change is too broad for edit_file")
    );
}

#[test]
fn test_system_prompt_discourages_non_rustfmt_rust_diffs() {
    assert!(BASE_SYSTEM_PROMPT.contains("keep diffs rustfmt-consistent"));
    assert!(BASE_SYSTEM_PROMPT.contains("do not hand-wrap argument lists or method chains"));
}

#[test]
fn test_write_file_tool_description_uses_guard_names_instead_of_hardcoded_numbers() {
    let definitions = tool_definitions();
    let description = definitions
        .as_array()
        .expect("tool definitions must be an array")
        .iter()
        .find(|entry: &&Value| {
            entry.get("name").and_then(|v: &Value| v.as_str()) == Some("write_file")
        })
        .and_then(|entry: &Value| entry.get("description"))
        .and_then(|value: &Value| value.as_str())
        .expect("write_file description must be present");

    assert!(description.contains("diff-preferred threshold"));
    assert!(description.contains("max line limit"));
    assert!(!description.contains("~200"));
    assert!(!description.contains("~500"));
}

#[test]
fn test_edit_tools_discourage_non_rustfmt_rust_diffs() {
    let definitions = tool_definitions();
    let entries = definitions
        .as_array()
        .expect("tool definitions must be an array");

    let apply_patch_description = entries
        .iter()
        .find(|entry| entry.get("name").and_then(|v| v.as_str()) == Some("apply_patch"))
        .and_then(|entry| entry.get("description"))
        .and_then(|value| value.as_str())
        .expect("apply_patch description must be present");
    assert!(apply_patch_description.contains("rustfmt-consistent"));
    assert!(apply_patch_description.contains("formatting-only churn"));

    let edit_file_description = entries
        .iter()
        .find(|entry| entry.get("name").and_then(|v| v.as_str()) == Some("edit_file"))
        .and_then(|entry| entry.get("description"))
        .and_then(|value| value.as_str())
        .expect("edit_file description must be present");
    assert!(edit_file_description.contains("rustfmt-consistent"));
    assert!(edit_file_description.contains("preserve surrounding style"));
}

#[test]
fn test_system_prompt_includes_history_condensing_awareness() {
    assert!(BASE_SYSTEM_PROMPT.contains("Tool results from earlier turns may be condensed"));
}

#[test]
fn test_with_project_instructions_none_uses_base_prompt() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )))
    .with_project_instructions(None);
    assert_eq!(client.effective_system_prompt(), BASE_SYSTEM_PROMPT);
}

#[test]
fn test_with_project_instructions_some_wraps_in_delimiters() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )))
    .with_project_instructions(Some("# no unwrap".to_string()));
    let prompt = client.effective_system_prompt();
    assert!(prompt.starts_with(BASE_SYSTEM_PROMPT));
    assert!(prompt.contains("[project instructions: start]"));
    assert!(prompt.contains("# no unwrap"));
    assert!(prompt.contains("[project instructions: end]"));
}

#[test]
fn test_system_prompt_includes_supplementary_prompt_when_set() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )));
    client.set_supplementary_system_prompt(Some("coding mode active".to_string()));

    let prompt = client.system_prompt();

    assert!(prompt.contains("[coding prompt: start]"));
    assert!(prompt.contains("coding mode active"));
    assert!(prompt.contains("[coding prompt: end]"));
}

#[test]
fn test_system_prompt_clears_supplementary_prompt() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )));
    client.set_supplementary_system_prompt(Some("coding mode active".to_string()));
    client.set_supplementary_system_prompt(None);

    assert_eq!(client.system_prompt(), BASE_SYSTEM_PROMPT);
}

#[test]
fn test_is_context_overflow_detects_token_exceeded() {
    assert!(is_context_overflow(
        "request (4291 tokens) exceeds the available context size (4096 tokens), try increasing it"
    ));
}

#[test]
fn test_is_context_overflow_detects_max_context_length() {
    assert!(is_context_overflow(
        "This model's maximum context length is 4096 tokens"
    ));
}

#[test]
fn test_is_context_overflow_negative() {
    assert!(!is_context_overflow("invalid model name"));
    assert!(!is_context_overflow("bad request"));
    assert!(!is_context_overflow(""));
}

#[test]
fn test_map_api_status_error_context_overflow_local() {
    let err = map_api_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        "request (4291 tokens) exceeds the available context size (4096 tokens)",
        "http://localhost:8000/v1/messages",
        None,
    );
    let msg = format!("{}", err);
    assert!(
        msg.contains("exceeded the server's context window"),
        "got: {msg}"
    );
    assert!(msg.contains("--ctx-size"), "got: {msg}");
    assert!(msg.contains("/compact"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_generic_400_local() {
    let err = map_api_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        "invalid model name",
        "http://localhost:8000/v1/messages",
        None,
    );
    let msg = format!("{}", err);
    assert!(msg.contains("protocol"), "got: {msg}");
    assert!(msg.contains("MessagesV1"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_remote_400() {
    let err = map_api_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        "bad request body",
        "https://api.example.com/v1/messages",
        None,
    );
    let msg = format!("{}", err);
    assert!(msg.contains("bad request body"), "got: {msg}");
    assert!(!msg.contains("--ctx-size"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_server_500() {
    let err = map_api_status_error(
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error: out of memory",
        "http://localhost:8000/v1/messages",
        None,
    );
    let msg = format!("{}", err);
    assert!(msg.contains("500"), "got: {msg}");
    assert!(msg.contains("out of memory"), "got: {msg}");
}

#[tokio::test]
async fn test_map_api_request_error_local_connect_mentions_retry_and_telemetry() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let request_url = format!("http://{addr}/v1/messages");
    let error = reqwest::Client::new()
        .post(&request_url)
        .send()
        .await
        .expect_err("request should fail once the listener is dropped");

    let msg = map_api_request_error(error, &request_url).to_string();
    assert!(
        msg.contains("retries short-lived local startup connection failures"),
        "got: {msg}"
    );
    assert!(msg.contains("--display-internal-telemetry"), "got: {msg}");
    assert!(msg.contains("RUST_LOG=debug"), "got: {msg}");
}

#[test]
fn test_system_prompt_registers_run_command_with_approval_notice() {
    let prompt = BASE_SYSTEM_PROMPT;
    assert!(
        prompt.contains("run_command"),
        "system prompt must list run_command in the registered tool set"
    );
    assert!(
        prompt.contains("run_shell_command"),
        "system prompt must mention run_shell_command alias"
    );
    assert!(
        prompt.contains("approval"),
        "system prompt must state that run_command requires user approval"
    );
}
