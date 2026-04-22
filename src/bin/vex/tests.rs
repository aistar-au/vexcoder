#![allow(unsafe_code)]

use super::{
    Cli, Commands, CredentialsCommands, SkillsCommands, apply_process_policy_overrides,
    default_auto_approve_scope, format_task_entries_table, map_encoding_to_output_format,
    read_secret_from_env_var, read_secret_from_reader, render_task_entries,
    resolve_credentials_secret, resolve_resume_state, tool_policy_from_cli,
};
use clap::{CommandFactory, Parser};
use clap_complete::Shell;
use std::io::Cursor;
use std::process::Command;
use vexapi::app::TuiMode;
use vexapi::batch_mode::{AutoApproveScope, BatchResult, OutputFormat};
use vexapi::config::Config;
use vexapi::disk_policy::DiskPolicyMode;
use vexapi::init::{
    INIT_CONFIG_NORMATIVE_KEYS, INIT_CONFIG_TEMPLATE, extract_init_template_keys, run_init,
};
use vexapi::pr_summary::{prepare_pr_summary_prompt, run_branch, run_pr_summary_with_batch};
use vexapi::runtime::{TaskState, TaskStatus, ToolPolicy};
use vexapi::startup::{looks_like_session_output, should_ignore_startup_paste_text};
use vexapi::tui_frontend::{
    active_file_picker, active_slash_picker, apply_file_picker_selection,
    apply_slash_picker_selection, build_file_overlay, build_slash_overlay,
    file_picker_is_dismissed, render_file_picker_hint, render_slash_picker_hint,
    slash_prefix_token,
};
use vexapi::ui::editor::InputEditor;
use vexapi::ui::editor::file_mention_range;

mod test_support {
    pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
}

#[path = "tests/picker.rs"]
mod picker;

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: stdout={} stderr={}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(repo: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: stdout={} stderr={}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_git_repo() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(temp.path(), &["checkout", "-b", "main"]);
    run_git(temp.path(), &["config", "user.name", "Test User"]);
    run_git(temp.path(), &["config", "user.email", "test@example.com"]);
    std::fs::write(temp.path().join("README.md"), "hello\n").unwrap();
    run_git(temp.path(), &["add", "README.md"]);
    run_git(temp.path(), &["commit", "-m", "initial"]);
    temp
}

