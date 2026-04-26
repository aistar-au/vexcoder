use super::*;
use crate::runtime::RuntimeEvent;
use crate::runtime::backend::{ModelBackendKind, ModelProtocol, ToolCallMode};
use crate::test_support::{ENV_LOCK, RequestLog, spawn_axum_server};
use crate::types::{ApiMessage, Content};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use futures::StreamExt;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod protocol;

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

#[tokio::test]
async fn client_falls_back_to_non_streaming_chat_compat_when_stream_is_slow() {
    async fn handler(State(log): State<RequestLog>, Json(p): Json<Value>) -> impl IntoResponse {
        log.lock().unwrap().push(p.clone());
        if p.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return ([(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"late\"},\"finish_reason\":\"stop\"}]}\n\n",
            ).into_response();
        }
        Json(json!({
            "id":"chatcmpl-fallback","object":"chat.completion","created":1,"model":"local/test-model",
            "choices":[{"index":0,"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}
        })).into_response()
    }
    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let (server, addr) = spawn_axum_server(
        Router::new()
            .route("/v1/chat/completions", post(handler))
            .with_state(requests.clone()),
    )
    .await;
    let config = local_stream_test_config(
        format!("http://{addr}/v1/chat/completions"),
        ModelProtocol::ChatCompat,
    );
    let client = ApiClient::new(&config).expect("client");
    let mut stream = client
        .create_stream(&single_user_message("Reply OK."))
        .await
        .expect("stream");
    let envelopes: Vec<_> = futures::StreamExt::collect::<Vec<_>>(&mut stream)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();
    server.abort();
    let reqs = requests.lock().unwrap();
    assert_eq!(reqs.len(), 2, "expected streaming request + fallback retry");
    assert_eq!(reqs[0].get("stream"), Some(&Value::Bool(true)));
    assert_eq!(reqs[1].get("stream"), Some(&Value::Bool(false)));
    assert!(envelopes.iter().any(|e| matches!(&e.event,
        RuntimeEvent::TranscriptBlockDelta { delta, .. } if delta == "OK"
    )));
}

#[tokio::test]
async fn client_falls_back_to_non_streaming_messages_v1_when_stream_is_slow() {
    async fn handler(State(log): State<RequestLog>, Json(p): Json<Value>) -> impl IntoResponse {
        log.lock().unwrap().push(p.clone());
        if p.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return ([(header::CONTENT_TYPE, "text/event-stream")],
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-late\",\"role\":\"assistant\",\"model\":\"local/test-model\"}}\n\n",
            ).into_response();
        }
        Json(json!({
            "id":"msg-fallback","type":"message","role":"assistant","model":"local/test-model",
            "content":[{"type":"tool_use","id":"toolu_1","name":"read_file","input":{"path":"src/lib.rs"}}],
            "stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":11,"output_tokens":3}
        })).into_response()
    }
    let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let (server, addr) = spawn_axum_server(
        Router::new()
            .route("/v1/messages", post(handler))
            .with_state(requests.clone()),
    )
    .await;
    let config = local_stream_test_config(
        format!("http://{addr}/v1/messages"),
        ModelProtocol::MessagesV1,
    );
    let client = ApiClient::new(&config).expect("client");
    let mut stream = client
        .create_stream(&single_user_message("Read src/lib.rs"))
        .await
        .expect("stream");
    let envelopes: Vec<_> = futures::StreamExt::collect::<Vec<_>>(&mut stream)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();
    server.abort();
    let reqs = requests.lock().unwrap();
    assert_eq!(reqs.len(), 2, "expected streaming request + fallback retry");
    assert!(envelopes.iter().any(|e| matches!(&e.event,
        RuntimeEvent::ToolCallStarted { tool_name, .. } if tool_name == "read_file"
    )));
}

#[test]
fn map_api_status_error_encodes_protocol_hint_and_server_message() {
    let msg = format!(
        "{}",
        map_api_status_error(
            StatusCode::BAD_REQUEST,
            "invalid model name",
            "http://localhost:8000/v1/messages",
            None
        )
    );
    assert!(msg.contains("MessagesV1"), "400: got: {msg}");
    let msg = format!(
        "{}",
        map_api_status_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "out of memory",
            "http://localhost:8000/v1/messages",
            None
        )
    );
    assert!(
        msg.contains("500") && msg.contains("out of memory"),
        "500: got: {msg}"
    );
}

#[tokio::test]
async fn map_api_request_error_local_connect_failure_mentions_retry() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let url = format!("http://{addr}/v1/messages");
    let err = reqwest::Client::new()
        .post(&url)
        .send()
        .await
        .expect_err("should fail");
    let msg = map_api_request_error(err, &url).to_string();
    assert!(
        msg.contains("retries short-lived local startup connection failures"),
        "got: {msg}"
    );
}

#[test]
fn system_prompt_lists_required_tools_and_approval_notice() {
    let prompt = BASE_SYSTEM_PROMPT;
    assert!(prompt.contains("run_command"), "missing run_command");
    assert!(prompt.contains("approval"), "missing approval notice");
}
