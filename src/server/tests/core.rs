use super::*;
use crate::api::stream::StreamParser;
use crate::app::UiUpdate;
use crate::runtime::json_handoff::{RuntimeEnvelopeNormalizer, TurnEndContext};
use crate::state::{StreamBlock, ToolStatus};
use crate::types::{ContentBlock, StreamEvent};
use serde_json::json;

#[tokio::test]
async fn test_health_endpoint_returns_ok() {
    let router = build_router(Config::default_for_tui());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_schema_endpoint_returns_bundle() {
    let router = build_router(Config::default_for_tui());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/schema")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_agents_endpoint_returns_available_false_without_config() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let router = build_router(config);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("available"), Some(&Value::Bool(false)));
}

#[tokio::test]
async fn test_delegate_and_watch_routes_create_session_task_rollup() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".vex")).unwrap();
    std::fs::write(
        temp.path().join(".vex/agents.toml"),
        r#"
[[agents]]
name = "reviewer"
isolation = "worktree"
allowed_capabilities = ["read-file"]
"#,
    )
    .unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let router = build_router(config);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/delegate")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"parent_task_id":"test-parent","agent_id":"reviewer","prompt":"inspect docs"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    let session_task_id = payload
        .get("session_task_id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    let response = router
        .oneshot(
            Request::builder()
                .uri(format!("/v1/watch/{session_task_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let watch: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        watch.get("kind"),
        Some(&Value::String("session-task".into()))
    );
    assert_eq!(
        watch.get("id"),
        Some(&Value::String(session_task_id.clone()))
    );
}

#[tokio::test]
async fn test_release_session_task_route_completes_task_and_drops_lease() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".vex")).unwrap();
    std::fs::write(
        temp.path().join(".vex/agents.toml"),
        r#"
[[agents]]
name = "reviewer"
isolation = "worktree"
allowed_capabilities = ["read-file"]
"#,
    )
    .unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let router = build_router(config);

    let delegate = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/delegate")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"parent_task_id":"parent-release","agent_id":"reviewer","prompt":"inspect docs"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delegate.status(), StatusCode::OK);
    let body = to_bytes(delegate.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    let session_task_id = payload
        .get("session_task_id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    let watch_before = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/watch/{session_task_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(watch_before.status(), StatusCode::OK);
    let body = to_bytes(watch_before.into_body(), usize::MAX)
        .await
        .unwrap();
    let watch_before_payload: Value = serde_json::from_slice(&body).unwrap();
    let worktree_path = watch_before_payload
        .get("worktree_path")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    assert!(Path::new(&worktree_path).exists());

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/session-tasks/{session_task_id}/release"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("ok"), Some(&Value::Bool(true)));

    let watch_after = router
        .oneshot(
            Request::builder()
                .uri(format!("/v1/watch/{session_task_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(watch_after.status(), StatusCode::OK);
    let body = to_bytes(watch_after.into_body(), usize::MAX).await.unwrap();
    let watch_after_payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        watch_after_payload.get("status"),
        Some(&Value::String("completed".to_string()))
    );
    assert_eq!(
        watch_after_payload.get("worktree_path"),
        Some(&Value::String(worktree_path.clone()))
    );
    assert!(!Path::new(&worktree_path).exists());
}

#[tokio::test]
async fn test_release_session_task_route_returns_not_found_for_unknown_id() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let router = build_router(config);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/session-tasks/missing-session-task/release")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        payload.get("type"),
        Some(&Value::String(
            "https://aistar-au.github.io/vexcoder/problems/session_task_not_found".to_string()
        ))
    );
    assert_eq!(payload.get("status"), Some(&Value::Number(404u64.into())));
    assert_eq!(content_type.as_deref(), Some("application/problem+json"));
}

#[tokio::test]
async fn test_http_router_requires_bearer_token() {
    let mut config = Config::default_for_tui();
    config.api.key = Some("token-123".to_string());
    let router = build_http_router(
        LocalApiState::new(config),
        HttpSurfaceSettings {
            bearer_token: Arc::<str>::from("token-123"),
            hsts_enabled: false,
        },
    );

    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = router
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .header(AUTHORIZATION, "Bearer token-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_http_router_rejects_invalid_bearer_token() {
    let mut config = Config::default_for_tui();
    config.api.key = Some("token-123".to_string());
    let router = build_http_router(
        LocalApiState::new(config),
        HttpSurfaceSettings {
            bearer_token: Arc::<str>::from("token-123"),
            hsts_enabled: false,
        },
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .header(AUTHORIZATION, "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        payload.get("type"),
        Some(&Value::String(
            "https://aistar-au.github.io/vexcoder/problems/unauthorized".to_string()
        ))
    );
    assert_eq!(payload.get("status"), Some(&Value::Number(401u64.into())));
    assert_eq!(content_type.as_deref(), Some("application/problem+json"));
}

#[tokio::test]
async fn test_turns_invalid_request_uses_problem_details_content_type() {
    let router = build_router(Config::default_for_tui());
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"type":"interrupt","request_id":"req-1","task_id":"task-1"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/problem+json")
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("status"), Some(&Value::Number(400u64.into())));
    assert_eq!(
        payload.get("type"),
        Some(&Value::String(
            "https://aistar-au.github.io/vexcoder/problems/invalid_request_type".to_string()
        ))
    );
}

#[tokio::test]
async fn test_runtime_sse_response_emits_keepalive_comment() {
    #[derive(Clone)]
    struct TestSseState {
        _sender: mpsc::UnboundedSender<String>,
        receiver: Arc<AsyncMutex<Option<mpsc::UnboundedReceiver<String>>>>,
    }

    async fn keepalive_handler(State(state): State<TestSseState>) -> impl IntoResponse {
        let receiver = state
            .receiver
            .lock()
            .await
            .take()
            .expect("single keepalive request");
        runtime_sse_response(
            receiver,
            Duration::from_millis(20),
            TurnsSseMode::RuntimeEnvelope,
        )
    }

    let (sender, receiver) = mpsc::unbounded_channel::<String>();
    let state = TestSseState {
        _sender: sender,
        receiver: Arc::new(AsyncMutex::new(Some(receiver))),
    };
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/", get(keepalive_handler))
                .with_state(state),
        )
        .await
        .unwrap();
    });

    let response = reqwest::get(format!("http://{addr}/")).await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let mut stream = response.bytes_stream();
    let mut frame = Vec::new();
    for _ in 0..8 {
        let chunk = timeout(
            Duration::from_secs(1),
            futures::StreamExt::next(&mut stream),
        )
        .await
        .expect("keepalive chunk timed out")
        .expect("stream ended unexpectedly")
        .unwrap();
        frame.extend_from_slice(&chunk);

        if frame.windows(2).any(|window| window == b"\n\n")
            || frame.windows(4).any(|window| window == b"\r\n\r\n")
        {
            break;
        }
    }
    let payload = String::from_utf8_lossy(&frame);
    assert!(
        payload.contains(": keepalive"),
        "expected SSE keepalive comment, got {payload:?}"
    );

    server.abort();
}