fn init_pr_summary_repo() -> tempfile::TempDir {
    let temp = init_git_repo();
    let main_sha = git_stdout(temp.path(), &["rev-parse", "HEAD"]);
    run_git(
        temp.path(),
        &["update-ref", "refs/remotes/origin/main", &main_sha],
    );
    run_git(
        temp.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    run_git(temp.path(), &["checkout", "-b", "feature/pr-summary"]);
    std::fs::write(temp.path().join("feature.txt"), "feature change\n").unwrap();
    run_git(temp.path(), &["add", "feature.txt"]);
    run_git(temp.path(), &["commit", "-m", "feature"]);
    temp
}

#[test]
fn test_cli_parses_internal_telemetry_flags() {
    let cli = Cli::parse_from([
        "vex",
        "--display-internal-telemetry",
        "--telemetry-json",
        "-p",
        "Reply with exactly ok.",
    ]);
    assert!(cli.display_internal_telemetry);
    assert!(cli.telemetry_json);
    assert_eq!(
        cli.project_map_only.as_deref(),
        Some("Reply with exactly ok.")
    );
}

#[test]
fn test_cli_parses_print_plan_jsonl_and_telemetry_flags_together() {
    let cli = Cli::parse_from([
        "vex",
        "--display-internal-telemetry",
        "--telemetry-json",
        "-p",
        "Reply with exactly ok.",
        "-t",
        "-m",
        "jsonl",
    ]);

    assert!(cli.display_internal_telemetry);
    assert!(cli.telemetry_json);
    assert!(cli.restrict_payload_tools);
    assert_eq!(cli.set_map_encoding, "jsonl");
    assert_eq!(
        cli.project_map_only.as_deref(),
        Some("Reply with exactly ok.")
    );
}

#[test]
fn transcript_detection_matches_following_view_dump() {
    let input =
        "mode:ready approval:none history:9 view:scrolled\n1 | > list files\ntest result: ok.";
    assert!(looks_like_session_output(input));
}

#[test]
fn transcript_detection_matches_cargo_test_noise() {
    let input = "Running tests/integration_test.rs (target/debug/deps/integration_test-b458ef4801b11438)\n\
                     test result: ok. 2 passed; 0 failed; 0 ignored;\n\
                     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s";
    assert!(looks_like_session_output(input));
}

#[test]
fn transcript_detection_keeps_normal_prompt() {
    let input = "list files in this directory and summarize in one sentence";
    assert!(!looks_like_session_output(input));
}

#[test]
fn startup_paste_filter_keeps_large_normal_prompt_blocks() {
    let input = (0..20)
        .map(|idx| format!("line {idx}: plan the next edit"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !should_ignore_startup_paste_text(&input, true),
        "normal pasted prompt blocks must not be dropped during startup"
    );
}

#[test]
fn startup_paste_filter_still_ignores_transcript_noise_during_startup() {
    let input =
        "mode:ready approval:none history:9 view:scrolled\n1 | > list files\ntest result: ok.";
    assert!(
        should_ignore_startup_paste_text(input, true),
        "startup transcript dumps must still be ignored"
    );
}

#[test]
fn test_recall_coordinates_flag_cli_parses_with_id() {
    let cli = Cli::parse_from(["vex", "--recall-coordinates", "task-1234"]);
    assert_eq!(cli.recall_coordinates, Some("task-1234".to_string()));
    assert!(cli.project_map_only.is_none());
}

#[test]
fn test_recall_coordinates_flag_cli_parses_without_id() {
    let cli = Cli::parse_from(["vex", "--recall-coordinates"]);
    assert_eq!(cli.recall_coordinates, Some(String::new()));
}

#[test]
fn test_recall_coordinates_flag_absent_is_none() {
    let cli = Cli::parse_from(["vex"]);
    assert!(cli.recall_coordinates.is_none());
}

#[test]
fn test_recall_coordinates_flag_can_be_combined_with_project_map_only() {
    let cli = Cli::parse_from(["vex", "--recall-coordinates", "task-1", "-p", "hello"]);
    assert_eq!(cli.recall_coordinates, Some("task-1".to_string()));
    assert_eq!(cli.project_map_only, Some("hello".to_string()));
}

#[test]
fn test_resolve_resume_state_unknown_id_errors() {
    let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str()) };

    let result = resolve_resume_state("does-not-exist");
    assert!(result.is_err(), "unknown task id must produce an error");

    unsafe { std::env::remove_var("VEX_STATE_DIR") };
}

#[test]
fn test_resolve_resume_state_empty_dir_returns_none() {
    let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str()) };

    let result = resolve_resume_state("").expect("empty-dir must not error");
    assert!(result.is_none(), "empty state dir must return None");

    unsafe { std::env::remove_var("VEX_STATE_DIR") };
}

#[test]
fn test_resolve_resume_state_most_recent() {
    use filetime::{FileTime, set_file_mtime};
    use vexapi::runtime::TaskState;
    let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str()) };

    let older = TaskState::new("task-older".to_string());
    older.save(temp.path()).unwrap();
    let newer = TaskState::new("task-newer".to_string());
    newer.save(temp.path()).unwrap();
    set_file_mtime(
        temp.path().join("task-older.json"),
        FileTime::from_unix_time(1_700_000_000, 0),
    )
    .unwrap();
    set_file_mtime(
        temp.path().join("task-newer.json"),
        FileTime::from_unix_time(1_700_000_001, 0),
    )
    .unwrap();

    let state = resolve_resume_state("")
        .expect("must succeed")
        .expect("must find a task");
    assert_eq!(
        state.id, "task-newer",
        "must pick the most recently modified task"
    );

    unsafe { std::env::remove_var("VEX_STATE_DIR") };
}

