use crate::app::{task_graph_rollup_path, todos_rollup_path};
use crate::test_support::{ENV_LOCK, EnvRestore};

use super::phase_e::{delegate_one, setup_phase_e_router};
use super::*;
use filetime::{FileTime, set_file_mtime};
use serde_json::json;

const LARGE_SCAN_REPO_TASKS: usize = 70;
const LARGE_SCAN_LEGACY_LIVE_TASKS: usize = 20;
const LARGE_SCAN_LEGACY_COMPLETED_TASKS: usize = 20;

fn write_parent_state(
    dir: &std::path::Path,
    id: &str,
    status: &str,
    agent_id: &str,
    lifecycle_states: &[&str],
) {
    let session_tasks = lifecycle_states
        .iter()
        .enumerate()
        .map(|(index, lifecycle_state)| {
            json!({
                "id": format!("{id}-{agent_id}-{index}"),
                "parent_task_id": id,
                "agent_id": agent_id,
                "prompt": format!("task-{index}"),
                "lifecycle_state": lifecycle_state,
            })
        })
        .collect::<Vec<_>>();

    let state = json!({
        "id": id,
        "status": status,
        "active_grants": {},
        "changed_files": [],
        "command_history": [],
        "conversation_snapshot": {"message_count": 0, "summary": ""},
        "interrupted_sessions": [],
        "session_tasks": session_tasks,
    });
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
}

fn setup_large_scan_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();

    let nested = temp.path().join("src/nested");
    let repo_state_dir = temp.path().join(".vex/state");
    let legacy_state_dir = nested.join(".vex/state");
    std::fs::create_dir_all(&repo_state_dir).unwrap();
    std::fs::create_dir_all(&legacy_state_dir).unwrap();

    for index in 0..LARGE_SCAN_REPO_TASKS {
        write_parent_state(
            &repo_state_dir,
            &format!("repo-task-{index:03}"),
            "Running",
            "repo-reviewer",
            &["Pending"],
        );
    }

    for index in 0..LARGE_SCAN_LEGACY_LIVE_TASKS {
        write_parent_state(
            &legacy_state_dir,
            &format!("legacy-live-{index:03}"),
            "Running",
            "legacy-reviewer",
            &["Pending"],
        );
    }

    for index in 0..LARGE_SCAN_LEGACY_COMPLETED_TASKS {
        write_parent_state(
            &legacy_state_dir,
            &format!("legacy-done-{index:03}"),
            "Completed",
            "legacy-reviewer",
            &["Completed"],
        );
    }

    write_parent_state(
        &repo_state_dir,
        "task-dup",
        "Running",
        "repo-reviewer",
        &["Pending"],
    );
    set_file_mtime(
        repo_state_dir.join("task-dup.json"),
        FileTime::from_unix_time(1_700_000_001, 0),
    )
    .unwrap();

    write_parent_state(
        &legacy_state_dir,
        "task-dup",
        "Completed",
        "legacy-reviewer",
        &["Completed"],
    );
    set_file_mtime(
        legacy_state_dir.join("task-dup.json"),
        FileTime::from_unix_time(1_700_000_002, 0),
    )
    .unwrap();

    (temp, nested)
}

fn setup_scan_router(working_dir: &std::path::Path) -> axum::Router {
    let mut config = Config::default_for_tui();
    config.working_dir = working_dir.to_path_buf();
    build_router(config)
}

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
        "expected active session task {st_id} in /v1/todos response"
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

#[tokio::test]
async fn test_task_graph_endpoint_scans_large_state_dirs_and_prefers_newest_duplicate() {
    let _env_lock = ENV_LOCK.lock().await;
    let _state_dir = EnvRestore::capture(&_env_lock, "VEX_STATE_DIR");
    let _startup_task_scans = EnvRestore::capture(&_env_lock, "VEX_MAX_STARTUP_TASK_SCANS");
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
    crate::test_support::test_remove_var(&_env_lock, "VEX_MAX_STARTUP_TASK_SCANS");

    let (_temp, nested) = setup_large_scan_fixture();

    let response = setup_scan_router(&nested)
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
    assert_eq!(
        nodes.len(),
        LARGE_SCAN_REPO_TASKS
            + LARGE_SCAN_LEGACY_LIVE_TASKS
            + LARGE_SCAN_LEGACY_COMPLETED_TASKS
            + 1,
        "expected every unique task-state file to appear exactly once"
    );

    let duplicate = nodes
        .iter()
        .find(|node| node.get("id") == Some(&Value::String("task-dup".into())))
        .expect("expected duplicate task id in task graph");
    assert_eq!(
        duplicate.get("status"),
        Some(&Value::String("completed".into())),
        "expected newest duplicate copy to win"
    );
    let session_tasks = duplicate
        .get("session_tasks")
        .and_then(Value::as_array)
        .expect("expected session_tasks array on duplicate node");
    assert_eq!(session_tasks.len(), 1);
    assert_eq!(
        session_tasks[0].get("agent_id"),
        Some(&Value::String("legacy-reviewer".into()))
    );
    assert_eq!(
        session_tasks[0].get("lifecycle_state"),
        Some(&Value::String("completed".into()))
    );
}