#[tokio::test]
async fn test_runtime_sse_response_omits_event_ids_until_resume_support_exists() {
    #[derive(Clone)]
    struct TestSseState {
        receiver: Arc<AsyncMutex<Option<mpsc::UnboundedReceiver<String>>>>,
    }

    async fn runtime_handler(State(state): State<TestSseState>) -> impl IntoResponse {
        let receiver = state
            .receiver
            .lock()
            .await
            .take()
            .expect("single runtime request");
        runtime_sse_response(
            receiver,
            Duration::from_millis(20),
            TurnsSseMode::RuntimeEnvelope,
        )
    }

    let (sender, receiver) = mpsc::unbounded_channel::<String>();
    let state = TestSseState {
        receiver: Arc::new(AsyncMutex::new(Some(receiver))),
    };
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/", get(runtime_handler))
                .with_state(state),
        )
        .await
        .unwrap();
    });

    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-runtime-sse");
    let mut envelopes = vec![normalizer.start_turn(1, Some("inspect file".to_string()))];
    envelopes.extend(
        normalizer.normalize_ui_update(&UiUpdate::TurnComplete, Some(TurnEndContext::default())),
    );
    for envelope in envelopes {
        sender
            .send(serde_json::to_string(&envelope).unwrap())
            .unwrap();
    }
    drop(sender);

    let response = reqwest::get(format!("http://{addr}/")).await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let bytes = response.bytes().await.unwrap();
    let payload = String::from_utf8_lossy(&bytes);
    assert!(
        payload.contains("data:"),
        "expected SSE data frame, got {payload:?}"
    );
    assert!(
        !payload.starts_with("id:") && !payload.contains("\nid:") && !payload.contains("\r\nid:"),
        "runtime SSE must omit event ids until replay exists, got {payload:?}"
    );

    server.abort();
}

