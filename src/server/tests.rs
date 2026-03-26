use crate::config::Config;
use crate::local_api::{ActiveTask, LocalApiState, LocalApiTaskShared};
use crate::runtime::json_handoff::RuntimeEnvelopeNormalizer;

use super::http::{build_http_router, build_router};
use super::sse::runtime_sse_response;
use super::util::resolve_serve_config;
use super::HttpSurfaceSettings;

use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::timeout;
use tower::ServiceExt;

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
async fn test_delegate_and_watch_routes_create_session_task_snapshot() {
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
                    r#"{"agent_id":"reviewer","prompt":"inspect docs"}"#,
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
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
    assert_eq!(
        payload.get("reason"),
        Some(&Value::String("unauthorized".to_string()))
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
        runtime_sse_response(receiver, Duration::from_millis(20))
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
                    r#"{"type":"interrupt","task_id":"missing-task"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
    assert_eq!(
        payload.get("reason"),
        Some(&Value::String("task_not_found".to_string()))
    );
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
                        r#"{"type":"approve_capability","task_id":"missing-task","capability":"run_command","scope":"once"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
    assert_eq!(
        payload.get("reason"),
        Some(&Value::String("task_not_found".to_string()))
    );
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
    let shared = Arc::new(Mutex::new(LocalApiTaskShared {
        normalizer: RuntimeEnvelopeNormalizer::new(task_id.clone()),
        envelope_tx,
        pending_approval: None,
        quit,
        turn_in_progress: false,
        interrupted: false,
    }));
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
                        r#"{{"type":"approve_capability","task_id":"{task_id}","capability":"run_command","scope":"once"}}"#,
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
    assert_eq!(
        payload.get("reason"),
        Some(&Value::String("no_pending_approval".to_string()))
    );
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

    if !super::util::is_strict_loopback_host("localhost", config.api.port).unwrap() {
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
