use super::phase_e::{delegate_one, setup_phase_e_router};
use super::*;

#[tokio::test]
async fn watch_returns_not_found_for_unknown_session_task() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/session-tasks/no-such-id/watch")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn watch_emits_rollup_and_terminates_on_completed_status() {
    let temp = tempfile::tempdir().unwrap();
    let st_id = delegate_one(
        setup_phase_e_router(temp.path()),
        "watch-parent",
        temp.path(),
    )
    .await;

    setup_phase_e_router(temp.path())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/session-tasks/{st_id}/status"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"status":"completed"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    let response = setup_phase_e_router(temp.path())
        .oneshot(
            Request::builder()
                .uri(format!("/v1/session-tasks/{st_id}/watch"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CACHE_CONTROL).unwrap(),
        "no-cache, no-store, must-revalidate"
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("session-task") || text.contains("completed") || text.contains(&st_id),
        "watch response must contain task rollup; body: {text}"
    );
}
