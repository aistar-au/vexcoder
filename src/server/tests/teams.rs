use super::phase_e::delegate_one;
use super::*;
use tokio::sync::Barrier;

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

fn agents_toml_with_multi_member_team() -> &'static str {
    r#"
[[agents]]
name = "reviewer"
isolation = "worktree"
max_parallel_tasks = 2

[[agents]]
name = "planner"
isolation = "worktree"
max_parallel_tasks = 2

[[agents]]
name = "tester"
isolation = "worktree"
max_parallel_tasks = 2

[[teams]]
name = "review-team"
scheduler = "fan_out_join"
members = ["reviewer", "planner", "tester"]
"#
}

fn setup_multi_member_team_router(temp: &std::path::Path) -> axum::Router {
    std::fs::create_dir_all(temp.join(".vex")).unwrap();
    std::fs::write(
        temp.join(".vex/agents.toml"),
        agents_toml_with_multi_member_team(),
    )
    .unwrap();
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_schedule_team_handler_enforces_max_parallel_tasks_under_parallel_requests() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_team_router(temp.path());

    let worker_count = 8;
    let barrier = std::sync::Arc::new(Barrier::new(worker_count));
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let barrier = barrier.clone();
        let router = router.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let response = router
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/teams/review-team/schedule")
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            r#"{"parent_task_id":"sched-race","prompt":"review the code"}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            (status, serde_json::from_slice::<Value>(&body).unwrap())
        }));
    }

    let mut ok = 0;
    let mut conflicts = 0;
    for handle in handles {
        let (status, payload) = handle.await.unwrap();
        match status {
            StatusCode::OK => {
                ok += 1;
                assert_eq!(payload.get("ok"), Some(&Value::Bool(true)));
            }
            StatusCode::CONFLICT => {
                conflicts += 1;
                assert_eq!(
                    payload.get("type"),
                    Some(&Value::String(
                        "https://aistar-au.github.io/vexcoder/problems/concurrency_limit_reached"
                            .into()
                    ))
                );
            }
            other => panic!("unexpected schedule_team status: {other}"),
        }
    }

    assert_eq!(ok, 4, "expected max_parallel_tasks successful schedules");
    assert_eq!(
        conflicts,
        worker_count - ok,
        "expected remaining schedules to hit the concurrency cap"
    );

    let todos_response = setup_team_router(temp.path())
        .oneshot(
            Request::builder()
                .uri("/v1/todos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(todos_response.status(), StatusCode::OK);
    let body = to_bytes(todos_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let todos: Value = serde_json::from_slice(&body).unwrap();
    let arr = todos.as_array().expect("expected array from /v1/todos");
    assert_eq!(
        arr.len(),
        ok,
        "expected one active task per successful schedule"
    );
    assert!(
        arr.iter().all(|task| {
            task.get("parent_task_id") == Some(&Value::String("sched-race".into()))
        })
    );
}

#[tokio::test]
async fn test_join_status_handler_returns_pending_for_active_tasks() {
    let temp = tempfile::tempdir().unwrap();

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_join_status_handler_stays_pending_until_all_team_members_complete() {
    let temp = tempfile::tempdir().unwrap();
    let router = setup_multi_member_team_router(temp.path());

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/teams/review-team/schedule")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"parent_task_id":"join-multi","prompt":"review the code"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    let session_task_ids = payload
        .get("session_task_ids")
        .and_then(Value::as_array)
        .expect("expected session_task_ids array")
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        session_task_ids.len(),
        3,
        "expected one task per team member"
    );

    let complete_first = router.clone().oneshot(
        Request::builder()
            .method("PATCH")
            .uri(format!("/v1/session-tasks/{}/status", session_task_ids[0]))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"status":"completed"}"#))
            .unwrap(),
    );
    let complete_second = router.clone().oneshot(
        Request::builder()
            .method("PATCH")
            .uri(format!("/v1/session-tasks/{}/status", session_task_ids[1]))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"status":"completed"}"#))
            .unwrap(),
    );
    let (first_response, second_response) = tokio::join!(complete_first, complete_second);
    assert_eq!(first_response.unwrap().status(), StatusCode::OK);
    assert_eq!(second_response.unwrap().status(), StatusCode::OK);

    let pending_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/tasks/join-multi/join-status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pending_response.status(), StatusCode::OK);
    let body = to_bytes(pending_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let pending_payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(pending_payload.get("pending"), Some(&Value::Bool(true)));
    assert_eq!(pending_payload.get("all_done"), Some(&Value::Bool(false)));

    let final_patch = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/session-tasks/{}/status", session_task_ids[2]))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"status":"completed"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(final_patch.status(), StatusCode::OK);

    let done_response = router
        .oneshot(
            Request::builder()
                .uri("/v1/tasks/join-multi/join-status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(done_response.status(), StatusCode::OK);
    let body = to_bytes(done_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let done_payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(done_payload.get("pending"), Some(&Value::Bool(false)));
    assert_eq!(done_payload.get("all_done"), Some(&Value::Bool(true)));
    assert_eq!(done_payload.get("completed"), Some(&Value::from(3)));
}
