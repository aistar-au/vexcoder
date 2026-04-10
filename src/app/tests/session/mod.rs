use super::*;

mod permissions;
mod runtime;

// -- PI-04 / PI-05 / PJ-01 / PJ-02 ---------------------------------------

#[test]
fn test_tui_new_saves_current_state_before_reset() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

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
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_new_creates_fresh_task_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

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
        !mode.is_turn_in_progress(),
        "/new must not leave a stale turn active"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_new_clears_active_edit_loop() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/new".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/new must clear active edit-loop state"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_restores_active_grants() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut saved = TaskState::new("task-resume-001".to_string());
    saved.active_grants.insert(
        crate::runtime::Capability::ApplyPatch,
        crate::runtime::ApprovalScope::Session,
    );
    saved.turns.push(crate::turn_evidence::TurnEvidenceState {
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
    assert!(mode
        .task_doc
        .meta
        .active_grants
        .contains_key(&crate::runtime::Capability::ApplyPatch));
    assert_eq!(
        mode.task_doc
            .completed_turns
            .iter()
            .flat_map(|t| t.changed_files.iter().map(PathBuf::from))
            .collect::<Vec<_>>(),
        vec![PathBuf::from("src/app.rs")]
    );
    assert_eq!(
        mode.task_doc.meta.status,
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
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_without_id_offers_recent_task_selection() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

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
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_does_not_restore_conversation() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

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
        !mode.is_turn_in_progress(),
        "/resume must not start a model turn"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_unknown_id_emits_error() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-does-not-exist".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[resume: task 'task-does-not-exist' not found]")),
        "expected not-found message in history"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_restores_legacy_subdir_state() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let old_cwd = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    let nested = temp.path().join("src/nested");
    let legacy_state_dir = nested.join(".vex/state");
    std::fs::create_dir_all(&legacy_state_dir).unwrap();

    let mut saved = TaskState::new("task-legacy-ui".to_string());
    saved.status = crate::runtime::TaskStatus::Completed;
    saved.save(&legacy_state_dir).unwrap();

    std::env::remove_var("VEX_STATE_DIR");
    std::env::set_current_dir(&nested).unwrap();

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-legacy-ui".to_string(), &mut ctx);

    std::env::set_current_dir(old_cwd).unwrap();

    assert_eq!(mode.current_task_id(), "task-legacy-ui");
    assert!(
        mode.history_lines()[0].contains("[resumed: task-legacy-ui status=Completed]"),
        "expected resume confirmation in history"
    );
}

// -- /compact -------------------------------------------------------------

#[test]
fn test_tui_compact_resets_conversation_history() {
    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();

    mode.on_user_input("/compact".to_string(), &mut ctx);

    // 1 confirmation line (pre_session_notice) + 1 compaction boundary row
    assert_eq!(
        mode.history_lines().len(),
        2,
        "/compact must reset the transcript with boundary marker"
    );
    assert!(
        mode.history_lines()[0].starts_with("[compacted: "),
        "expected compacted confirmation, got: {}",
        mode.history_lines()[0]
    );
    assert!(
        mode.history_lines()[1].starts_with("[context compacted at turn "),
        "expected compaction boundary marker, got: {}",
        mode.history_lines()[1]
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_tui_compact_persists_cleared_turns() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.task_doc
        .completed_turns
        .push(crate::runtime::task_document::TurnDocument {
            turn_index: 0,
            input: "draft a plan".to_string(),
            entries: vec![],
            outcome: crate::runtime::task_document::TurnOutcome::Completed,
            changed_files: vec![],
            command_history: vec![],
            tokens: crate::usage::TurnTokens {
                ..Default::default()
            },
            started_at_ms: 0,
            completed_at_ms: 1,
            ttft_ms: None,
            timings: None,
        });
    mode.persist_task_document();

    let task_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);

    let saved = TaskState::load(temp.path(), &task_id).unwrap();
    assert!(
        saved.turns.is_empty(),
        "/compact must persist cleared turns"
    );

    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_compact_preserves_task_id_and_grants() {
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    mode.task_doc.meta.active_grants.insert(
        crate::runtime::Capability::RunCommand,
        crate::runtime::ApprovalScope::Session,
    );
    let mut ctx = setup_ctx();

    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task_id(),
        original_id,
        "/compact must not change task-id"
    );
    assert!(
        mode.task_doc
            .meta
            .active_grants
            .contains_key(&crate::runtime::Capability::RunCommand),
        "/compact must preserve active grants"
    );
    assert!(
        !mode.is_turn_in_progress(),
        "/compact must clear active edit-loop state"
    );
}

