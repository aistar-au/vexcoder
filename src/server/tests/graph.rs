use super::phase_e::{delegate_one, setup_phase_e_router};
use super::*;

#[tokio::test]
async fn test_task_graph_endpoint_returns_nodes() {
    let temp = tempfile::tempdir().unwrap();

    let _ = delegate_one(
        setup_phase_e_router(temp.path()),
        "graph-parent",
        temp.path(),
    )
    .await;

    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/task-graph")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    let nodes = payload
        .get("nodes")
        .and_then(Value::as_array)
        .expect("expected nodes array");
    assert!(
        nodes
            .iter()
            .any(|n| n.get("id") == Some(&Value::String("graph-parent".into()))),
        "expected graph-parent node in task graph"
    );
    // Each node should carry a session_tasks array.
    let node = nodes
        .iter()
        .find(|n| n.get("id") == Some(&Value::String("graph-parent".into())))
        .unwrap();
    assert!(
        node.get("session_tasks")
            .and_then(Value::as_array)
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "expected at least one session_task inside graph-parent node"
    );
}

#[tokio::test]
async fn test_list_todos_endpoint_returns_live_tasks() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(
        setup_phase_e_router(temp.path()),
        "todos-parent",
        temp.path(),
    )
    .await;

    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/todos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let items: Value = serde_json::from_slice(&body).unwrap();
    let arr = items.as_array().expect("expected array from /v1/todos");
    assert!(
        arr.iter()
            .any(|t| t.get("id") == Some(&Value::String(st_id.clone()))),
        "expected live session task {st_id} in /v1/todos response"
    );
}

#[tokio::test]
async fn test_list_todos_endpoint_excludes_completed_tasks() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(
        setup_phase_e_router(temp.path()),
        "todos-done-parent",
        temp.path(),
    )
    .await;

    // Complete the session task.
    let router = setup_phase_e_router(temp.path());
    let patch = router
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
    assert_eq!(patch.status(), StatusCode::OK);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/todos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let items: Value = serde_json::from_slice(&body).unwrap();
    let arr = items.as_array().expect("expected array from /v1/todos");
    assert!(
        !arr.iter()
            .any(|t| t.get("id") == Some(&Value::String(st_id.clone()))),
        "completed session task {st_id} should not appear in /v1/todos"
    );
}
