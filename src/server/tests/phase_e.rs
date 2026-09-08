use super::*;
use crate::runtime::{TaskState, WorkingSetRecord};

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
    _temp: &std::path::Path,
) -> String {
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
    serde_json::from_slice::<Value>(&bytes)
        .unwrap()
        .get("session_task_id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn list_tasks_returns_parent_and_session_task_endpoints() {
    let temp = tempfile::tempdir().unwrap();
    let st_id = delegate_one(
        setup_phase_e_router(temp.path()),
        "list-parent",
        temp.path(),
    )
    .await;

    let tasks_body = to_bytes(
        setup_phase_e_router(temp.path())
            .oneshot(
                Request::builder()
                    .uri("/v1/tasks")
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
    let tasks: Value = serde_json::from_slice(&tasks_body).unwrap();
    assert!(
        tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t.get("id") == Some(&Value::String("list-parent".into())))
    );

    let sts_body = to_bytes(
        setup_phase_e_router(temp.path())
            .oneshot(
                Request::builder()
                    .uri("/v1/session-tasks")
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
    let sts: Value = serde_json::from_slice(&sts_body).unwrap();
    assert!(
        sts.as_array()
            .unwrap()
            .iter()
            .any(|t| t.get("id") == Some(&Value::String(st_id.clone())))
    );
}

#[tokio::test]
async fn working_set_route_returns_envelope_and_404_when_missing() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    let state_dir = TaskState::state_dir_from(temp.path());
    std::fs::create_dir_all(&state_dir).unwrap();
    TaskState::new("env-parent".to_string())
        .save(&state_dir)
        .unwrap();
    WorkingSetRecord::new("keep the objective")
        .save(&state_dir, "env-parent")
        .unwrap();

    let router = setup_phase_e_router(temp.path());
    let found = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/tasks/env-parent/working-set")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(found.status(), StatusCode::OK);
    let payload: Value =
        serde_json::from_slice(&to_bytes(found.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(
        payload.get("task_id"),
        Some(&Value::String("env-parent".into()))
    );
    assert_eq!(
        payload
            .pointer("/working_set/objective")
            .and_then(Value::as_str),
        Some("keep the objective")
    );

    let missing = router
        .oneshot(
            Request::builder()
                .uri("/v1/tasks/missing/working-set")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn session_task_crud_get_and_status_update() {
    let temp = tempfile::tempdir().unwrap();
    let st_id = delegate_one(setup_phase_e_router(temp.path()), "get-parent", temp.path()).await;

    let snap_body = to_bytes(
        setup_phase_e_router(temp.path())
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/session-tasks/{st_id}"))
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
    let snap: Value = serde_json::from_slice(&snap_body).unwrap();
    assert_eq!(snap.get("id"), Some(&Value::String(st_id.clone())));
    assert_eq!(
        snap.get("agent_id"),
        Some(&Value::String("reviewer".into()))
    );

    let nf = setup_phase_e_router(temp.path())
        .oneshot(
            Request::builder()
                .uri("/v1/session-tasks/no-such-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(nf.status(), StatusCode::NOT_FOUND);

    let patch_body = to_bytes(
        setup_phase_e_router(temp.path())
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/v1/session-tasks/{st_id}/status"))
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"status":"running"}"#))
                    .unwrap(),
            )
            .await
            .unwrap()
            .into_body(),
        usize::MAX,
    )
    .await
    .unwrap();
    let patched: Value = serde_json::from_slice(&patch_body).unwrap();
    assert_eq!(
        patched.get("lifecycle_state"),
        Some(&Value::String("running".into()))
    );

    let bad = setup_phase_e_router(temp.path())
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
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
}