#[test]
fn test_tui_compact_clears_active_edit_loop() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/compact must clear active edit-loop state"
    );
}

// -- /fork ----------------------------------------------------------------

#[test]
fn test_tui_fork_saves_parent_before_branching() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let parent_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    let parent_file = temp.path().join(format!("{parent_id}.json"));
    assert!(parent_file.exists(), "/fork must save parent state file");
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_creates_new_task_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let parent_id = mode.current_task_id();
    mode.task_doc.meta.active_grants.insert(
        crate::runtime::Capability::RunCommand,
        crate::runtime::ApprovalScope::Session,
    );
    // In the new model changed_files live in completed turns, not on current_task.
    // Pre-populate a completed turn to carry the file across the fork.
    mode.task_doc.meta.status = crate::runtime::TaskStatus::Running;
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();

    mode.on_user_input("/fork feature work".to_string(), &mut ctx);

    assert_ne!(
        mode.current_task_id(),
        parent_id,
        "/fork must assign a new task-id"
    );
    assert!(mode.current_task_id().ends_with("-feature-work"));
    assert!(mode
        .task_doc
        .meta
        .active_grants
        .contains_key(&crate::runtime::Capability::RunCommand));
    // NOTE: In the document-projector model, forks start with empty completed_turns;
    // changed_files are per-turn and are not inherited by the forked task.
    assert_eq!(
        mode.task_doc.meta.status,
        crate::runtime::TaskStatus::Running
    );
    assert_eq!(mode.history_lines().len(), 1, "/fork must reset transcript");
    assert!(
        mode.history_lines()[0].contains(&format!("branched from {parent_id}")),
        "expected fork confirmation in history"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_does_not_copy_conversation() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    assert!(
        !mode.history_lines().iter().any(|l| l == "stale transcript"),
        "/fork must clear prior transcript state"
    );
    assert!(
        !mode.is_turn_in_progress(),
        "/fork must not start a model turn"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_aborts_on_save_failure() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let blocking_path = temp.path().join("state-file");
    std::fs::write(&blocking_path, "occupied").unwrap();
    std::env::set_var("VEX_STATE_DIR", blocking_path.as_os_str());

    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task_id(),
        original_id,
        "/fork must not change task-id when parent save fails"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[fork] save failed")),
        "expected save failure message"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

// -- PK-01: /quit and /exit ------------------------------------------------

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
        mode.task_doc.active_turn.is_none(),
        "/quit must not start a model turn"
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
        mode.task_doc.active_turn.is_none(),
        "/exit must not start a model turn"
    );
}

// -- PK-02: /about ---------------------------------------------------------

#[test]
fn test_tui_about_renders_without_model_turn() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/about".to_string(), &mut ctx);
    assert!(
        mode.task_doc.active_turn.is_none(),
        "/about must not start a model turn"
    );
    let has_version = mode.history_lines().iter().any(|l| l.starts_with("vex "));
    assert!(has_version, "/about must render version line");
    let has_build = mode.history_lines().iter().any(|l| l.contains("build"));
    assert!(has_build, "/about must render build metadata");
    let has_commit = mode.history_lines().iter().any(|l| l.contains("commit"));
    assert!(has_commit, "/about must render commit metadata");
}
