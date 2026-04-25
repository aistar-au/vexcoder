use super::*;

mod compact;
mod fork;
mod permissions;
mod runtime;

#[test]
fn test_tui_new_saves_current_state_before_reset() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();

    mode.on_user_input("/new".to_string(), &mut ctx);

    let state_file = temp.path().join(format!("{original_id}.json"));
    assert!(state_file.exists(), "/new must save the prior task state");
    assert_eq!(
        mode.history_lines().len(),
        1,
        "/new must reset the transcript"
    );
    assert!(
        mode.history_lines()[0].starts_with("[new session: task-"),
        "expected new-session marker, got: {:?}",
        mode.history_lines()
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_new_creates_fresh_task_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/new".to_string(), &mut ctx);

    assert_ne!(
        mode.current_task_id(),
        original_id,
        "/new must assign a new task-id"
    );
    assert!(
        !mode.is_pulse_in_progress(),
        "/new must not leave a stale pulse active"
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_new_clears_active_edit_loop() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/new".to_string(), &mut ctx);

    assert!(
        !mode.is_pulse_in_progress(),
        "/new must clear active edit-loop state"
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_restores_active_grants() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut saved = TaskState::new("task-resume-001".to_string());
    saved.active_grants.insert(
        crate::runtime::Capability::ApplyPatch,
        crate::runtime::ApprovalScope::Session,
    );
    saved.pulses.push(crate::pulse_evidence::TurnEvidenceState {
        input: "prior work".to_string(),
        response: "done".to_string(),
        changed_files: vec!["src/app.rs".to_string()],
        ..Default::default()
    });
    saved.status = crate::runtime::TaskStatus::Completed;
    saved.save(temp.path()).unwrap();

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-resume-001".to_string(), &mut ctx);

    assert_eq!(mode.current_task_id(), "task-resume-001");
    assert!(
        mode.task_doc
            .info
            .active_grants
            .contains_key(&crate::runtime::Capability::ApplyPatch)
    );
    assert_eq!(
        mode.task_doc
            .completed_turns
            .iter()
            .flat_map(|t| t.changed_files.iter().map(PathBuf::from))
            .collect::<Vec<_>>(),
        vec![PathBuf::from("src/app.rs")]
    );
    assert_eq!(
        mode.task_doc.info.status,
        crate::runtime::TaskStatus::Completed
    );
    assert!(
        !mode.history_lines().iter().any(|l| l == "stale transcript"),
        "/resume must clear the stale pre-session transcript"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[resumed: task-resume-001 status=Completed]")),
        "expected resume confirmation in history"
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_without_id_offers_recent_task_selection() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let older = TaskState::new("task-resume-older".to_string());
    older.save(temp.path()).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let mut newer = TaskState::new("task-resume-newer".to_string());
    newer.status = crate::runtime::TaskStatus::Running;
    newer.save(temp.path()).unwrap();

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume".to_string(), &mut ctx);

    assert!(
        mode.overlay_active(),
        "/resume without id must open a selection overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("task-resume-newer (Running)")),
        "expected recent-task list in history"
    );

    let newer_selection = mode
        .history_lines()
        .iter()
        .find_map(|line| {
            if !line.contains("task-resume-newer (Running)") {
                return None;
            }
            line.trim()
                .split_once('.')
                .and_then(|(selection, _)| selection.parse::<usize>().ok())
        })
        .expect("expected numeric resume selection for task-resume-newer");

    mode.on_user_input(newer_selection.to_string(), &mut ctx);

    assert_eq!(mode.current_task_id(), "task-resume-newer");
    assert_eq!(
        mode.history_lines().len(),
        1,
        "resume selection must reset transcript"
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_does_not_restore_conversation() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let saved = TaskState::new("task-resume-002".to_string());
    saved.save(temp.path()).unwrap();

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-resume-002".to_string(), &mut ctx);

    assert_eq!(
        mode.history_lines().len(),
        1,
        "/resume must clear prior transcript state"
    );
    assert!(
        !mode.is_pulse_in_progress(),
        "/resume must not start a model pulse"
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_unknown_id_emits_error() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-does-not-exist".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[resume: task 'task-does-not-exist' not found]")),
        "expected not-found message in history"
    );
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_restores_legacy_subdir_state() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let old_cwd = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    let nested = temp.path().join("src/nested");
    let saved_state_dir = nested.join(".vex/state");
    std::fs::create_dir_all(&saved_state_dir).unwrap();

    let mut saved = TaskState::new("task-saved-ui".to_string());
    saved.status = crate::runtime::TaskStatus::Completed;
    saved.save(&saved_state_dir).unwrap();

    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
    std::env::set_current_dir(&nested).unwrap();

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-saved-ui".to_string(), &mut ctx);

    std::env::set_current_dir(old_cwd).unwrap();

    assert_eq!(mode.current_task_id(), "task-saved-ui");
    assert!(
        mode.history_lines()[0].contains("[resumed: task-saved-ui status=Completed]"),
        "expected resume confirmation in history"
    );
}

#[test]
fn test_tui_quit_command_requests_quit() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/quit".to_string(), &mut ctx);
    assert!(
        mode.quit_requested(),
        "/quit must set quit_requested immediately"
    );
    assert!(
        mode.task_doc.active_pulse.is_none(),
        "/quit must not start a model pulse"
    );
}

#[test]
fn test_tui_exit_is_alias_for_quit() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/exit".to_string(), &mut ctx);
    assert!(
        mode.quit_requested(),
        "/exit must behave identically to /quit"
    );
    assert!(
        mode.task_doc.active_pulse.is_none(),
        "/exit must not start a model pulse"
    );
}

#[test]
fn test_tui_about_renders_without_model_turn() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/about".to_string(), &mut ctx);
    assert!(
        mode.task_doc.active_pulse.is_none(),
        "/about must not start a model pulse"
    );
    let has_version = mode.history_lines().iter().any(|l| l.starts_with("vex "));
    assert!(has_version, "/about must render version line");
    let has_build = mode.history_lines().iter().any(|l| l.contains("build"));
    assert!(has_build, "/about must render build metadata");
    let has_commit = mode.history_lines().iter().any(|l| l.contains("commit"));
    assert!(has_commit, "/about must render commit metadata");
}
