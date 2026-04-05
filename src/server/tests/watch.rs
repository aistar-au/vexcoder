use super::phase_e::{delegate_one, setup_phase_e_router};
use super::*;

#[tokio::test]
async fn test_watch_session_task_stream_returns_not_found_for_unknown_id() {
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

/// A session task that has already reached a final state causes the watch
/// stream to emit exactly one snapshot event and then close, allowing the test
/// to read the full body without an unbounded wait.
#[tokio::test]
async fn test_watch_session_task_stream_emits_rollup_and_terminates_on_terminal() {
    let temp = tempfile::tempdir().unwrap();

    // Create a session task and immediately transition it to a final state.
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

    // The watch stream should emit the "completed" snapshot then close.
    let router = setup_phase_e_router(temp.path());
    let response = router
        .oneshot(
            Request::builder()
                .uri(format!("/v1/session-tasks/{st_id}/watch"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let raw = timeout(
        Duration::from_secs(5),
        to_bytes(response.into_body(), usize::MAX),
    )
    .await
    .expect("watch stream did not close within 5 seconds")
    .unwrap();

    let body_str = String::from_utf8_lossy(&raw);
    assert!(
        body_str.contains(&st_id),
        "expected session task id {st_id} in watch stream body"
    );
    assert!(
        body_str.contains("completed"),
        "expected lifecycle_state completed in watch stream body"
    );
}

#[tokio::test]
async fn test_watch_session_task_stream_broadcasts_live_update_without_poll_delay() {
    let temp = tempfile::tempdir().unwrap();

    let router = setup_phase_e_router(temp.path());
    let st_id = delegate_one(router.clone(), "watch-live-parent", temp.path()).await;

    let watch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/session-tasks/{st_id}/watch"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(watch_response.status(), StatusCode::OK);

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

    let raw = timeout(
        Duration::from_millis(500),
        to_bytes(watch_response.into_body(), usize::MAX),
    )
    .await
    .expect("watch stream did not receive broadcast update within 500ms")
    .unwrap();

    let body_str = String::from_utf8_lossy(&raw);
    assert!(
        body_str.contains("pending"),
        "expected initial lifecycle_state pending in watch stream body"
    );
    assert!(
        body_str.contains("completed"),
        "expected lifecycle_state completed in watch stream body"
    );
}

#[tokio::test]
async fn test_watch_session_task_stream_emits_release_update_with_last_known_worktree_path() {
    let temp = tempfile::tempdir().unwrap();

    let router = setup_phase_e_router(temp.path());
    let st_id = delegate_one(router.clone(), "watch-release-parent", temp.path()).await;

    let watch_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/session-tasks/{st_id}/watch"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(watch_response.status(), StatusCode::OK);

    let release_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/session-tasks/{st_id}/release"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(release_response.status(), StatusCode::OK);

    let raw = timeout(
        Duration::from_secs(2),
        to_bytes(watch_response.into_body(), usize::MAX),
    )
    .await
    .expect("watch stream did not receive release update within 2 seconds")
    .unwrap();

    let body_str = String::from_utf8_lossy(&raw);
    assert!(
        body_str.contains("pending"),
        "expected initial lifecycle_state pending in watch stream body"
    );
    assert!(
        body_str.contains("completed"),
        "expected completed lifecycle_state after release"
    );

    let events = body_str
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|line| serde_json::from_str::<Value>(line.trim()).unwrap())
        .collect::<Vec<_>>();
    assert!(
        events.len() >= 2,
        "expected at least two session_task events"
    );
    assert!(
        events[0].get("worktree_path").is_some(),
        "expected initial watch event to include worktree_path"
    );
    let final_event = events.last().unwrap();
    assert_eq!(
        final_event.get("lifecycle_state"),
        Some(&Value::String("completed".into()))
    );
    assert!(
        final_event.get("worktree_path").is_some(),
        "expected release update to preserve the last known worktree_path"
    );
}