#[test]
fn test_resolve_resume_state_explicit_id() {
    use vexapi::runtime::TaskState;
    let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str()) };

    let state = TaskState::new("task-explicit".to_string());
    state.save(temp.path()).unwrap();

    let loaded = resolve_resume_state("task-explicit")
        .expect("must succeed")
        .expect("must find the task");
    assert_eq!(loaded.id, "task-explicit");

    unsafe { std::env::remove_var("VEX_STATE_DIR") };
}

#[test]
fn test_resolve_resume_state_explicit_id_falls_back_to_legacy_subdir() {
    use vexapi::runtime::TaskState;

    let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
    let old_cwd = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    let nested = temp.path().join("src/nested");
    let saved_state_dir = nested.join(".vex/state");
    std::fs::create_dir_all(&saved_state_dir).unwrap();

    let state = TaskState::new("task-saved".to_string());
    state.save(&saved_state_dir).unwrap();

    unsafe { std::env::remove_var("VEX_STATE_DIR") };
    std::env::set_current_dir(&nested).unwrap();

    let loaded = resolve_resume_state("task-saved")
        .expect("must succeed")
        .expect("must find the saved task");
    assert_eq!(loaded.id, "task-saved");

    std::env::set_current_dir(old_cwd).unwrap();
}

#[test]
fn task_list_interactive_render_uses_columns() {
    let entries = vec![
        super::TaskListEntry {
            id: "task-parent".to_string(),
            status: "Ready".to_string(),
            kind: "task",
            parent_task_id: None,
            agent_id: None,
        },
        super::TaskListEntry {
            id: "task-parent-reviewer-123".to_string(),
            status: "Running".to_string(),
            kind: "session-task",
            parent_task_id: Some("task-parent".to_string()),
            agent_id: Some("reviewer".to_string()),
        },
    ];

    let rendered = format_task_entries_table(&entries);

    assert!(rendered.contains("kind"), "table output: {rendered}");
    assert!(rendered.contains("origin"), "table output: {rendered}");
    assert!(rendered.contains("reviewer"), "table output: {rendered}");
    assert!(rendered.contains("task-parent"), "table output: {rendered}");
}

#[test]
fn task_list_noninteractive_render_keeps_line_mode_and_origin_copy() {
    let entries = vec![super::TaskListEntry {
        id: "task-parent-reviewer-123".to_string(),
        status: "Running".to_string(),
        kind: "session-task",
        parent_task_id: Some("task-parent".to_string()),
        agent_id: Some("reviewer".to_string()),
    }];

    let rendered = render_task_entries(&entries, false);

    assert_eq!(
        rendered,
        "session-task task-parent-reviewer-123 origin=task-parent agent=reviewer status=Running"
    );
}

#[test]
fn test_project_map_only_flag_cli_parses() {
    let cli = Cli::parse_from(["vex", "-p", "hello world"]);
    assert_eq!(cli.project_map_only, Some("hello world".to_string()));
    assert!(cli.recall_coordinates.is_none());
}

#[test]
fn test_project_map_only_long_form_parses() {
    let cli = Cli::parse_from(["vex", "--project-map-only", "hello world"]);
    assert_eq!(cli.project_map_only, Some("hello world".to_string()));
}

#[test]
fn test_project_map_only_flag_can_be_combined_with_recall_coordinates() {
    let cli = Cli::parse_from(["vex", "-p", "hello", "--recall-coordinates"]);
    assert_eq!(cli.project_map_only, Some("hello".to_string()));
    assert_eq!(cli.recall_coordinates, Some(String::new()));
}

