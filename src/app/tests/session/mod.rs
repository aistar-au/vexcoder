use super::*;

mod compact;
mod fork;
mod permissions;
mod runtime;

#[test]
fn new_command_saves_state_resets_transcript_and_creates_fresh_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();

    mode.on_user_input("/new".to_string(), &mut ctx);

    assert!(
        temp.path().join(format!("{original_id}.json")).exists(),
        "/new must save prior task"
    );
    assert_ne!(
        mode.current_task_id(),
        original_id,
        "/new must create fresh task-id"
    );
    assert!(!mode.is_pulse_in_progress());
    assert!(mode.history_lines()[0].starts_with("[new session: task-"));

    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn resume_restores_active_grants_and_rejects_unknown_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut state = TaskState::new("resume-test-01".to_string());
    state.active_grants.insert(
        crate::runtime::Capability::ReadFile,
        crate::runtime::ApprovalScope::Session,
    );
    state.save(temp.path()).unwrap();

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume resume-test-01".to_string(), &mut ctx);
    assert_eq!(mode.current_task_id(), "resume-test-01");
    assert!(
        mode.task_doc
            .info
            .active_grants
            .contains_key(&crate::runtime::Capability::ReadFile)
    );

    mode.on_user_input("/resume no-such-task-99".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("not found") || l.contains("error") || l.contains("unknown")),
        "unknown id must emit error; lines: {:?}",
        mode.history_lines()
    );

    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn quit_command_requests_quit() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/quit".to_string(), &mut ctx);
    assert!(mode.quit_requested(), "/quit must set quit flag");

    let mut mode2 = TuiMode::new();
    let mut ctx2 = setup_ctx();
    mode2.on_user_input("/exit".to_string(), &mut ctx2);
    assert!(mode2.quit_requested(), "/exit must be alias for /quit");
}
