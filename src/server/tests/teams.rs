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
async fn schedule_team_handler_returns_session_task_ids() {
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
    let ids = payload
        .get("session_task_ids")
        .and_then(Value::as_array)
        .expect("session_task_ids");
    assert!(!ids.is_empty());
}

#[tokio::test]
async fn schedule_team_returns_not_found_for_unknown_team_and_rejects_blank_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_team_router(temp.path());

    let not_found = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/teams/no-such-team/schedule")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"parent_task_id":"p","prompt":"review"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);

    let bad_prompt = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/teams/review-team/schedule")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"parent_task_id":"p","prompt":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_prompt.status(), StatusCode::BAD_REQUEST);
}
