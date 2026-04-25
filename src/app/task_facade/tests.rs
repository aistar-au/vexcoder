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
        parent.add_session_task(SessionTask::new(parent_id, *agent_id, format!("prompt for {agent_id}"), None));
    }
    parent.save(&state_dir).unwrap();
    parent
}

#[test]
fn delegate_rejects_prompt_exceeding_max_bytes() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(dir.path(), "[[agents]]\nname = \"worker\"\nisolation = \"shared\"\nmax_parallel_tasks = 2\n");
    let long_prompt = "x".repeat(MAX_DELEGATE_PROMPT_BYTES + 1);
    let result = facade_delegate_session_task(dir.path(), Some("parent-1".to_string()), "worker", &long_prompt);
    assert!(matches!(result, Err(DelegateError::PromptTooLong)));
}

#[test]
fn delegate_enforces_max_parallel_tasks() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    write_agents_toml(dir.path(), "[[agents]]\nname = \"worker\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n");
    let state_dir = TaskState::state_dir_from(dir.path());
    std::fs::create_dir_all(&state_dir).unwrap();
    let mut parent = TaskState::new("parent-seed".to_string());
    let mut st = SessionTask::new("parent-seed", "worker", "already running", None);
    st.worktree_path = Some(PathBuf::from("/tmp/dummy"));
    parent.add_session_task(st);
    parent.save(&state_dir).unwrap();
    let result = facade_delegate_session_task(dir.path(), Some("parent-1".to_string()), "worker", "new work");
    assert!(matches!(result, Err(DelegateError::ConcurrencyLimitReached)));
}

#[test]
fn post_peer_message_validates_sender_and_content() {
    let _env_lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let parent = seed_parent_task(dir.path(), "parent-peer", &["reviewer"]);
    let sender_id = parent.session_tasks[0].id.clone();

    assert!(matches!(
        facade_post_peer_message(dir.path(), "parent-peer", "missing-session-task", "reviewer", "*", "observation", "message"),
        Err(PeerChannelError::SenderNotInTask)
    ));

    let long_content = "x".repeat(peer_channel::MAX_PEER_MESSAGE_BYTES + 1);
    assert!(matches!(
        facade_post_peer_message(dir.path(), "parent-peer", &sender_id, "reviewer", "*", "observation", &long_content),
        Err(PeerChannelError::ContentTooLong)
    ));

    assert!(matches!(
        facade_post_peer_message(dir.path(), "parent-peer", &sender_id, "reviewer", "*", "invalid", "message"),
        Err(PeerChannelError::InvalidKind)
    ));
}