#[tokio::test]
async fn test_runtime_sse_response_block_delta_emits_tx_tool_id_over_http() {
    #[derive(Clone)]
    struct TestSseState {
        receiver: Arc<AsyncMutex<Option<mpsc::UnboundedReceiver<String>>>>,
    }

    async fn protocol_handler(
        headers: axum::http::HeaderMap,
        State(state): State<TestSseState>,
    ) -> impl IntoResponse {
        let receiver = state
            .receiver
            .lock()
            .await
            .take()
            .expect("single block-delta request");
        let mode = negotiate_turns_sse_mode(
            headers
                .get(axum::http::header::ACCEPT)
                .and_then(|value| value.to_str().ok()),
        );
        runtime_sse_response(receiver, Duration::from_millis(20), mode)
    }

    let (sender, receiver) = mpsc::unbounded_channel::<String>();
    let state = TestSseState {
        receiver: Arc::new(AsyncMutex::new(Some(receiver))),
    };
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/", get(protocol_handler))
                .with_state(state),
        )
        .await
        .unwrap();
    });

    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-block-sse");
    let mut envelopes = vec![normalizer.start_turn(1, Some("inspect file".to_string()))];
    envelopes.extend(normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolCall {
                id: "provider-call-1".to_string(),
                name: "read_file".to_string(),
                input: json!({}),
                status: ToolStatus::Pending,
            },
        },
        None,
    ));
    envelopes.extend(normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockDelta {
            index: 1,
            delta: r#"{"path":"src/lib.rs"}"#.to_string(),
        },
        None,
    ));
    envelopes
        .extend(normalizer.normalize_ui_update(&UiUpdate::StreamBlockComplete { index: 1 }, None));
    envelopes.extend(
        normalizer.normalize_ui_update(&UiUpdate::TurnComplete, Some(TurnEndContext::default())),
    );
    for envelope in envelopes {
        sender
            .send(serde_json::to_string(&envelope).unwrap())
            .unwrap();
    }
    drop(sender);

    let response = reqwest::Client::new()
        .get(format!("http://{addr}/"))
        .header("Accept", "application/vnd.block-delta+sse")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let mut parser = StreamParser::new();
    let mut saw_tool_start = false;
    let mut saw_tool_delta = false;
    let mut saw_tool_stop = false;
    let mut saw_message_stop = false;
    let mut stream = response.bytes_stream();

    for _ in 0..12 {
        let Some(chunk) = timeout(
            Duration::from_secs(1),
            futures::StreamExt::next(&mut stream),
        )
        .await
        .expect("block-delta chunk timed out") else {
            break;
        };
        let chunk = chunk.expect("stream ended unexpectedly");
        for event in parser.process(&chunk).unwrap() {
            match event {
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlock::ToolUse { id, name, .. },
                    ..
                } => {
                    assert!(id.starts_with("tx_"), "expected tx_ id, got {id}");
                    assert_eq!(name, "read_file");
                    saw_tool_start = true;
                }
                StreamEvent::ContentBlockDelta { delta, .. } => {
                    assert_eq!(
                        delta.partial_json.as_deref(),
                        Some(r#"{"path":"src/lib.rs"}"#)
                    );
                    saw_tool_delta = true;
                }
                StreamEvent::ContentBlockStop { .. } => saw_tool_stop = true,
                StreamEvent::MessageStop => {
                    saw_message_stop = true;
                    break;
                }
                _ => {}
            }
        }
        if saw_message_stop {
            break;
        }
    }

    assert!(saw_tool_start, "expected tool start over block-delta SSE");
    assert!(saw_tool_delta, "expected tool delta over block-delta SSE");
    assert!(saw_tool_stop, "expected tool stop over block-delta SSE");
    assert!(
        saw_message_stop,
        "expected message stop over block-delta SSE"
    );

    server.abort();
}

