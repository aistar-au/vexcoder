use super::*;
use crate::mcp::{McpRegistrySnapshot, McpServerSnapshot, McpToolSummary};

#[test]
fn test_history_cap_env_invalid_uses_default() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var(MAX_HISTORY_LINES_ENV, "invalid-cap");

    let mode = TuiMode::new();
    assert_eq!(mode.history_line_cap, DEFAULT_MAX_HISTORY_LINES);

    std::env::remove_var(MAX_HISTORY_LINES_ENV);
}
#[test]
fn test_tui_second_edit_command_blocked_while_loop_active() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.active_edit_loop = Some(EditLoop::new("task-existing".to_string()));
    mode.history_state.turn_in_progress = true;
    mode.on_user_input("/edit add more tests".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[edit loop already active")),
        "second /edit while loop active must emit reentrancy guard"
    );
}
#[test]
fn test_slash_command_returns_none_for_non_slash_input() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("hello world".to_string(), &mut ctx);
    assert!(
        mode.is_turn_in_progress(),
        "non-slash input must dispatch a model turn"
    );
}
#[test]
fn test_slash_command_does_not_call_start_turn_directly() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit refactor the parser".to_string(), &mut ctx);
    assert_eq!(
        mode.last_turn_input.as_deref(),
        Some("refactor the parser"),
        "/edit must pass bare instruction (not the full slash command) to start_turn"
    );
}
#[test]
fn test_tui_run_command_invokes_validation_suite_only() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input(successful_run_input(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/run must not start a model turn"
    );
    assert!(
        mode.active_edit_loop.is_none(),
        "/run must not seed or invoke EditLoop"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[run]")),
        "expected /run transcript output"
    );
}
#[test]
fn test_init_slash_command_scaffolds_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("/init local-test".to_string(), &mut ctx);

    assert!(temp.path().join(".vex/config.toml").exists());
    assert!(temp.path().join(".vex/validate.toml").exists());
    assert!(temp.path().join("AGENTS.md").exists());
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("selected environment: local-test")));
}
#[tokio::test]
async fn test_tui_context_renders_without_model_turn() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input("/context".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/context must not start a model turn"
    );
    assert_eq!(
        ctx.test_message_count().await,
        initial_messages,
        "/context must not call ctx.start_turn"
    );
    assert!(
        mode.history_lines().iter().any(|line| line == "[context]"),
        "expected context header"
    );
}
#[test]
fn test_tui_context_shows_tilde_token_estimate() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/context".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.trim_start().starts_with("tokens") && line.contains('~')),
        "token estimate line must include '~'"
    );
}
#[test]
fn test_tui_context_shows_active_grants_count() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.current_task
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);

    mode.on_user_input("/context".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("1 active grant(s)")),
        "expected active grants count in /context output"
    );
}
#[test]
fn test_tui_context_shows_active_profile_name() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let mut profile =
        ModelProfile::default_for_backend(crate::runtime::ModelBackendKind::ApiServer);
    profile.name = "api-structured".to_string();
    mode.active_edit_loop = Some(EditLoop::new("task-profile".to_string()).with_profile(profile));

    mode.on_user_input("/context".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("profile") && line.contains("api-structured")),
        "expected active profile name in /context output"
    );
}
#[test]
fn test_tui_commands_renders_all_registered_commands() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/commands".to_string(), &mut ctx);

    assert!(
        mode.history_lines().iter().any(|line| line == "[commands]"),
        "expected commands header"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("retrieve with /context, /tools, and /generate-tests")),
        "expected retrieval guidance header"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[retrieve + context]"),
        "expected grouped retrieval section"
    );
    for spec in SLASH_COMMANDS {
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains(spec.display) && line.contains(spec.description)),
            "expected '{}' to appear in /commands output",
            spec.display
        );
    }
}