#[tokio::test]
async fn test_list_todos_endpoint_scans_large_state_dirs_and_ignores_older_duplicate_live_tasks() {
    let _env_lock = ENV_LOCK.lock().await;
    let _state_dir = EnvRestore::capture(&_env_lock, "VEX_STATE_DIR");
    let _startup_task_scans = EnvRestore::capture(&_env_lock, "VEX_MAX_STARTUP_TASK_SCANS");
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
    crate::test_support::test_remove_var(&_env_lock, "VEX_MAX_STARTUP_TASK_SCANS");

    let (_temp, nested) = setup_large_scan_fixture();

    let response = setup_scan_router(&nested)
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
    assert_eq!(
        arr.len(),
        LARGE_SCAN_REPO_TASKS + LARGE_SCAN_LEGACY_LIVE_TASKS,
        "expected only active tasks from the newest copy of each parent task"
    );
    assert!(
        !arr.iter()
            .any(|item| { item.get("parent_task_id") == Some(&Value::String("task-dup".into())) }),
        "older duplicate active task should not appear when a newer completed copy exists"
    );
}

#[tokio::test]
async fn test_delegate_writes_task_graph_rollup_file() {
    let temp = tempfile::tempdir().unwrap();

    let _ = delegate_one(
        setup_phase_e_router(temp.path()),
        "snap-parent",
        temp.path(),
    )
    .await;

    let snap_path = task_graph_rollup_path(temp.path());
    assert!(
        snap_path.exists(),
        "expected task-graph.json to be written after delegate"
    );

    let raw = std::fs::read_to_string(&snap_path).unwrap();
    let val: Value = serde_json::from_str(&raw).unwrap();
    assert!(
        val.get("generated_at_ms").and_then(Value::as_u64).is_some(),
        "expected generated_at_ms in task-graph.json"
    );
    let nodes = val.get("nodes").and_then(Value::as_array).unwrap();
    assert!(
        nodes
            .iter()
            .any(|n| n.get("id") == Some(&Value::String("snap-parent".into()))),
        "expected snap-parent in persisted task-graph.json nodes"
    );
}

#[tokio::test]
async fn test_delegate_writes_todos_rollup_file() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(
        setup_phase_e_router(temp.path()),
        "todos-snap-parent",
        temp.path(),
    )
    .await;

    let snap_path = todos_rollup_path(temp.path());
    assert!(
        snap_path.exists(),
        "expected todos.json to be written after delegate"
    );

    let raw = std::fs::read_to_string(&snap_path).unwrap();
    let val: Value = serde_json::from_str(&raw).unwrap();
    let items = val.get("items").and_then(Value::as_array).unwrap();
    assert!(
        items
            .iter()
            .any(|t| t.get("id") == Some(&Value::String(st_id.clone()))),
        "expected session task {st_id} in persisted todos.json"
    );
}

#[tokio::test]
async fn test_status_update_refreshes_todos_rollup() {
    let temp = tempfile::tempdir().unwrap();

    let st_id = delegate_one(
        setup_phase_e_router(temp.path()),
        "todos-refresh-parent",
        temp.path(),
    )
    .await;

    let router = setup_phase_e_router(temp.path());
    let patch = router
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

    let snap_path = todos_rollup_path(temp.path());
    let raw = std::fs::read_to_string(&snap_path).unwrap();
    let val: Value = serde_json::from_str(&raw).unwrap();
    let items = val.get("items").and_then(Value::as_array).unwrap();
    assert!(
        !items
            .iter()
            .any(|t| t.get("id") == Some(&Value::String(st_id.clone()))),
        "completed session task {st_id} should not appear in todos.json after status update"
    );
}

#[tokio::test]
async fn test_projection_endpoint_returns_file_paths() {
    let temp = tempfile::tempdir().unwrap();

    let _ = delegate_one(
        setup_phase_e_router(temp.path()),
        "proj-endpoint-parent",
        temp.path(),
    )
    .await;

    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/projection")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        payload
            .get("task_graph_path")
            .and_then(Value::as_str)
            .map(|p| p.ends_with("task-graph.json"))
            .unwrap_or(false),
        "expected task_graph_path ending with task-graph.json"
    );
    assert!(
        payload
            .get("todos_path")
            .and_then(Value::as_str)
            .map(|p| p.ends_with("todos.json"))
            .unwrap_or(false),
        "expected todos_path ending with todos.json"
    );
}