#[tokio::test]
async fn test_runtime_sse_response_choices_delta_emits_tx_tool_id_over_http() {
    #[derive(Clone)]
    struct TestSseState {
        receiver: Arc<AsyncMutex<Option<mpsc::UnboundedReceiver<String>>>>,
    }

    async fn protocol_handler(
        headers: axum::http::HeaderMap,
        State(state): State<TestSseState>,
    ) -> impl IntoResponse {
        let receiver = state
            .receiver
            .lock()
            .await
            .take()
            .expect("single choices request");
        let mode = negotiate_turns_sse_mode(
            headers
                .get(axum::http::header::ACCEPT)
                .and_then(|value| value.to_str().ok()),
        );
        runtime_sse_response(receiver, Duration::from_millis(20), mode)
    }

    let (sender, receiver) = mpsc::unbounded_channel::<String>();
    let state = TestSseState {
        receiver: Arc::new(AsyncMutex::new(Some(receiver))),
    };
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/", get(protocol_handler))
                .with_state(state),
        )
        .await
        .unwrap();
    });

    let mut normalizer = RuntimeEnvelopeNormalizer::new("task-choices-sse");
    let mut envelopes = vec![normalizer.start_turn(1, Some("inspect file".to_string()))];
    envelopes.extend(normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "provider-call-1".to_string(),
                name: "read_file".to_string(),
                input: json!({}),
                status: ToolStatus::Pending,
            },
        },
        None,
    ));
    envelopes.extend(normalizer.normalize_ui_update(
        &UiUpdate::StreamBlockDelta {
            index: 0,
            delta: r#"{"path":"src/lib.rs"}"#.to_string(),
        },
        None,
    ));
    envelopes
        .extend(normalizer.normalize_ui_update(&UiUpdate::StreamBlockComplete { index: 0 }, None));
    envelopes.extend(
        normalizer.normalize_ui_update(&UiUpdate::TurnComplete, Some(TurnEndContext::default())),
    );
    for envelope in envelopes {
        sender
            .send(serde_json::to_string(&envelope).unwrap())
            .unwrap();
    }
    drop(sender);

    let response = reqwest::Client::new()
        .get(format!("http://{addr}/"))
        .header("Accept", "application/vnd.choices-delta+sse")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let mut parser = StreamParser::new();
    let mut saw_tool_start = false;
    let mut saw_tool_delta = false;
    let mut saw_tool_stop = false;
    let mut stream = response.bytes_stream();

    for _ in 0..12 {
        let Some(chunk) = timeout(
            Duration::from_secs(1),
            futures::StreamExt::next(&mut stream),
        )
        .await
        .expect("choices chunk timed out") else {
            break;
        };
        let chunk = chunk.expect("stream ended unexpectedly");
        for event in parser.process(&chunk).unwrap() {
            match event {
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlock::ToolUse { id, name, .. },
                    ..
                } => {
                    assert!(id.starts_with("tx_"), "expected tx_ id, got {id}");
                    assert_eq!(name, "read_file");
                    saw_tool_start = true;
                }
                StreamEvent::ContentBlockDelta { delta, .. } => {
                    assert_eq!(
                        delta.partial_json.as_deref(),
                        Some(r#"{"path":"src/lib.rs"}"#)
                    );
                    saw_tool_delta = true;
                }
                StreamEvent::ContentBlockStop { .. } => {
                    saw_tool_stop = true;
                    break;
                }
                _ => {}
            }
        }
        if saw_tool_stop {
            break;
        }
    }

    assert!(saw_tool_start, "expected tool start over choices SSE");
    assert!(saw_tool_delta, "expected tool delta over choices SSE");
    assert!(saw_tool_stop, "expected tool stop over choices SSE");

    server.abort();
}

#[tokio::test]
async fn test_interrupt_handler_returns_not_found_for_unknown_task() {
    let mut config = Config::default_for_tui();
    config.api.key = Some("token-123".to_string());
    let router = build_http_router(
        LocalApiState::new(config),
        HttpSurfaceSettings {
            bearer_token: Arc::<str>::from("token-123"),
            hsts_enabled: false,
        },
    );

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/interrupt")
                .header(AUTHORIZATION, "Bearer token-123")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"type":"interrupt","request_id":"req-interrupt-missing","task_id":"missing-task"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("status"), Some(&Value::Number(404u64.into())));
    assert_eq!(
        payload.get("type"),
        Some(&Value::String(
            "https://aistar-au.github.io/vexcoder/problems/task_not_found".to_string()
        ))
    );
    assert_eq!(content_type.as_deref(), Some("application/problem+json"));
}

#[tokio::test]
async fn test_approve_handler_returns_not_found_for_unknown_task() {
    let mut config = Config::default_for_tui();
    config.api.key = Some("token-123".to_string());
    let router = build_http_router(
        LocalApiState::new(config),
        HttpSurfaceSettings {
            bearer_token: Arc::<str>::from("token-123"),
            hsts_enabled: false,
        },
    );

    let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/approve")
                    .header(AUTHORIZATION, "Bearer token-123")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"type":"approve_capability","request_id":"req-approve-missing","task_id":"missing-task","capability":"run_command","scope":"once"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("status"), Some(&Value::Number(404u64.into())));
    assert_eq!(
        payload.get("type"),
        Some(&Value::String(
            "https://aistar-au.github.io/vexcoder/problems/task_not_found".to_string()
        ))
    );
    assert_eq!(content_type.as_deref(), Some("application/problem+json"));
}

