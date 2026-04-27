use super::tools::{tool_definitions_chat_compat_for_policy, tool_definitions_for_policy};
use super::*;
use crate::runtime::RuntimeSignal;
use crate::runtime::backend::{ModelBackendKind, ModelProtocol, ToolCallMode};
use crate::test_support::{RequestLog, spawn_axum_server};
use crate::types::{ApiMessage, Content};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use axum::routing::post;
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::convert::Infallible;
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
            return Json(json!({
                "id":"chatcmpl-stream-json","object":"chat.completion","created":1,"model":"local/test-model",
                "choices":[{"index":0,"message":{"role":"assistant","content":"late"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}
            })).into_response();
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
    assert!(envelopes.iter().any(|e| matches!(&e.signal,
        RuntimeSignal::TranscriptBlockDelta { delta, .. } if delta == "OK"
    )));
}

#[tokio::test]
async fn client_falls_back_to_non_streaming_messages_v1_when_stream_is_slow() {
    async fn handler(State(log): State<RequestLog>, Json(p): Json<Value>) -> impl IntoResponse {
        log.lock().unwrap().push(p.clone());
        if p.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return Json(json!({
                "id":"msg-stream-json","type":"message","role":"assistant","model":"local/test-model",
                "content":[{"type":"text","text":"late"}],
                "stop_reason":"end_turn","stop_sequence":null,
                "usage":{"input_tokens":11,"output_tokens":1}
            })).into_response();
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
    assert!(envelopes.iter().any(|e| matches!(&e.signal,
        RuntimeSignal::ToolCallStarted { tool_name, .. } if tool_name == "read_file"
    )));
}

#[tokio::test]
async fn client_waits_for_delayed_local_non_stream_messages_v1_fallback() {
    async fn handler(State(log): State<RequestLog>, Json(p): Json<Value>) -> impl IntoResponse {
        log.lock().unwrap().push(p.clone());
        if p.get("stream").and_then(Value::as_bool) == Some(true) {
            tokio::time::sleep(Duration::from_millis(75)).await;
            return ([ (header::CONTENT_TYPE, "text/event-stream") ],
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-late\",\"role\":\"assistant\",\"model\":\"local/test-model\"}}\n\n",
            ).into_response();
        }

        tokio::time::sleep(Duration::from_millis(125)).await;
        Json(json!({
            "id":"msg-fallback","type":"message","role":"assistant","model":"local/test-model",
            "content":[{"type":"text","text":"Delayed OK"}],
            "stop_reason":"end_turn","stop_sequence":null,
            "usage":{"input_tokens":11,"output_tokens":2}
        }))
        .into_response()
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
        .create_stream(&single_user_message("Reply with Delayed OK."))
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
    assert!(envelopes.iter().any(|e| matches!(&e.signal,
        RuntimeSignal::TranscriptBlockDelta { delta, .. } if delta == "Delayed OK"
    )));
}

#[tokio::test]
async fn client_keeps_local_sse_stream_when_first_event_is_delayed() {
    async fn handler(State(log): State<RequestLog>, Json(p): Json<Value>) -> impl IntoResponse {
        log.lock().unwrap().push(p.clone());

        let delayed_start = stream::once(async {
            tokio::time::sleep(Duration::from_millis(75)).await;
            Ok::<Event, Infallible>(
                Event::default().event("message_start").data(
                    r#"{"type":"message_start","message":{"id":"msg-stream","type":"message","role":"assistant","model":"local/test-model","content":[],"stop_reason":null,"stop_sequence":null}}"#,
                ),
            )
        });
        let tail = stream::iter(vec![
            Ok::<Event, Infallible>(
                Event::default().event("content_block_start").data(
                    r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                ),
            ),
            Ok::<Event, Infallible>(
                Event::default().event("content_block_delta").data(
                    r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"streamed OK"}}"#,
                ),
            ),
            Ok::<Event, Infallible>(
                Event::default().event("content_block_stop").data(
                    r#"{"type":"content_block_stop","index":0}"#,
                ),
            ),
            Ok::<Event, Infallible>(
                Event::default().event("message_delta").data(
                    r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":11,"output_tokens":2}}"#,
                ),
            ),
        ]);

        Sse::new(delayed_start.chain(tail)).into_response()
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
        .create_stream(&single_user_message("Reply with streamed OK."))
        .await
        .expect("stream");
    let envelopes: Vec<_> = futures::StreamExt::collect::<Vec<_>>(&mut stream)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();
    server.abort();

    let reqs = requests.lock().unwrap();
    assert_eq!(
        reqs.len(),
        1,
        "delayed SSE should not trigger non-stream fallback"
    );
    assert_eq!(reqs[0].get("stream"), Some(&Value::Bool(true)));
    assert!(envelopes.iter().any(|e| matches!(&e.signal,
        RuntimeSignal::TranscriptBlockDelta { delta, .. } if delta == "streamed OK"
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

#[test]
fn tool_definitions_keep_protocol_names_in_sync() {
    let base_names: BTreeSet<String> = tool_definitions_for_policy(ToolPolicy::Full, &[])
        .as_array()
        .expect("tool definitions must be an array")
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();

    let chat_compat_names: BTreeSet<String> =
        tool_definitions_chat_compat_for_policy(ToolPolicy::Full, &[])
            .as_array()
            .expect("chat-compat tool definitions must be an array")
            .iter()
            .filter_map(|tool| {
                tool.get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
            })
            .map(ToOwned::to_owned)
            .collect();

    assert_eq!(chat_compat_names, base_names);
}

#[test]
fn messages_v1_tool_definitions_keep_input_schemas_structured() {
    let definitions = tool_definitions_for_policy(ToolPolicy::Full, &[]);
    let tools = definitions
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
fn chat_compat_tool_definitions_keep_parameters_structured() {
    let definitions = tool_definitions_chat_compat_for_policy(ToolPolicy::Full, &[]);
    let tools = definitions
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
fn shared_prefix_fingerprint_changes_with_workspace_context() {
    let client = ApiClient::new(&crate::config::Config::default_for_tui()).expect("client");

    let left = client
        .shared_prefix_fingerprint("## Shared context prefix\n- src/lib.rs\n")
        .expect("left fingerprint");
    let right = client
        .shared_prefix_fingerprint("## Shared context prefix\n- src/main.rs\n")
        .expect("right fingerprint");

    assert_ne!(left, right);
}

#[test]
fn shared_prefix_fingerprint_tracks_supplementary_prompt() {
    let client = ApiClient::new(&crate::config::Config::default_for_tui()).expect("client");
    let workspace_context = "## Shared context prefix\n- src/lib.rs\n";
    let baseline = client
        .shared_prefix_fingerprint(workspace_context)
        .expect("baseline fingerprint");

    client.set_supplementary_system_prompt(Some("Use a precise reviewer tone.".to_string()));

    let updated = client
        .shared_prefix_fingerprint(workspace_context)
        .expect("updated fingerprint");

    assert_ne!(baseline, updated);
}
