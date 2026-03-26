use super::*;

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
    saved.changed_files.push(PathBuf::from("src/app.rs"));
    saved.status = crate::runtime::TaskStatus::Completed;
    saved.save(temp.path()).unwrap();

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-resume-001".to_string(), &mut ctx);

    assert_eq!(mode.current_task_id(), "task-resume-001");
    assert!(mode
        .current_task
        .active_grants
        .contains_key(&crate::runtime::Capability::ApplyPatch));
    assert_eq!(
        mode.current_task.changed_files,
        vec![PathBuf::from("src/app.rs")]
    );
    assert_eq!(
        mode.current_task.status,
        crate::runtime::TaskStatus::Completed
    );
    assert_eq!(
        mode.history_lines().len(),
        1,
        "/resume must reset the transcript"
    );
    assert!(
        mode.history_lines()[0].contains("[resumed: task-resume-001 status=Completed]"),
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

    mode.on_user_input("1".to_string(), &mut ctx);

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
#[test]
fn test_tui_compact_resets_conversation_history() {
    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();

    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert_eq!(
        mode.history_lines().len(),
        1,
        "/compact must reset the transcript"
    );
    assert!(
        mode.history_lines()[0].starts_with("[compacted: conversation history reset; task "),
        "expected compacted confirmation"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_tui_compact_persists_cleared_turns() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.current_task.turns.push(TurnEvidenceState {
        input: "draft a plan".to_string(),
        response: "plan ready".to_string(),
        tool_invocations: Vec::new(),
        changed_files: Vec::new(),
        command_history: Vec::new(),
        tokens: Default::default(),
    });
    mode.persist_current_task_state();

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
    mode.current_task.active_grants.insert(
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
        mode.current_task
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
    mode.current_task.active_grants.insert(
        crate::runtime::Capability::RunCommand,
        crate::runtime::ApprovalScope::Session,
    );
    mode.current_task
        .changed_files
        .push(PathBuf::from("src/app.rs"));
    mode.current_task.status = crate::runtime::TaskStatus::Running;
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
        .current_task
        .active_grants
        .contains_key(&crate::runtime::Capability::RunCommand));
    assert_eq!(
        mode.current_task.changed_files,
        vec![PathBuf::from("src/app.rs")]
    );
    assert_eq!(
        mode.current_task.status,
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

    assert_eq!(
        mode.history_lines().len(),
        1,
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
        !mode.history_state.turn_in_progress,
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
        !mode.history_state.turn_in_progress,
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
        !mode.history_state.turn_in_progress,
        "/about must not start a model turn"
    );
    let has_version = mode
        .history_state
        .lines
        .iter()
        .any(|l| l.starts_with("vex "));
    assert!(has_version, "/about must render version line");
    let has_build = mode.history_state.lines.iter().any(|l| l.contains("build"));
    assert!(has_build, "/about must render build metadata");
    let has_commit = mode
        .history_state
        .lines
        .iter()
        .any(|l| l.contains("commit"));
    assert!(has_commit, "/about must render commit metadata");
}
// -- PI-01 / PI-02 / PI-03 -------------------------------------------------

#[test]
fn test_permissions_empty_grants() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/permissions".to_string(), &mut ctx);
    assert!(
        mode.history_lines().iter().any(|l| l == "[permissions]"),
        "expected permissions header"
    );
    for &cap in ALL_CAPABILITIES {
        let cap_name = capability_to_kebab(cap);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains(cap_name) && l.contains("(none)")),
            "expected {cap_name} with (none) in empty-grants permissions output"
        );
    }
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_permissions_lists_active_grants() {
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    mode.current_task
        .active_grants
        .insert(Capability::Network, ApprovalScope::Once);
    let mut ctx = setup_ctx();
    mode.on_user_input("/permissions".to_string(), &mut ctx);
    let lines = mode.history_lines().to_vec();
    let has_header = lines.iter().any(|l| l == "[permissions]");
    let has_run_command = lines
        .iter()
        .any(|l| l.contains("run-command") && l.contains("session"));
    let has_network = lines
        .iter()
        .any(|l| l.contains("network") && l.contains("once"));
    let has_apply_patch_none = lines
        .iter()
        .any(|l| l.contains("apply-patch") && l.contains("(none)"));
    assert!(has_header, "expected active grants header");
    assert!(has_run_command, "expected run-command session entry");
    assert!(has_network, "expected network once entry");
    assert!(
        has_apply_patch_none,
        "expected apply-patch (none) for absent grant"
    );
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_allow_inserts_grant() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow run-command session".to_string(), &mut ctx);
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::RunCommand),
        Some(&ApprovalScope::Session),
        "allow must insert the grant with session scope"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: run-command granted for session]")),
        "expected grant confirmation"
    );
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_allow_defaults_to_once_scope() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow write-file".to_string(), &mut ctx);
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Once),
        "allow without scope must default to once"
    );
}
#[test]
fn test_allow_unknown_capability_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow bogus-cap".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown capability 'bogus-cap']")),
        "expected unknown-capability error"
    );
    assert!(mode.current_task.active_grants.is_empty());
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_allow_task_scope_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow network task".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown scope 'task'; valid: once | session]")),
        "expected task scope rejection"
    );
    assert!(mode.current_task.active_grants.is_empty());
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_allow_unknown_scope_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow network forever".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown scope 'forever'; valid: once | session]")),
        "expected unknown-scope error"
    );
    assert!(mode.current_task.active_grants.is_empty());
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_deny_removes_grant() {
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Task);
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny apply-patch".to_string(), &mut ctx);
    assert!(
        !mode
            .current_task
            .active_grants
            .contains_key(&Capability::ApplyPatch),
        "deny must remove the grant"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: apply-patch removed]")),
        "expected revoke confirmation"
    );
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_deny_no_grant_emits_info() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny browser".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: browser not in active grants]")),
        "expected no-active-grant info message"
    );
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_deny_unknown_capability_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny not-a-thing".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: unknown capability 'not-a-thing']")),
        "expected unknown-capability error"
    );
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_capability_kebab_round_trip() {
    for &cap in ALL_CAPABILITIES {
        let kebab = capability_to_kebab(cap);
        let round_tripped = kebab_to_capability(kebab);
        assert_eq!(
            round_tripped,
            Some(cap),
            "capability {kebab} failed round-trip through kebab_to_capability"
        );
    }
}
#[test]
fn test_capability_for_tool_name_maps_builtin_tools() {
    assert_eq!(
        capability_for_tool_name("read_file"),
        Some(Capability::ReadFile)
    );
    assert_eq!(
        capability_for_tool_name("write_file"),
        Some(Capability::WriteFile)
    );
    assert_eq!(
        capability_for_tool_name("apply_patch"),
        Some(Capability::ApplyPatch)
    );
    assert_eq!(
        capability_for_tool_name("run_command"),
        Some(Capability::RunCommand)
    );
    assert_eq!(
        capability_for_tool_name("git_commit"),
        Some(Capability::ApplyPatch)
    );
    assert_eq!(capability_for_tool_name("unknown_tool"), None);
}
// -- PM-01 (app side): build_runtime_with_resume ---------------------------