#[tokio::test]
async fn test_approve_handler_returns_conflict_without_pending_approval() {
    let mut config = Config::default_for_tui();
    config.api.key = Some("token-123".to_string());
    let state = LocalApiState::new(config);
    let task_id = "task-approval-409".to_string();
    let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
    let (envelope_tx, _envelope_rx) = mpsc::unbounded_channel();
    let quit = Arc::new(AtomicBool::new(false));
    let shared = Arc::new(Mutex::new(LocalApiTaskShared::new(
        task_id.clone(),
        envelope_tx,
        quit,
        state
            .config
            .api_client
            .delta_accumulator_memory_watermark_bytes(),
    )));
    state.tasks.lock().await.insert(
        task_id.clone(),
        ActiveTask {
            interrupt_tx,
            shared,
        },
    );
    let router = build_http_router(
        state,
        HttpSurfaceSettings {
            bearer_token: Arc::<str>::from("token-123"),
            hsts_enabled: false,
        },
    );

    let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/approve")
                    .header(AUTHORIZATION, "Bearer token-123")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"type":"approve_capability","request_id":"req-approve-conflict","task_id":"{task_id}","capability":"run_command","scope":"once"}}"#,
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("status"), Some(&Value::Number(409u64.into())));
    assert_eq!(
        payload.get("type"),
        Some(&Value::String(
            "https://aistar-au.github.io/vexcoder/problems/no_pending_approval".to_string()
        ))
    );
    assert_eq!(content_type.as_deref(), Some("application/problem+json"));
}

#[tokio::test]
async fn test_turns_endpoint_rejects_schema_invalid_request() {
    let mut config = Config::default_for_tui();
    config.api.key = Some("token-123".to_string());
    let router = build_http_router(
        LocalApiState::new(config),
        HttpSurfaceSettings {
            bearer_token: Arc::<str>::from("token-123"),
            hsts_enabled: false,
        },
    );

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns")
                .header(AUTHORIZATION, "Bearer token-123")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"type":"submit_input","task_id":"task-only"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_turns_endpoint_returns_sse_headers() {
    let router = build_router(Config::default_for_tui());
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"type":"submit_input","request_id":"req-turns-sse","input":"hello"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CACHE_CONTROL).unwrap(),
        "no-cache, no-store, must-revalidate"
    );
    assert_eq!(response.headers().get("x-accel-buffering").unwrap(), "no");
    assert!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"))
    );
}

#[test]
fn test_resolve_serve_config_rejects_non_loopback_without_tls() {
    let mut config = Config::default_for_tui();
    config.api.host = "192.168.1.20".to_string();
    config.api.key = Some("token-123".to_string());

    let error = resolve_serve_config(&config, None, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("requires both api.tls_cert and api.tls_key"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn test_resolve_serve_config_accepts_ipv4_loopback_aliases_without_tls() {
    let mut config = Config::default_for_tui();
    config.api.host = "127.42.0.7".to_string();
    config.api.key = Some("token-123".to_string());

    let resolved = resolve_serve_config(&config, None, None).unwrap();
    let http = resolved.http.expect("http surface should be present");
    assert_eq!(http.bind_addr, "127.42.0.7");
    assert!(http.tls.is_none());
}

#[test]
fn test_resolve_serve_config_accepts_ipv6_loopback_without_tls() {
    let mut config = Config::default_for_tui();
    config.api.host = "::1".to_string();
    config.api.key = Some("token-123".to_string());

    let resolved = resolve_serve_config(&config, None, None).unwrap();
    let http = resolved.http.expect("http surface should be present");
    assert_eq!(http.bind_addr, "::1");
    assert!(http.tls.is_none());
}

#[test]
fn test_resolve_serve_config_accepts_localhost_without_tls() {
    let mut config = Config::default_for_tui();
    config.api.host = "localhost".to_string();
    config.api.key = Some("token-123".to_string());

    if !crate::server::util::is_strict_loopback_host("localhost", config.api.port).unwrap() {
        return;
    }

    let resolved = resolve_serve_config(&config, None, None).unwrap();
    let http = resolved.http.expect("http surface should be present");
    assert_eq!(http.bind_addr, "localhost");
    assert!(http.tls.is_none());
}

#[test]
fn test_resolve_serve_config_rejects_vpn_trust_true() {
    let mut config = Config::default_for_tui();
    config.api.host = "127.0.0.1".to_string();
    config.api.key = Some("token-123".to_string());
    config.api.vpn_trust = true;

    let error = resolve_serve_config(&config, None, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("api.vpn_trust must remain false"),
        "unexpected error: {error:#}"
    );
}
