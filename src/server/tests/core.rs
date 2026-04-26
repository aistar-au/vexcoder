use super::*;
use crate::api::stream::StreamParser;
use crate::app::UiUpdate;
use crate::runtime::json_handoff::{PulseEndContext, RuntimeEnvelopeNormalizer, RuntimeEvent};
use crate::state::{StreamBlock, ToolStatus};
use serde_json::json;

#[tokio::test]
async fn core_endpoints_health_privacy_schema_return_ok() {
    let router = build_router(Config::default_for_tui());
    for uri in ["/v1/health", "/v1/privacy", "/v1/schema"] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "endpoint {uri} must return 200"
        );
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/privacy")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("version"), Some(&json!(1)));
}

#[tokio::test]
async fn delegate_and_watch_routes_create_session_task_rollup() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".vex")).unwrap();
    std::fs::write(temp.path().join(".vex/agents.toml"), "[[agents]]\nname = \"reviewer\"\nisolation = \"worktree\"\nallowed_capabilities = [\"read-file\"]\n").unwrap();
    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let router = build_router(config);

    let body = to_bytes(
        router.clone().oneshot(Request::builder().method("POST").uri("/v1/delegate")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"parent_task_id":"test-parent","agent_id":"reviewer","prompt":"inspect docs"}"#))
            .unwrap()).await.unwrap().into_body(), usize::MAX).await.unwrap();
    let session_task_id = serde_json::from_slice::<Value>(&body)
        .unwrap()
        .get("session_task_id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    let watch_body = to_bytes(
        router
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/watch/{session_task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .into_body(),
        usize::MAX,
    )
    .await
    .unwrap();
    let watch: Value = serde_json::from_slice(&watch_body).unwrap();
    assert_eq!(
        watch.get("kind"),
        Some(&Value::String("session-task".into()))
    );
    assert_eq!(watch.get("id"), Some(&Value::String(session_task_id)));
}

#[tokio::test]
async fn bearer_token_enforcement_blocks_unauthenticated_requests() {
    let mut config = Config::default_for_tui();
    config.api.key = Some("token-123".to_string());
    let router = build_http_router(
        LocalApiState::new(config),
        HttpSurfaceSettings {
            bearer_token: Arc::<str>::from("token-123"),
            hsts_enabled: false,
        },
    );
    let unauth = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
    assert!(unauth.headers().contains_key("x-request-id"));

    let authed = router
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .header("Authorization", "Bearer token-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authed.status(), StatusCode::OK);
}

#[tokio::test]
async fn invalid_request_uses_problem_details_content_type() {
    let router = build_router(Config::default_for_tui());
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/pulses")
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
            .and_then(|v| v.to_str().ok()),
        Some("application/problem+json")
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("status"), Some(&Value::Number(400u64.into())));
}

#[test]
fn resolve_serve_config_loopback_allowed_non_loopback_requires_tls() {
    let mut config = Config::default_for_tui();
    config.api.key = Some("token-123".to_string());

    config.api.host = "192.168.1.20".to_string();
    let error = resolve_serve_config(&config, None, None).unwrap_err();
    assert!(format!("{error:#}").contains("requires both api.tls_cert and api.tls_key"));

    config.api.host = "127.42.0.7".to_string();
    let resolved = resolve_serve_config(&config, None, None).unwrap();
    let http = resolved.http.expect("http surface");
    assert_eq!(http.bind_addr, "127.42.0.7");
    assert!(http.tls.is_none());

    config.api.host = "::1".to_string();
    let resolved = resolve_serve_config(&config, None, None).unwrap();
    assert!(resolved.http.unwrap().tls.is_none());
}