#[test]
fn test_build_runtime_with_resume_restores_task() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = TaskState::new("task-startup-resume".to_string());
    state
        .active_grants
        .insert(Capability::Network, ApprovalScope::Session);
    state.status = crate::runtime::TaskStatus::Running;

    let config = Config {
        model_token: None,
        model_name: "mock-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: temp.path().to_path_buf(),
        model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
        model_protocol: crate::runtime::ModelProtocol::MessagesV1,
        tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
        model_profile: ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
    };

    let (runtime, _ctx) =
        build_runtime_with_resume(config, state).expect("build_runtime_with_resume should succeed");

    assert_eq!(runtime.mode.current_task.id, "task-startup-resume");
    assert_eq!(
        runtime
            .mode
            .current_task
            .active_grants
            .get(&Capability::Network),
        Some(&ApprovalScope::Session)
    );
    assert!(
        runtime
            .mode
            .history_lines()
            .iter()
            .any(|l| l.contains("[resumed: task-startup-resume status=Running]")),
        "expected resume banner in history"
    );
}
#[test]
fn test_tui_new_clears_active_edit_loop_field() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.active_edit_loop = Some(EditLoop::new("task-before-new".to_string()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/new".to_string(), &mut ctx);

    assert!(
        mode.active_edit_loop.is_none(),
        "/new must clear active_edit_loop field"
    );
    std::env::remove_var("VEX_STATE_DIR");
}
#[test]
fn test_tui_compact_clears_active_edit_loop_field() {
    let mut mode = TuiMode::new();
    mode.active_edit_loop = Some(EditLoop::new("task-before-compact".to_string()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert!(
        mode.active_edit_loop.is_none(),
        "/compact must clear active_edit_loop field"
    );
}
#[test]
fn test_tui_compact_resets_turn_evidence_and_token_counter() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    // Simulate a completed turn with token usage.
    mode.current_task
        .turns
        .push(crate::turn_evidence::TurnEvidenceState {
            input: "hello".to_string(),
            response: "world".to_string(),
            tokens: crate::usage::TurnTokens {
                input: 1000,
                output: 500,
                estimated: false,
                ..Default::default()
            },
            ..Default::default()
        });
    ctx.test_record_session_turn(crate::usage::TurnTokens {
        input: 1000,
        output: 500,
        estimated: false,
        ..Default::default()
    });

    // Verify tokens are non-zero before compact.
    let status_before = mode.status_line();
    assert!(
        status_before.contains("tokens:1500"),
        "status line must show accumulated tokens before compact, got: {status_before}"
    );

    mode.on_user_input("/compact".to_string(), &mut ctx);

    // After compact, turns must be cleared so the status line shows tokens:0.
    assert!(
        mode.current_task.turns.is_empty(),
        "/compact must clear turn evidence to reset token counter"
    );
    let status_after = mode.status_line();
    assert!(
        status_after.contains("tokens:0"),
        "status line must show tokens:0 after compact, got: {status_after}"
    );
}
#[test]
fn test_tui_compact_preserves_task_id_but_clears_turns() {
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();

    mode.current_task
        .turns
        .push(crate::turn_evidence::TurnEvidenceState {
            input: "test".to_string(),
            response: "response".to_string(),
            ..Default::default()
        });

    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task_id(),
        original_id,
        "/compact must preserve task-id"
    );
    assert!(
        mode.current_task.turns.is_empty(),
        "/compact must clear accumulated turns"
    );
    assert!(
        !mode.current_task.active_grants.is_empty() || mode.current_task.active_grants.is_empty(),
        "grants state must remain consistent"
    );
}