#[test]
fn test_project_map_only_flag_can_be_combined_with_plan_policy_and_jsonl_output() {
    let cli = Cli::parse_from(["vex", "-p", "hello", "-t", "-m", "jsonl"]);

    assert_eq!(cli.project_map_only, Some("hello".to_string()));
    assert!(cli.restrict_payload_tools);
    assert_eq!(cli.set_map_encoding, "jsonl");
}

#[test]
fn test_restrict_payload_tools_maps_to_plan_policy() {
    assert_eq!(tool_policy_from_cli(false, true), ToolPolicy::Plan);
}

#[test]
fn test_view_intended_trajectory_maps_to_plan_policy() {
    assert_eq!(tool_policy_from_cli(true, false), ToolPolicy::Plan);
}

#[test]
fn test_force_defaults_batch_auto_approve_to_task_scope() {
    let mut config = Config::default_for_tui();
    config.force = true;

    assert_eq!(
        default_auto_approve_scope(&config, None),
        Some(AutoApproveScope::Task)
    );
}

#[test]
fn test_explicit_auto_approve_scope_takes_precedence_over_force_default() {
    let mut config = Config::default_for_tui();
    config.force = true;

    assert_eq!(
        default_auto_approve_scope(&config, Some(AutoApproveScope::Once)),
        Some(AutoApproveScope::Once)
    );
}

#[test]
fn test_bypass_integrity_locks_forces_disk_policy_off_for_process() {
    let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
    unsafe { std::env::set_var("VEX_DISK_POLICY", "strict") };

    let mut config = Config::default_for_tui();
    config.bypass_policy = true;
    apply_process_policy_overrides(&config);

    assert_eq!(
        vexapi::disk_policy::resolve_policy_mode(),
        DiskPolicyMode::Off
    );

    vexapi::disk_policy::set_process_policy_override(None);
    unsafe { std::env::remove_var("VEX_DISK_POLICY") };
}

#[test]
fn test_set_map_encoding_jsonl_maps_to_jsonl_output() {
    assert_eq!(map_encoding_to_output_format("jsonl"), OutputFormat::Jsonl);
}

#[test]
fn test_cli_rejects_obsolete_json_map_encoding_alias() {
    let err = match Cli::try_parse_from(["vex", "-p", "hello", "-m", "json"]) {
        Ok(_) => panic!("expected clap to reject the obsolete json alias"),
        Err(err) => err,
    };

    assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    assert!(
        err.to_string().contains("jsonl"),
        "unexpected clap error: {err}"
    );
}

#[test]
fn test_help_paths_emit_display_help_without_running_the_binary() {
    for argv in [
        &["vex", "--help"][..],
        &["vex", "completions", "--help"][..],
    ] {
        let err = Cli::command()
            .try_get_matches_from(argv)
            .expect_err("help argv should short-circuit with DisplayHelp");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }
}

#[test]
fn test_completions_cli_parses_zsh() {
    let cli = Cli::parse_from(["vex", "completions", "zsh"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Completions { shell: Shell::Zsh })
    ));
}

#[test]
fn test_completions_cli_parses_bash() {
    let cli = Cli::parse_from(["vex", "completions", "bash"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Completions { shell: Shell::Bash })
    ));
}

#[test]
fn test_completions_cli_parses_fish() {
    let cli = Cli::parse_from(["vex", "completions", "fish"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Completions { shell: Shell::Fish })
    ));
}

#[test]
fn test_completions_cli_parses_powershell() {
    let cli = Cli::parse_from(["vex", "completions", "powershell"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Completions {
            shell: Shell::PowerShell
        })
    ));
}

#[test]
fn test_install_hooks_cli_parses() {
    let cli = Cli::parse_from(["vex", "install-hooks"]);
    assert!(matches!(cli.command, Some(Commands::InstallHooks)));
}

#[test]
fn test_doctor_cli_parses() {
    let cli = Cli::parse_from(["vex", "doctor"]);
    assert!(matches!(cli.command, Some(Commands::Doctor)));
}