#[test]
fn test_agents_and_delegate_commands_manage_session_tasks() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_STATE_DIR");
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".vex")).unwrap();
    std::fs::write(
        temp.path().join(".vex/agents.toml"),
        r#"
[[agents]]
name = "reviewer"
isolation = "worktree"
allowed_capabilities = ["read-file"]

[[teams]]
name = "review-team"
members = ["reviewer"]
"#,
    )
    .unwrap();

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();

    mode.on_user_input("/agents".to_string(), &mut ctx);
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("reviewer profile=")));

    mode.on_user_input("/delegate reviewer inspect docs".to_string(), &mut ctx);
    assert_eq!(mode.current_task.session_tasks.len(), 1);
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("[delegate] session task")));
}

#[test]
fn test_watch_command_reports_saved_session_task() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_STATE_DIR");
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();

    let session_task = crate::runtime::SessionTask::new(
        mode.current_task.id.clone(),
        "reviewer",
        "inspect docs",
        None,
    );
    let session_task_id = session_task.id.clone();
    mode.current_task.add_session_task(session_task);
    mode.persist_current_task_state();

    mode.on_user_input(format!("/watch {session_task_id}"), &mut ctx);
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("[watch]")));
}
#[test]
fn test_tui_help_is_alias_for_commands() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut commands_mode = TuiMode::new();
    let mut help_mode = TuiMode::new();
    let mut ctx = setup_ctx();

    commands_mode.on_user_input("/commands".to_string(), &mut ctx);
    help_mode.on_user_input("/help".to_string(), &mut ctx);

    assert_eq!(
        &commands_mode.history_lines()[2..],
        &help_mode.history_lines()[2..],
        "/help must render the same command directory as /commands"
    );
}
#[tokio::test]
async fn test_commands_output_does_not_call_start_turn() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input("/commands".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/commands must not start a model turn"
    );
    assert_eq!(
        ctx.test_message_count().await,
        initial_messages,
        "/commands must not call ctx.start_turn"
    );
}
#[test]
fn test_custom_command_appears_in_commands_list() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "standup.toml",
        "standup",
        "summarise changes",
        "Summarise {{input}} using {{context}}",
    );

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/commands".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[custom commands]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("/standup [input]") && line.contains("summarise changes")));
}
#[test]
fn test_custom_command_invokes_single_turn() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "standup.toml",
        "standup",
        "summarise changes",
        "Prompt: {{input}}",
    );

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/standup src/lib.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(mode.is_turn_in_progress());
    assert!(turn_input.contains("Prompt: src/lib.rs"));
}
#[test]
fn test_custom_command_context_substitution() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "standup.toml",
        "standup",
        "summarise changes",
        "Context:\n{{context}}",
    );

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/standup src/lib.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("pub fn answer() -> i32 { 42 }"));
    assert_eq!(
        turn_input.matches("pub fn answer() -> i32 { 42 }").count(),
        1,
        "custom command context must be injected exactly once"
    );
    assert_eq!(
        turn_input.matches("## Context").count(),
        1,
        "custom command context header must not be duplicated"
    );
}
#[test]
fn test_custom_command_project_scoped_takes_precedence() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let xdg = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", xdg.path());

    write_custom_command(
        &xdg.path().join("vex/commands"),
        "standup.toml",
        "standup",
        "user",
        "USER TEMPLATE",
    );
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "standup.toml",
        "standup",
        "project",
        "PROJECT TEMPLATE",
    );

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/standup".to_string(), &mut ctx);
    std::env::remove_var("XDG_CONFIG_HOME");

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("PROJECT TEMPLATE"));
    assert!(!turn_input.contains("USER TEMPLATE"));
}
#[test]
fn test_custom_command_cannot_shadow_builtin() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "edit.toml",
        "edit",
        "shadow builtin",
        "shadow",
    );

    let mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    assert!(mode.custom_commands.is_empty());
}
#[test]
fn test_tui_tools_renders_builtin_tools() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/tools".to_string(), &mut ctx);

    assert!(mode.history_lines().iter().any(|line| line == "[tools]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("live registry: built-in tools only")));
    assert!(mode.history_lines().iter().any(|line| line.contains(
        "discovery flow: list_files/find_files -> search_content/codebase_search -> read_file"
    )));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[tools:retrieve]"));
    for tool in builtin_tool_summaries() {
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains(&tool.name)),
            "expected '{}' in /tools output",
            tool.name
        );
    }
}

