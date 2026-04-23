use super::*;
use crate::runtime::SessionTask;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

fn write_agents_toml(dir: &std::path::Path, content: &str) {
    let vex_dir = dir.join(".vex");
    std::fs::create_dir_all(&vex_dir).unwrap();
    std::fs::write(vex_dir.join("agents.toml"), content).unwrap();
}

fn env_lock() -> crate::test_support::EnvLockGuard<'static> {
    crate::test_support::ENV_LOCK.blocking_lock()
}

fn seed_parent_task(dir: &std::path::Path, parent_id: &str, agent_ids: &[&str]) -> TaskState {
    std::fs::create_dir_all(dir.join(".git")).unwrap();

    let state_dir = TaskState::state_dir_from(dir);
    std::fs::create_dir_all(&state_dir).unwrap();

    let mut parent = TaskState::new(parent_id.to_string());
    for agent_id in agent_ids {
        parent.add_session_task(SessionTask::new(
            parent_id,
            *agent_id,
            format!("prompt for {agent_id}"),
            None,
        ));
    }
    parent.save(&state_dir).unwrap();
    parent
}

#[test]
fn delegate_rejects_prompt_exceeding_max_bytes() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"worker\"\nisolation = \"shared\"\nmax_parallel_tasks = 2\n",
    );

    let long_prompt = "x".repeat(MAX_DELEGATE_PROMPT_BYTES + 1);
    let result = facade_delegate_session_task(
        dir.path(),
        Some("parent-1".to_string()),
        "worker",
        &long_prompt,
    );

    assert!(
        matches!(result, Err(DelegateError::PromptTooLong)),
        "expected PromptTooLong, got: {result:?}"
    );
}

#[test]
fn delegate_enforces_max_parallel_tasks() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"worker\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n",
    );

    let state_dir = TaskState::state_dir_from(dir.path());
    std::fs::create_dir_all(&state_dir).unwrap();
    let mut parent = TaskState::new("parent-seed".to_string());
    let mut st = SessionTask::new("parent-seed", "worker", "already running", None);

    st.worktree_path = Some(PathBuf::from("/tmp/dummy"));
    parent.add_session_task(st);
    parent.save(&state_dir).unwrap();

    let result = facade_delegate_session_task(
        dir.path(),
        Some("parent-1".to_string()),
        "worker",
        "new work",
    );

    assert!(
        matches!(result, Err(DelegateError::ConcurrencyLimitReached)),
        "expected ConcurrencyLimitReached, got: {result:?}"
    );
}

#[test]
fn delegate_enforces_max_parallel_tasks_under_parallel_calls() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"worker\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n",
    );

    let _race_hook = install_delegate_race_hook(Arc::new(|| {
        thread::sleep(Duration::from_millis(150));
    }));

    let worker_count = 8;
    let barrier = Arc::new(Barrier::new(worker_count));
    let working_dir = dir.path().to_path_buf();
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let barrier = Arc::clone(&barrier);
        let working_dir = working_dir.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            facade_delegate_session_task(
                &working_dir,
                Some("parent-race".to_string()),
                "worker",
                "inspect docs",
            )
        }));
    }

    let mut successes = 0;
    let mut limit_rejections = 0;
    for handle in handles {
        match handle.join().unwrap() {
            Ok(_) => successes += 1,
            Err(DelegateError::ConcurrencyLimitReached) => limit_rejections += 1,
            Err(other) => panic!("unexpected delegate result: {other:?}"),
        }
    }

    assert_eq!(
        successes, 1,
        "expected exactly one successful delegate call"
    );
    assert_eq!(
        limit_rejections,
        worker_count - 1,
        "expected remaining delegates to be rejected by the concurrency cap"
    );
}

