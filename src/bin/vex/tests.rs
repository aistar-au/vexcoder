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
use vexcoder::app::TuiMode;
use vexcoder::batch_mode::{AutoApproveScope, BatchResult, OutputFormat};
use vexcoder::config::Config;
use vexcoder::disk_policy::DiskPolicyMode;
use vexcoder::init::{
    INIT_CONFIG_NORMATIVE_KEYS, INIT_CONFIG_TEMPLATE, extract_init_template_keys, run_init,
};
use vexcoder::pr_summary::{prepare_pr_summary_prompt, run_branch, run_pr_summary_with_batch};
use vexcoder::runtime::{TaskState, TaskStatus, ToolPolicy};
use vexcoder::startup::{looks_like_session_output, should_ignore_startup_paste_text};
use vexcoder::tui_frontend::{
    active_file_picker, active_slash_picker, apply_file_picker_selection,
    apply_slash_picker_selection, build_file_overlay, build_slash_overlay,
    file_picker_is_dismissed, render_file_picker_hint, render_slash_picker_hint,
    slash_prefix_token,
};
use vexcoder::ui::editor::InputEditor;
use vexcoder::ui::editor::file_mention_range;

mod test_support {
    pub struct EnvLock(tokio::sync::Mutex<()>);
    impl EnvLock {
        pub const fn new() -> Self { Self(tokio::sync::Mutex::const_new(())) }
        pub fn blocking_lock(&self) -> EnvLockGuard<'_> {
            EnvLockGuard { _guard: self.0.blocking_lock() }
        }
    }
    pub struct EnvLockGuard<'a> { _guard: tokio::sync::MutexGuard<'a, ()> }
    impl EnvLockGuard<'_> {
        #[allow(unsafe_code)]
        pub fn set_var(&self, key: &str, val: impl AsRef<std::ffi::OsStr>) {
            unsafe { std::env::set_var(key, val) }
        }
        #[allow(unsafe_code)]
        pub fn remove_var(&self, key: &str) { unsafe { std::env::remove_var(key) } }
    }
    pub static ENV_LOCK: EnvLock = EnvLock::new();
}

#[path = "tests/picker.rs"]
mod picker;

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let out = Command::new("git").current_dir(repo).args(args).output().unwrap();
    assert!(out.status.success(), "git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
}

fn init_git_repo() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for args in [
        vec!["init"], vec!["checkout", "-b", "main"],
        vec!["config", "user.name", "Test"], vec!["config", "user.email", "t@t.com"],
    ] { run_git(temp.path(), &args); }
    std::fs::write(temp.path().join("README.md"), "hello\n").unwrap();
    run_git(temp.path(), &["add", "README.md"]);
    run_git(temp.path(), &["commit", "-m", "initial"]);
    temp
}

#[test]
fn cli_key_flags_parse_correctly() {
    let cli = Cli::parse_from(["vex", "--display-internal-telemetry", "--telemetry-json", "-p", "task"]);
    assert!(cli.display_internal_telemetry && cli.telemetry_json);
    assert_eq!(cli.project_map_only.as_deref(), Some("task"));

    let cli2 = Cli::parse_from(["vex", "--recall-coordinates", "task-1", "-t", "-m", "jsonl"]);
    assert_eq!(cli2.recall_coordinates, Some("task-1".to_string()));
    assert!(cli2.restrict_payload_tools);
    assert_eq!(cli2.set_map_encoding, "jsonl");

    let cli3 = Cli::parse_from(["vex", "--force"]);
    assert!(matches!(default_auto_approve_scope(&cli3), Some(AutoApproveScope::Task)));
}

#[test]
fn transcript_detection_distinguishes_session_output_from_plain_prompts() {
    assert!(looks_like_session_output(
        "mode:ready approval:none history:9 view:scrolled\n1 | > list files\ntest result: ok."
    ));
    assert!(!looks_like_session_output("list files in this directory"));
}

#[test]
fn startup_paste_filter_ignores_transcript_noise_but_keeps_normal_prompts() {
    let normal = (0..20).map(|i| format!("line {i}: plan the next edit")).collect::<Vec<_>>().join("\n");
    assert!(!should_ignore_startup_paste_text(&normal, true));
    assert!(should_ignore_startup_paste_text(
        "mode:ready approval:none history:9\n1 | > list files\ntest result: ok.", true
    ));
}

#[test]
fn resolve_resume_state_returns_most_recent_task_and_errors_on_unknown_id() {
    use filetime::{FileTime, set_file_mtime};
    let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    _env_lock.set_var("VEX_STATE_DIR", temp.path().as_os_str());
    TaskState::new("task-older".to_string()).save(temp.path()).unwrap();
    TaskState::new("task-newer".to_string()).save(temp.path()).unwrap();
    set_file_mtime(temp.path().join("task-older.json"), FileTime::from_unix_time(1_700_000_000, 0)).unwrap();
    set_file_mtime(temp.path().join("task-newer.json"), FileTime::from_unix_time(1_700_000_001, 0)).unwrap();
    assert_eq!(resolve_resume_state("").unwrap().expect("must find a task").id, "task-newer");
    assert!(resolve_resume_state("does-not-exist").is_err());
    _env_lock.remove_var("VEX_STATE_DIR");
}