#[test]
fn test_tui_tools_lists_loaded_mcp_tools() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    mode.mcp_snapshot = Some(McpRegistrySnapshot {
        servers: vec![McpServerSnapshot {
            name: "docs".to_string(),
            transport: "stdio".to_string(),
            tools: vec![McpToolSummary {
                full_name: "mcp.docs.search".to_string(),
                short_name: "search".to_string(),
                description: "Search documentation".to_string(),
            }],
        }],
    });
    let mut ctx = setup_ctx();

    mode.on_user_input("/tools desc".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[tools:mcp]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("mcp.docs.search") && line.contains("Search documentation")));
}

#[test]
fn test_tui_mcp_list_and_show_commands() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    mode.mcp_snapshot = Some(McpRegistrySnapshot {
        servers: vec![McpServerSnapshot {
            name: "docs".to_string(),
            transport: "stdio".to_string(),
            tools: vec![McpToolSummary {
                full_name: "mcp.docs.search".to_string(),
                short_name: "search".to_string(),
                description: "Search documentation".to_string(),
            }],
        }],
    });
    let mut ctx = setup_ctx();

    mode.on_user_input("/mcp list".to_string(), &mut ctx);
    mode.on_user_input("/mcp show docs".to_string(), &mut ctx);

    assert!(mode.history_lines().iter().any(|line| line == "[mcp]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("1 server(s), 1 tool(s) loaded")));
    assert!(mode.history_lines().iter().any(|line| line == "[mcp:docs]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("mcp.docs.search") && line.contains("Search documentation")));
}
#[test]
fn test_tui_tools_desc_includes_descriptions() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/tools desc".to_string(), &mut ctx);

    for tool in builtin_tool_summaries() {
        assert!(
            mode.history_lines().iter().any(|line| {
                line.contains(&tool.name) && line.contains(&tool.description) && line.contains('[')
            }),
            "expected '{}' description in /tools desc output",
            tool.name
        );
    }
}
#[test]
fn test_usage_command_uses_last_turn_estimate_flag() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    ctx.test_record_session_turn(crate::usage::TurnTokens {
        input: 10,
        output: 5,
        estimated: true,
        ..Default::default()
    });
    ctx.test_record_session_turn(crate::usage::TurnTokens {
        input: 4,
        output: 3,
        estimated: false,
        ..Default::default()
    });

    mode.on_user_input("/usage".to_string(), &mut ctx);

    assert!(mode.history_lines().iter().any(|line| line == "[usage]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "  this turn   : 4 in / 3 out"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "  session     : 14 in / 8 out (estimated)"));
}
#[tokio::test]
async fn test_tui_tools_does_not_start_model_turn() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input("/tools".to_string(), &mut ctx);

    assert!(!mode.is_turn_in_progress());
    assert_eq!(ctx.test_message_count().await, initial_messages);
}
#[test]
fn test_tui_generate_tests_assembles_context() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/generate-tests src/lib.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("generate tests for src/lib.rs"));
    assert!(turn_input.contains("pub fn answer() -> i32 { 42 }"));
}
#[test]
fn test_tui_generate_tests_infers_framework() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\n",
    )
    .unwrap();

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/generate-tests src/lib.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("Preferred test framework: cargo-test"));
}
#[test]
fn test_tui_generate_tests_framework_flag_overrides_inference() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\n",
    )
    .unwrap();

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input(
        "/generate-tests src/lib.rs --framework jest".to_string(),
        &mut ctx,
    );

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("Preferred test framework: jest"));
    assert!(!turn_input.contains("Preferred test framework: cargo-test"));
}
#[test]
fn test_missing_command_description_is_compile_error() {
    assert!(
        !SLASH_COMMANDS.is_empty(),
        "slash command registry must not be empty"
    );
    for spec in SLASH_COMMANDS {
        assert!(
            !spec.description.is_empty(),
            "command '{}' must have a description",
            spec.display
        );
    }
}