#[test]
fn test_doctor_cli_rejects_obsolete_json_flag() {
    assert!(Cli::try_parse_from(["vex", "doctor", "--json"]).is_err());
}

#[test]
fn test_privacy_cli_parses() {
    let cli = Cli::parse_from(["vex", "privacy"]);
    assert!(matches!(cli.command, Some(Commands::Privacy)));
}

#[test]
fn test_credentials_set_cli_parses_account() {
    let cli = Cli::parse_from(["vex", "credentials", "set", "model-token"]);
    match cli.command {
        Some(Commands::Credentials {
            sub: CredentialsCommands::Set { account },
        }) => {
            assert_eq!(account, "model-token");
        }
        _ => panic!("expected credentials set"),
    }
}

#[test]
fn test_credentials_set_cli_rejects_obsolete_stdin_flag() {
    assert!(Cli::try_parse_from(["vex", "credentials", "set", "model-token", "--stdin"]).is_err());
}

#[test]
fn test_credentials_set_cli_rejects_obsolete_from_env_flag() {
    assert!(
        Cli::try_parse_from([
            "vex",
            "credentials",
            "set",
            "model-token",
            "--from-env",
            "VEX_MODEL_TOKEN"
        ])
        .is_err()
    );
}

#[test]
fn test_credentials_set_cli_rejects_positional_secret() {
    assert!(Cli::try_parse_from(["vex", "credentials", "set", "model-token", "secret"]).is_err());
}

#[test]
fn test_credentials_action_from_cli_requires_non_argv_secret_source() {
    let err = resolve_credentials_secret("model-token", false, None, false, |_| unreachable!())
        .unwrap_err();
    assert!(
        !err.to_string().is_empty(),
        "expected a non-empty error describing the unavailable sources"
    );
    assert!(
        err.to_string().contains("stdin") || err.to_string().contains("redirect"),
        "error must describe the stdin redirect path: {err}"
    );
}

#[test]
fn test_resolve_credentials_secret_uses_interactive_prompt_when_tty_available() {
    let secret = resolve_credentials_secret("model-token", false, None, true, |account| {
        assert_eq!(account, "model-token");
        Ok("prompt-secret".to_string())
    })
    .expect("interactive secret");

    assert_eq!(secret, "prompt-secret");
}

#[test]
fn test_read_secret_from_reader_strips_one_trailing_newline() {
    let secret = read_secret_from_reader(Cursor::new("token-value\n")).expect("secret");
    assert_eq!(secret, "token-value");
}

#[test]
fn test_read_secret_from_env_var_reads_named_value() {
    let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
    unsafe { std::env::set_var("VEX_TEST_SECRET", "secret-value") };
    let secret = read_secret_from_env_var("VEX_TEST_SECRET").expect("env secret");
    assert_eq!(secret, "secret-value");
    unsafe { std::env::remove_var("VEX_TEST_SECRET") };
}

#[test]
fn test_export_cli_parses_task_id() {
    let cli = Cli::parse_from(["vex", "export", "task-123"]);
    match cli.command {
        Some(Commands::Export { task_id }) => {
            assert_eq!(task_id, "task-123");
        }
        _ => panic!("expected export subcommand"),
    }
}

#[test]
fn test_export_cli_rejects_obsolete_format_flag() {
    assert!(Cli::try_parse_from(["vex", "export", "task-123", "--format", "markdown"]).is_err());
}

#[test]
fn test_export_cli_rejects_obsolete_output_flag() {
    assert!(Cli::try_parse_from(["vex", "export", "task-123", "--output", "/tmp/out.md"]).is_err());
}

#[test]
fn test_branch_cli_parses_name() {
    let cli = Cli::parse_from(["vex", "branch", "feature/demo"]);
    match cli.command {
        Some(Commands::Branch { name }) => assert_eq!(name, "feature/demo"),
        _ => panic!("expected branch subcommand"),
    }
}

