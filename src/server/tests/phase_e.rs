use super::*;

pub(super) fn agents_toml_shared() -> &'static str {
    r#"
[[agents]]
name = "reviewer"
isolation = "worktree"
max_parallel_tasks = 2
"#
}

pub(super) fn setup_phase_e_router(temp: &std::path::Path) -> axum::Router {
    std::fs::create_dir_all(temp.join(".vex")).unwrap();
    std::fs::write(temp.join(".vex/agents.toml"), agents_toml_shared()).unwrap();
    let mut config = Config::default_for_tui();
    config.working_dir = temp.to_path_buf();
    build_router(config)
}

pub(super) async fn delegate_one(
    router: axum::Router,
    parent_id: &str,
    temp: &std::path::Path,
) -> String {
    
    
    let _ = temp; 
    let body =
        format!(r#"{{"parent_task_id":"{parent_id}","agent_id":"reviewer","prompt":"task"}}"#);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/delegate")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "delegate failed");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap();
    val.get("session_task_id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn test_list_tasks_returns_parent_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_phase_e_router(temp.path());

    
    let st_id = delegate_one(
        setup_phase_e_router(temp.path()),
        "list-parent",
        temp.path(),
    )
    .await;
    let _ = st_id;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let tasks: Value = serde_json::from_slice(&body).unwrap();
    let arr = tasks.as_array().unwrap();
    assert!(
        arr.iter()
            .any(|t| t.get("id") == Some(&Value::String("list-parent".into()))),
        "expected list-parent in /v1/tasks response"
    );
}

#[tokio::test]
async fn test_list_session_tasks_returns_all_session_tasks() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(setup_phase_e_router(temp.path()), "lst-parent", temp.path()).await;

    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/session-tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let tasks: Value = serde_json::from_slice(&body).unwrap();
    let arr = tasks.as_array().unwrap();
    assert!(
        arr.iter()
            .any(|t| t.get("id") == Some(&Value::String(st_id.clone()))),
        "expected session task {st_id} in /v1/session-tasks response"
    );
}

#[tokio::test]
async fn test_get_session_task_returns_detail() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(setup_phase_e_router(temp.path()), "get-parent", temp.path()).await;

    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri(format!("/v1/session-tasks/{st_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let snap: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(snap.get("id"), Some(&Value::String(st_id.clone())));
    assert_eq!(
        snap.get("agent_id"),
        Some(&Value::String("reviewer".into()))
    );
    assert_eq!(
        snap.get("parent_task_id"),
        Some(&Value::String("get-parent".into()))
    );
}

#[tokio::test]
async fn test_get_session_task_returns_not_found_for_unknown_id() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/session-tasks/no-such-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_update_session_task_status_transitions_to_running() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(setup_phase_e_router(temp.path()), "upd-parent", temp.path()).await;

    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/session-tasks/{st_id}/status"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"status":"running"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let snap: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        snap.get("lifecycle_state"),
        Some(&Value::String("running".into()))
    );
}

#[tokio::test]
async fn test_update_session_task_status_rejects_invalid_status_string() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(setup_phase_e_router(temp.path()), "inv-parent", temp.path()).await;

    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/session-tasks/{st_id}/status"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"status":"bogus"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_update_session_task_status_rejects_transition_from_terminal() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(
        setup_phase_e_router(temp.path()),
        "term-parent",
        temp.path(),
    )
    .await;

    
    let release_router = setup_phase_e_router(temp.path());
    release_router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/session-tasks/{st_id}/release"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    
    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/session-tasks/{st_id}/status"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"status":"running"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}
