use super::phase_e::delegate_one;
use super::*;

fn agents_toml_with_team() -> &'static str {
    r#"
[[agents]]
name = "reviewer"
isolation = "worktree"
max_parallel_tasks = 4

[[teams]]
name = "review-team"
scheduler = "fan_out_join"
members = ["reviewer"]
"#
}

fn setup_team_router(temp: &std::path::Path) -> axum::Router {
    std::fs::create_dir_all(temp.join(".vex")).unwrap();
    std::fs::write(temp.join(".vex/agents.toml"), agents_toml_with_team()).unwrap();
    let mut config = Config::default_for_tui();
    config.working_dir = temp.to_path_buf();
    build_router(config)
}

#[tokio::test]
async fn test_schedule_team_handler_returns_session_task_ids() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_team_router(temp.path());

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/teams/review-team/schedule")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"parent_task_id":"sched-parent","prompt":"review the code"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("ok"), Some(&Value::Bool(true)));
    assert_eq!(
        payload.get("parent_task_id"),
        Some(&Value::String("sched-parent".into()))
    );
    let ids = payload
        .get("session_task_ids")
        .and_then(Value::as_array)
        .expect("expected session_task_ids array");
    assert!(
        !ids.is_empty(),
        "expected at least one session task id from schedule_team"
    );
}

#[tokio::test]
async fn test_schedule_team_handler_returns_not_found_for_unknown_team() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_team_router(temp.path());

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/teams/no-such-team/schedule")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"parent_task_id":"sched-parent","prompt":"review"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_schedule_team_handler_rejects_blank_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_team_router(temp.path());

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/teams/review-team/schedule")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"parent_task_id":"sched-parent","prompt":"   "}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// join_status_handler tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_join_status_handler_returns_pending_for_active_tasks() {
    let temp = tempfile::tempdir().unwrap();

    // Seed a live session task so the parent task has session tasks pending.
    let _ = delegate_one(setup_team_router(temp.path()), "join-parent", temp.path()).await;

    let router = setup_team_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/tasks/join-parent/join-status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    // With a live session task the join is still pending.
    assert_eq!(payload.get("pending"), Some(&Value::Bool(true)));
    assert_eq!(payload.get("all_done"), Some(&Value::Bool(false)));
}

#[tokio::test]
async fn test_join_status_handler_returns_all_done_for_terminal_tasks() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(
        setup_team_router(temp.path()),
        "join-done-parent",
        temp.path(),
    )
    .await;

    // Mark the session task completed so the join gate can close.
    let router = setup_team_router(temp.path());
    let patch_response = router
        .clone()
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
    assert_eq!(patch_response.status(), StatusCode::OK);

    let join_response = router
        .oneshot(
            Request::builder()
                .uri("/v1/tasks/join-done-parent/join-status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(join_response.status(), StatusCode::OK);
    let body = to_bytes(join_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload.get("pending"), Some(&Value::Bool(false)));
    assert_eq!(payload.get("all_done"), Some(&Value::Bool(true)));
}