#[test]
fn test_pr_summary_cli_parses() {
    let cli = Cli::parse_from(["vex", "pr-summary"]);
    assert!(matches!(cli.command, Some(Commands::PrSummary)));
}

#[test]
fn test_uninstall_hooks_cli_parses() {
    let cli = Cli::parse_from(["vex", "uninstall-hooks"]);
    assert!(matches!(cli.command, Some(Commands::UninstallHooks)));
}

#[test]
fn test_skills_list_cli_parses() {
    let cli = Cli::parse_from(["vex", "skills", "list"]);
    assert!(matches!(
        cli.command,
        Some(Commands::Skills {
            sub: SkillsCommands::List
        })
    ));
}

#[test]
fn test_skills_remove_cli_parses() {
    let cli = Cli::parse_from(["vex", "skills", "remove", "edit-loop"]);
    match cli.command {
        Some(Commands::Skills {
            sub: SkillsCommands::Remove { name },
        }) => {
            assert_eq!(name, "edit-loop");
        }
        _ => panic!("expected skills remove"),
    }
}

#[test]
fn test_init_cli_parses() {
    let cli = Cli::parse_from(["vex", "init"]);
    assert!(matches!(cli.command, Some(Commands::Init)));
}

#[test]
fn test_init_cli_rejects_obsolete_dir_flag() {
    assert!(Cli::try_parse_from(["vex", "init", "--dir", "/tmp/example"]).is_err());
}

#[test]
fn test_vex_init_creates_vex_dir() {
    let temp = tempfile::tempdir().unwrap();
    run_init(temp.path()).unwrap();
    assert!(temp.path().join(".vex").is_dir());
}

#[test]
fn test_vex_init_writes_config_toml_skeleton() {
    let temp = tempfile::tempdir().unwrap();
    run_init(temp.path()).unwrap();
    let content = std::fs::read_to_string(temp.path().join(".vex/config.toml")).unwrap();
    assert!(temp.path().join(".vex/config.toml").exists());
    assert!(content.contains("# model_backend = \"local-runtime\""));
    assert!(content.contains("# [api]"));
    assert!(content.contains("# [[hooks]]"));
    assert!(content.contains("# [[mcp_servers]]"));
    assert!(
        !content.lines().any(|line| line.starts_with("    ")),
        "config template must not contain leading indentation"
    );
}

#[test]
fn test_vex_init_writes_agents_md_template() {
    let temp = tempfile::tempdir().unwrap();
    run_init(temp.path()).unwrap();
    let content = std::fs::read_to_string(temp.path().join("AGENTS.md")).unwrap();
    assert!(content.contains("Project Agents"));
    assert!(content.contains("project-specific guidance"));
    assert!(
        !content.lines().any(|line| line.starts_with("    ")),
        "agents template must not contain leading indentation"
    );
}

#[test]
fn test_vex_init_skips_existing_files() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join(".vex/config.toml");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(&config_path, "existing\n").unwrap();

    let summary = run_init(temp.path()).unwrap();
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert_eq!(content, "existing\n", "must not overwrite existing file");
    assert!(
        temp.path().join("AGENTS.md").exists(),
        "missing files must still be created"
    );
    assert!(
        summary
            .iter()
            .any(|line| line == "[init] skip (exists): .vex/config.toml")
    );
}