#[test]
fn release_transitions_live_task_to_completed_and_drops_lease() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();

    let state_dir = TaskState::state_dir_from(dir.path());
    std::fs::create_dir_all(&state_dir).unwrap();

    let mut parent = TaskState::new("parent-rel".to_string());
    let mut st = SessionTask::new("parent-rel", "reviewer", "review docs", None);
    let st_id = st.id.clone();
    let lease_manager = WorktreeLeaseManager::new(&state_dir);
    let lease = lease_manager
        .lease_for_task(&st_id, Some("parent-rel"))
        .unwrap();
    st.worktree_path = Some(lease.path.clone());
    parent.add_session_task(st);
    parent.save(&state_dir).unwrap();

    let released = facade_release_session_task(dir.path(), &st_id).unwrap();
    assert!(released, "expected released = true");

    let reloaded = TaskState::load(&state_dir, "parent-rel").unwrap();
    let task = reloaded.session_task(&st_id).unwrap();
    assert_eq!(task.lifecycle_state, SessionTaskStatus::Completed);
    assert!(!lease.path.exists(), "expected lease path to be removed");
    assert!(
        lease_manager.list().unwrap().is_empty(),
        "expected lease metadata to be removed"
    );
}

#[test]
fn release_returns_false_for_unknown_session_task_id() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::create_dir_all(TaskState::state_dir_from(dir.path())).unwrap();

    let result = facade_release_session_task(dir.path(), "nonexistent-task-id").unwrap();
    assert!(!result, "expected released = false for unknown id");
}

#[test]
fn watch_rollup_formats_parent_task_status_with_display_names() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let state_dir = TaskState::state_dir_from(dir.path());
    std::fs::create_dir_all(&state_dir).unwrap();

    let mut parent = TaskState::new("parent-watch".to_string());
    parent.status = crate::runtime::TaskStatus::AwaitingApproval;
    parent.save(&state_dir).unwrap();

    let snapshot = facade_watch_rollup(dir.path(), "parent-watch")
        .unwrap()
        .expect("expected parent-task rollup");

    assert_eq!(snapshot.kind, "task");
    assert_eq!(snapshot.status, "awaiting_approval");
}

#[test]
fn schedule_team_rejects_prompt_exceeding_max_bytes() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 2\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out\"\nagents = [\"coder\"]\n",
    );

    let long_prompt = "x".repeat(MAX_DELEGATE_PROMPT_BYTES + 1);
    let result = facade_schedule_team(dir.path(), "parent-1", "review", &long_prompt);

    assert!(
        matches!(result, Err(ScheduleTeamError::PromptTooLong)),
        "expected PromptTooLong, got: {result:?}"
    );
}

#[test]
fn schedule_team_rejects_empty_parent_task_id() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let result = facade_schedule_team(dir.path(), "", "review", "do something");
    assert!(
        matches!(result, Err(ScheduleTeamError::ParentTaskIdRequired)),
        "expected ParentTaskIdRequired, got: {result:?}"
    );
}

#[test]
fn schedule_team_rejects_empty_prompt() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let result = facade_schedule_team(dir.path(), "parent-1", "review", "");
    assert!(
        matches!(result, Err(ScheduleTeamError::PromptRequired)),
        "expected PromptRequired, got: {result:?}"
    );

    let blank_result = facade_schedule_team(dir.path(), "parent-1", "review", "   ");
    assert!(
        matches!(blank_result, Err(ScheduleTeamError::PromptRequired)),
        "expected PromptRequired for blank prompt, got: {blank_result:?}"
    );
}

#[test]
fn schedule_team_enforces_max_parallel_tasks() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out_join\"\nmembers = [\"coder\"]\n",
    );

    let state_dir = TaskState::state_dir_from(dir.path());
    std::fs::create_dir_all(&state_dir).unwrap();
    let mut parent = TaskState::new("parent-seed".to_string());
    parent.add_session_task(SessionTask::new(
        "parent-seed",
        "coder",
        "already running",
        None,
    ));
    parent.save(&state_dir).unwrap();

    let result = facade_schedule_team(dir.path(), "parent-1", "review", "new work");

    assert!(
        matches!(result, Err(ScheduleTeamError::ConcurrencyLimitReached)),
        "expected ConcurrencyLimitReached, got: {result:?}"
    );
}

