use super::*;

#[test]
fn test_tui_memory_renders_empty_notes() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path));
    mode.on_user_input("/memory".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory] no notes")),
        "expected '[memory] no notes' in history"
    );
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_tui_memory_add_appends_to_file() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let state_dir = temp.path().join("state");
    std::env::set_var("VEX_STATE_DIR", state_dir.as_os_str());
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input("/memory add hello world".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory: note added]")),
        "expected '[memory: note added]' in history"
    );
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(content.contains("hello world"));
    assert_eq!(mode.current_task.session_notes.len(), 1);
    assert_eq!(mode.current_task.session_notes[0].content, "hello world");
    let saved = TaskState::load(&state_dir, &mode.current_task_id()).unwrap();
    assert_eq!(saved.session_notes, mode.current_task.session_notes);
    assert!(!mode.is_turn_in_progress());
    std::env::remove_var("VEX_STATE_DIR");
}
#[test]
fn test_tui_memory_clear_requires_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "existing note\n").unwrap();
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    assert!(
        mode.pending_memory_clear_overlay(),
        "memory clear must enter overlay state"
    );
    assert!(
        mode.overlay_active(),
        "overlay must be active during memory clear"
    );
    // File must not be cleared until confirmed
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(content.contains("existing note"));
    assert!(!mode.is_turn_in_progress());
}
#[test]
fn test_tui_memory_clear_cancellable() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "keep this note\n").unwrap();
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    mode.on_user_input("n".to_string(), &mut ctx);
    assert!(
        !mode.pending_memory_clear_overlay(),
        "overlay must clear after cancel"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory: cancelled]")),
        "expected '[memory: cancelled]' in history"
    );
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(
        content.contains("keep this note"),
        "file must not be cleared on cancel"
    );
}

#[test]
fn test_tui_memory_clear_persists_empty_session_notes() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let state_dir = temp.path().join("state");
    std::env::set_var("VEX_STATE_DIR", state_dir.as_os_str());

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input("/memory add keep this".to_string(), &mut ctx);
    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    mode.on_user_input("y".to_string(), &mut ctx);

    let saved = TaskState::load(&state_dir, &mode.current_task_id()).unwrap();
    assert!(saved.session_notes.is_empty());
    assert!(mode.current_task.session_notes.is_empty());
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(
        content.trim().is_empty(),
        "/memory clear must clear file contents"
    );

    std::env::remove_var("VEX_STATE_DIR");
}
#[test]
fn test_tui_memory_does_not_call_start_turn() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "a note\n").unwrap();
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));

    // /memory
    mode.on_user_input("/memory".to_string(), &mut ctx);
    assert!(!mode.is_turn_in_progress(), "/memory must not start a turn");

    // /memory add
    mode.on_user_input("/memory add another".to_string(), &mut ctx);
    assert!(
        !mode.is_turn_in_progress(),
        "/memory add must not start a turn"
    );

    // /memory clear + cancel
    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    assert!(
        !mode.is_turn_in_progress(),
        "/memory clear must not start a turn"
    );
    mode.on_user_input("n".to_string(), &mut ctx);
    assert!(!mode.is_turn_in_progress(), "cancel must not start a turn");
}
#[test]
fn test_tui_memory_reads_legacy_fallback_notes() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir_all(home.join(".vex")).unwrap();
    std::fs::write(home.join(".vex/memory.md"), "legacy note\n").unwrap();

    let old_home = std::env::var("HOME").ok();
    let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("HOME", home.as_os_str());
    std::env::remove_var("XDG_CONFIG_HOME");

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(None);
    mode.on_user_input("/memory".to_string(), &mut ctx);

    match old_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match old_xdg {
        Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("legacy note")),
        "expected legacy fallback notes to render"
    );
}
#[test]
fn test_memory_injection_within_budget_returns_content() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "my project note\n").unwrap();
    let (content, warning) = resolve_notes_for_injection(Some(notes_path.as_path()), 2048);
    assert!(warning.is_none(), "notes within budget must not warn");
    let content = content.as_deref().unwrap_or("");
    assert!(
        content.contains("my project note"),
        "notes content must be returned for system prompt injection"
    );
}
#[test]
fn test_memory_injection_over_budget_emits_startup_warning() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let big_content = "x".repeat((2048 * 4) + 1);
    std::fs::write(&notes_path, &big_content).unwrap();

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
        notes_path: Some(notes_path),
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
    };

    let (runtime, _ctx) = build_runtime(config).expect("runtime should build");
    let has_warning = runtime
        .mode
        .history_lines()
        .iter()
        .any(|l| l.contains("notes exceed token budget"));
    assert!(has_warning, "expected startup budget warning in history");
}