#[test]
fn test_vex_init_config_keys_match_normative_list() {
    let keys = extract_init_template_keys(INIT_CONFIG_TEMPLATE);
    let expected = INIT_CONFIG_NORMATIVE_KEYS
        .iter()
        .map(|value| value.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(keys, expected);
}

#[test]
fn test_vex_init_does_not_start_agent_loop() {
    let temp = tempfile::tempdir().unwrap();
    let summary = run_init(temp.path()).unwrap();
    assert!(!temp.path().join(".vex/state").exists());
    assert_eq!(summary.last().map(String::as_str), Some("[init] done"));
}

#[test]
fn test_vex_init_writes_validate_commands_stub() {
    let temp = tempfile::tempdir().unwrap();
    run_init(temp.path()).unwrap();
    let content = std::fs::read_to_string(temp.path().join(".vex/validate.toml")).unwrap();
    assert!(content.contains("# [[commands]]"));
    assert!(
        !content.lines().any(|line| line.starts_with("    ")),
        "validate template must not contain leading indentation"
    );
}

#[tokio::test]
async fn test_vex_branch_creates_git_branch() {
    let _env_lock = crate::tests::test_support::ENV_LOCK.lock().await;
    let repo = init_git_repo();
    let state_dir = repo.path().join("state");
    unsafe { std::env::set_var("VEX_STATE_DIR", state_dir.as_os_str()) };

    let summary = run_branch(repo.path(), "feature/demo").await.unwrap();
    let branch = git_stdout(repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);

    assert_eq!(branch, "feature/demo");
    assert!(
        summary
            .iter()
            .any(|line| line == "[branch] created: feature/demo")
    );

    unsafe { std::env::remove_var("VEX_STATE_DIR") };
}

#[tokio::test]
async fn test_vex_branch_records_in_task_state() {
    let _env_lock = crate::tests::test_support::ENV_LOCK.lock().await;
    let repo = init_git_repo();
    let state_dir = repo.path().join("state");
    unsafe { std::env::set_var("VEX_STATE_DIR", state_dir.as_os_str()) };

    let state = TaskState::new("task-branch".to_string());
    state.save(&state_dir).unwrap();

    run_branch(repo.path(), "feature/task-state").await.unwrap();

    let loaded = TaskState::load(&state_dir, "task-branch").unwrap();
    assert_eq!(loaded.branch_name.as_deref(), Some("feature/task-state"));

    unsafe { std::env::remove_var("VEX_STATE_DIR") };
}

#[tokio::test]
async fn test_vex_pr_summary_assembles_merge_base_diff() {
    let repo = init_pr_summary_repo();

    let prompt = prepare_pr_summary_prompt(repo.path()).await.unwrap();

    assert!(prompt.contains("refs/remotes/origin/main"));
    assert!(prompt.contains("feature/pr-summary"));
    assert!(prompt.contains("feature.txt"));
    assert!(prompt.contains("+feature change"));
}

#[tokio::test]
async fn test_vex_pr_summary_outputs_to_stdout() {
    let repo = init_pr_summary_repo();
    let config = Config::default_for_tui();

    let rendered = run_pr_summary_with_batch(repo.path(), config, |task, opts, _| async move {
        assert_eq!(opts.max_turns, Some(1));
        assert_eq!(opts.format, OutputFormat::Text);
        assert!(task.contains("feature.txt"));
        Ok(BatchResult {
            status: TaskStatus::Completed,
            output_lines: vec![
                "Title: Example PR".to_string(),
                String::new(),
                "## Summary".to_string(),
            ],
            turn_count: 1,
            task_id: "task-pr-summary".to_string(),
        })
    })
    .await
    .unwrap();

    assert!(rendered.starts_with("Title: Example PR"));
}

#[tokio::test]
async fn test_vex_pr_summary_does_not_start_tui() {
    let repo = init_pr_summary_repo();
    let config = Config::default_for_tui();
    let state_dir = repo.path().join(".vex/state");
    assert!(!state_dir.exists());

    let rendered = run_pr_summary_with_batch(repo.path(), config, |_, _, _| async move {
        Ok(BatchResult {
            status: TaskStatus::Completed,
            output_lines: vec!["Title: No TUI".to_string()],
            turn_count: 1,
            task_id: "task-pr-summary".to_string(),
        })
    })
    .await
    .unwrap();

    assert_eq!(rendered, "Title: No TUI");
    assert!(
        !state_dir.exists(),
        "pr-summary must not bootstrap TUI state"
    );
}