#[test]
fn schedule_team_enforces_max_parallel_tasks_under_parallel_calls() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out_join\"\nmembers = [\"coder\"]\n",
    );

    let _race_hook = install_delegate_race_hook(Arc::new(|| {
        thread::sleep(Duration::from_millis(150));
    }));

    let worker_count = 8;
    let barrier = Arc::new(Barrier::new(worker_count));
    let working_dir = dir.path().to_path_buf();
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let barrier = Arc::clone(&barrier);
        let working_dir = working_dir.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            facade_schedule_team(&working_dir, "parent-race", "review", "inspect docs")
        }));
    }

    let mut successes = 0;
    let mut limit_rejections = 0;
    for handle in handles {
        match handle.join().unwrap() {
            Ok(_) => successes += 1,
            Err(ScheduleTeamError::ConcurrencyLimitReached) => limit_rejections += 1,
            Err(other) => panic!("unexpected schedule_team result: {other:?}"),
        }
    }

    assert_eq!(
        successes, 1,
        "expected exactly one successful schedule_team call"
    );
    assert_eq!(
        limit_rejections,
        worker_count - 1,
        "expected remaining schedule_team calls to be rejected by the concurrency cap"
    );
}

#[test]
fn list_agents_exposes_max_parallel_tasks() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"coder\"\nisolation = \"worktree\"\nmax_parallel_tasks = 3\n",
    );

    let listing = facade_list_agents(dir.path()).unwrap();
    assert!(listing.available, "listing should be available");
    let agent = &listing.agents[0];
    assert_eq!(agent.name, "coder");
    assert_eq!(agent.max_parallel_tasks, 3);
    assert_eq!(agent.isolation, "worktree");
}

#[test]
fn list_agents_normalizes_team_scheduler_to_snake_case() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"a\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"t\"\nscheduler = \"fan_out_join\"\nmembers = [\"a\"]\n",
    );

    let listing = facade_list_agents(dir.path()).unwrap();
    assert_eq!(listing.teams[0].scheduler, "fan_out_join");
}

#[test]
fn schedule_team_normalizes_scheduler_to_snake_case() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    write_agents_toml(
        dir.path(),
        "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out_join\"\nmembers = [\"coder\"]\n",
    );

    let result = facade_schedule_team(dir.path(), "parent-1", "review", "inspect docs")
        .expect("team scheduling should succeed");

    assert_eq!(result.scheduler, "fan_out_join");
}

#[test]
fn schedule_team_returns_internal_for_unknown_member_reference() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    let vex_dir = dir.path().join(".vex");
    std::fs::create_dir_all(&vex_dir).unwrap();
    std::fs::write(
        vex_dir.join("agents.toml"),
        "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out_join\"\nmembers = [\"missing\"]\n",
    )
    .unwrap();

    let result = facade_schedule_team(dir.path(), "parent-1", "review", "inspect docs");

    match result {
        Err(ScheduleTeamError::Internal(error)) => {
            let message = error.to_string();
            assert!(
                message.contains("unknown agent"),
                "unexpected internal error message: {message}"
            );
        }
        other => panic!("expected Internal error, got: {other:?}"),
    }
}

#[test]
fn post_peer_message_rejects_unknown_sender() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    seed_parent_task(dir.path(), "parent-peer", &["reviewer"]);

    let result = facade_post_peer_message(
        dir.path(),
        "parent-peer",
        "missing-session-task",
        "reviewer",
        "*",
        "observation",
        "message",
    );

    assert!(matches!(result, Err(PeerChannelError::SenderNotInTask)));
}

