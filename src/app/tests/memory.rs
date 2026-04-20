use super::*;
use crate::config::CompactionConfig;
use crate::config::UndoConfig;

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
fn test_build_runtime_auto_index_warms_codebase_search_index() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::state::clear_codebase_index_for_tests();

    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("warm.rs"), "pub fn tui_warm_symbol() {}\n").unwrap();

    let search_config = crate::config::SearchConfig {
        enabled: true,
        auto_index: true,
        ..Default::default()
    };

    let count = crate::state::warm_codebase_index_with_config(temp.path(), &search_config);
    assert!(
        count.is_some() && count.unwrap() > 0,
        "warm_codebase_index_with_config must index at least one chunk"
    );
    let names = crate::state::codebase_index_names_for_tests();
    assert!(
        names.iter().any(|name| name == "tui_warm_symbol"),
        "expected startup warm to index tui_warm_symbol"
    );
}

#[test]
fn test_tui_memory_add_appends_to_file() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let state_dir = temp.path().join("state");
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", state_dir.as_os_str());
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input(
        "/memory add track the open build issue".to_string(),
        &mut ctx,
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory: note added]")),
        "expected '[memory: note added]' in history"
    );
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(content.contains("track the open build issue"));
    assert_eq!(mode.task_doc.session_notes.len(), 1);
    assert_eq!(
        mode.task_doc.session_notes[0].content,
        "track the open build issue"
    );
    let saved = TaskState::load(&state_dir, &mode.current_task_id()).unwrap();
    assert_eq!(saved.session_notes, mode.task_doc.session_notes);
    assert!(!mode.is_turn_in_progress());
    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
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
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", state_dir.as_os_str());

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input("/memory add keep this".to_string(), &mut ctx);
    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    mode.on_user_input("y".to_string(), &mut ctx);

    let saved = TaskState::load(&state_dir, &mode.current_task_id()).unwrap();
    assert!(saved.session_notes.is_empty());
    assert!(mode.task_doc.session_notes.is_empty());
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(
        content.trim().is_empty(),
        "/memory clear must clear file contents"
    );

    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
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
    crate::test_support::test_set_var(&_env_lock, "HOME", home.as_os_str());
    crate::test_support::test_remove_var(&_env_lock, "XDG_CONFIG_HOME");

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(None);
    mode.on_user_input("/memory".to_string(), &mut ctx);

    match old_home {
        Some(value) => crate::test_support::test_set_var(&_env_lock, "HOME", value),
        None => crate::test_support::test_remove_var(&_env_lock, "HOME"),
    }
    match old_xdg {
        Some(value) => crate::test_support::test_set_var(&_env_lock, "XDG_CONFIG_HOME", value),
        None => crate::test_support::test_remove_var(&_env_lock, "XDG_CONFIG_HOME"),
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
        tool_call_mode: crate::runtime::ToolCallMode::Structured,
        tool_policy: crate::runtime::ToolPolicy::Full,
        model_profile: ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: UndoConfig::default(),
        search: crate::config::SearchConfig {
            auto_index: false,
            ..Default::default()
        },
        notes_path: Some(notes_path),
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };

    let (runtime, _ctx) = build_runtime(config).expect("runtime should build");
    let has_warning = runtime
        .mode
        .history_lines()
        .iter()
        .any(|l| l.contains("notes exceed token budget"));
    assert!(has_warning, "expected startup budget warning in history");
}

// -- PM-04 auto-memory anchor tests ------------------------------------------

#[test]
fn test_auto_memory_disabled_by_default() {
    let config = Config::default_for_tui();
    assert!(
        !config.auto_memory.enabled,
        "auto_memory must be disabled by default"
    );
}

#[test]
fn test_auto_memory_max_notes_default_is_three() {
    let config = Config::default_for_tui();
    assert_eq!(config.auto_memory.max_notes_per_turn, 3);
}

#[test]
fn test_auto_memory_on_command_enables_flag() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    assert!(!mode.auto_memory_enabled, "should be off initially");
    mode.on_user_input("/memory auto on".to_string(), &mut ctx);
    assert!(
        mode.auto_memory_enabled,
        "/memory auto on should set auto_memory_enabled"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("auto extraction enabled"))
    );
}

#[test]
fn test_auto_memory_off_command_disables_flag() {
    let mut mode = TuiMode::new();
    mode.auto_memory_enabled = true;
    let mut ctx = setup_ctx();
    mode.on_user_input("/memory auto off".to_string(), &mut ctx);
    assert!(
        !mode.auto_memory_enabled,
        "/memory auto off should clear auto_memory_enabled"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("auto extraction disabled"))
    );
}

#[test]
fn test_extract_notes_respects_max_and_tags_auto() {
    let response = "- fact one\n- fact two\n- fact three\n- fact four\n";
    let notes = crate::auto_memory::extract_notes_from_turn("", response, 2);
    assert_eq!(notes.len(), 2, "max_notes_per_turn must be respected");
    for note in &notes {
        assert!(
            !note.contains("[auto]"),
            "raw extracted notes should remain plain text before formatting"
        );
    }
}

#[test]
fn test_extract_notes_empty_input_returns_nothing() {
    let notes = crate::auto_memory::extract_notes_from_turn("", "", 5);
    assert!(notes.is_empty(), "empty response must produce no notes");
}

#[test]
fn test_auto_memory_clear_removes_tagged_entries() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("notes.md");
    std::fs::write(
        &notes_path,
        "[1] [auto] auto line\nmanual line\n[2] [auto] another auto\n",
    )
    .unwrap();

    let removed = crate::auto_memory::remove_auto_notes(&notes_path).unwrap();
    assert_eq!(removed, 2);
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(content.contains("manual line"));
    assert!(!content.contains("[auto]"));
}

#[test]
fn test_format_auto_notes_adds_timestamp_prefix() {
    let formatted = crate::auto_memory::format_auto_notes(&["note".to_string()], 7);
    assert_eq!(formatted, vec!["[7] [auto] note".to_string()]);
}

#[test]
fn test_auto_memory_config_enabled_true_persists() {
    let layer = crate::config::AutoMemoryConfigLayer {
        enabled: Some(true),
        max_notes_per_turn: Some(5),
    };
    assert_eq!(layer.enabled, Some(true));
    assert_eq!(layer.max_notes_per_turn, Some(5));
}
