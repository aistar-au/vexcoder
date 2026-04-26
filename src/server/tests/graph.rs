use crate::app::{task_graph_rollup_path, todos_rollup_path};
use crate::test_support::{ENV_LOCK, EnvRestore};

use super::phase_e::{delegate_one, setup_phase_e_router};
use super::*;
use serde_json::json;

#[tokio::test]
async fn task_graph_endpoint_returns_nodes_with_session_tasks() {
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
        .expect("nodes array");
    let node = nodes
        .iter()
        .find(|n| n.get("id") == Some(&Value::String("graph-parent".into())))
        .expect("graph-parent node");
    assert!(
        node.get("session_tasks")
            .and_then(Value::as_array)
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    );
}

#[tokio::test]
async fn todos_endpoint_returns_live_session_tasks() {
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
    let todos = items.as_array().expect("todos array");
    assert!(
        todos
            .iter()
            .any(|t| t.get("id").and_then(Value::as_str) == Some(&st_id))
    );
}

#[tokio::test]
async fn delegate_writes_task_graph_and_todos_rollup_files() {
    let temp = tempfile::tempdir().unwrap();
    let _ = delegate_one(
        setup_phase_e_router(temp.path()),
        "rollup-parent",
        temp.path(),
    )
    .await;

    let graph_path = task_graph_rollup_path(temp.path());
    let todos_path = todos_rollup_path(temp.path());
    assert!(
        graph_path.exists(),
        "task-graph rollup must be written on delegate"
    );
    assert!(
        todos_path.exists(),
        "todos rollup must be written on delegate"
    );

    let graph: Value = serde_json::from_slice(&std::fs::read(&graph_path).unwrap()).unwrap();
    assert!(graph.get("nodes").and_then(Value::as_array).is_some());
}