#[test]
fn post_peer_message_rejects_content_too_long() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let parent = seed_parent_task(dir.path(), "parent-peer", &["reviewer"]);
    let sender_id = parent.session_tasks[0].id.clone();
    let long_content = "x".repeat(peer_channel::MAX_PEER_MESSAGE_BYTES + 1);

    let result = facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &sender_id,
        "reviewer",
        "*",
        "observation",
        &long_content,
    );

    assert!(matches!(result, Err(PeerChannelError::ContentTooLong)));
}

#[test]
fn post_peer_message_rejects_invalid_kind() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let parent = seed_parent_task(dir.path(), "parent-peer", &["reviewer"]);
    let sender_id = parent.session_tasks[0].id.clone();

    let result = facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &sender_id,
        "reviewer",
        "*",
        "invalid",
        "message",
    );

    assert!(matches!(result, Err(PeerChannelError::InvalidKind)));
}

#[test]
fn post_peer_message_uses_saved_sender_agent_id() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let parent = seed_parent_task(dir.path(), "parent-peer", &["reviewer"]);
    let sender_task = &parent.session_tasks[0];

    let message = facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &sender_task.id,
        "spoofed-agent",
        "*",
        "observation",
        "message",
    )
    .unwrap();

    assert_eq!(message.sender_agent_id, "reviewer");
}

#[test]
fn read_peer_messages_returns_empty_before_first_post() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    seed_parent_task(dir.path(), "parent-peer", &["reviewer"]);

    let messages =
        facade_read_peer_messages(dir.path(), "parent-peer", 0, Some("reviewer")).unwrap();

    assert!(messages.is_empty());
}

#[test]
fn post_peer_message_delivers_to_broadcast_and_targeted_reader() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let parent = seed_parent_task(dir.path(), "parent-peer", &["reviewer", "fixer"]);
    let reviewer_id = parent.session_tasks[0].id.clone();

    facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &reviewer_id,
        "reviewer",
        "*",
        "observation",
        "broadcast",
    )
    .unwrap();
    facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &reviewer_id,
        "reviewer",
        "reviewer",
        "correction",
        "targeted",
    )
    .unwrap();
    facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &reviewer_id,
        "reviewer",
        "fixer",
        "question",
        "other-agent",
    )
    .unwrap();

    let reviewer_messages =
        facade_read_peer_messages(dir.path(), "parent-peer", 0, Some("reviewer")).unwrap();

    assert_eq!(reviewer_messages.len(), 2);
    assert_eq!(reviewer_messages[0].content, "broadcast");
    assert_eq!(reviewer_messages[1].content, "targeted");
}

#[test]
fn read_peer_messages_respects_after_ms_cursor() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let parent = seed_parent_task(dir.path(), "parent-peer", &["reviewer"]);
    let sender_id = parent.session_tasks[0].id.clone();

    let first = facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &sender_id,
        "reviewer",
        "*",
        "observation",
        "first",
    )
    .unwrap();
    thread::sleep(Duration::from_millis(2));
    let second = facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &sender_id,
        "reviewer",
        "*",
        "observation",
        "second",
    )
    .unwrap();

    assert!(second.sent_at > first.sent_at);

    let messages =
        facade_read_peer_messages(dir.path(), "parent-peer", first.sent_at, Some("reviewer"))
            .unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "second");
}

#[test]
fn post_peer_message_returns_channel_full_at_depth_cap() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let parent = seed_parent_task(dir.path(), "parent-peer", &["reviewer"]);
    let sender_id = parent.session_tasks[0].id.clone();

    for index in 0..peer_channel::MAX_CHANNEL_DEPTH {
        let result = facade_post_peer_message(
            dir.path(),
            "parent-peer",
            &sender_id,
            "reviewer",
            "*",
            "observation",
            &format!("message-{index}"),
        );
        assert!(result.is_ok(), "expected capacity slot {index} to succeed");
    }

    let result = facade_post_peer_message(
        dir.path(),
        "parent-peer",
        &sender_id,
        "reviewer",
        "*",
        "observation",
        "overflow",
    );

    assert!(matches!(result, Err(PeerChannelError::ChannelFull)));
}
