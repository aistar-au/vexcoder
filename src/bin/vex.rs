use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use vexcoder::app::{run_tui_session, serve_facade_local_api};
use vexcoder::batch_mode::{run_batch, BatchRunOpts, OutputFormat};
use vexcoder::config::Config;
use vexcoder::doctor::run_doctor;
use vexcoder::exec::{parse_exec_command, run_exec};
use vexcoder::export::{render_task_export, write_export_output, ExportFormat};
use vexcoder::init::run_init;
use vexcoder::pr_summary::{run_branch, run_pr_summary};
use vexcoder::runtime::{TaskState, TaskStatus};
use vexcoder::startup::{emit_model_endpoint_warnings, prompt_tui_startup_config};
use vexcoder::tui_frontend::ManagedTuiFrontend;

#[derive(Parser)]
#[command(name = "vex", about = "vexcoder -- zero-licensing-cost coding agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Resume a saved task by ID, or omit the value for an interactive
    /// selection of the most-recent saved task.
    /// Example: `vex --resume task-1234` or just `vex --resume`.
    #[arg(
        long,
        num_args(0..=1),
        default_missing_value = ""
    )]
    resume: Option<String>,

    /// Run a single prompt turn non-interactively and print the result to
    /// stdout.  Reads additional content from stdin when stdin is not a TTY.
    /// Example: `vex -p "summarise this file" < README.md`
    #[arg(short = 'p', long = "print")]
    print_prompt: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a non-interactive batch task.
    Exec {
        #[arg(long, conflicts_with = "task_file")]
        task: Option<String>,
        #[arg(long = "task-file", conflicts_with = "task")]
        task_file: Option<String>,
        #[arg(long)]
        max_turns: Option<usize>,
        #[arg(long = "auto-approve", value_parser = ["once", "task"])]
        auto_approve: Option<String>,
        #[arg(long)]
        output: Option<String>,
        #[arg(long, default_value = "jsonl", value_parser = ["jsonl", "text"])]
        format: String,
    },
    /// Generate shell completion scripts and write them to stdout.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Install the vexcoder `prepare-commit-msg` hook.
    InstallHooks,
    /// Remove the vexcoder `prepare-commit-msg` hook.
    UninstallHooks,
    /// Manage the local skills registry.
    Skills {
        #[command(subcommand)]
        sub: SkillsCommands,
    },
    /// Scaffold a new vex workspace (`.vex/config.toml`, `AGENTS.md`,
    /// `.vex/validate.toml`).  Non-destructive: skips files that already exist.
    Init {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Create a new git branch from HEAD and record it on the most recent task.
    Branch { name: String },
    /// Generate a pull request title and body draft for the current branch.
    PrSummary,
    /// Configuration migration utilities.
    Migrate {
        #[command(subcommand)]
        sub: MigrateCommands,
    },
    /// Check environment health without starting the agent loop.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Export a saved task to JSONL or Markdown.
    Export {
        task_id: String,
        #[arg(long, default_value = "jsonl", value_parser = ["jsonl", "markdown"])]
        format: String,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Run the local API transport adapter.
    Serve {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
}

#[derive(Subcommand)]
enum SkillsCommands {
    /// List installed skills.
    List,
    /// Install a skill from a git repository URL or tarball URL.
    Install {
        /// Git repository URL or tarball URL.
        source: String,
        /// Select a subdirectory within the fetched source as the skill root.
        #[arg(long)]
        subdir: Option<String>,
    },
    /// Remove an installed skill by name.
    Remove {
        /// Name of the skill to remove.
        name: String,
    },
}

#[derive(Subcommand)]
enum MigrateCommands {
    /// Map pre-ADR-022 VEX_* env var values to current config.toml keys.
    /// Reads from the environment and writes a fragment to stdout by default.
    /// Non-destructive unless `--output <path>` is passed explicitly.
    Config {
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

fn emit_migrate_config_output(output_path: Option<&Path>) -> Result<()> {
    let fragment = vexcoder::config::migrate_config_from_env(&[]);
    if let Some(path) = output_path {
        std::fs::write(path, fragment)?;
    } else {
        print!("{}", fragment);
    }
    Ok(())
}

// ── PM-01: resolve task state for --resume ────────────────────────────────────

/// Load a `TaskState` for `--resume`.  An empty `task_id` means "pick the most
/// recent saved task"; a non-empty `task_id` loads that specific task.
/// Returns `None` only when no tasks exist yet (empty-id path).
fn resolve_resume_state(task_id: &str) -> Result<Option<TaskState>> {
    if task_id.is_empty() {
        match TaskState::state_files().into_iter().next() {
            Some(file) => Ok(Some(TaskState::load(&file.dir, &file.id)?)),
            None => Ok(None),
        }
    } else {
        Ok(Some(TaskState::load_from_search_dirs(task_id)?))
    }
}

// ── PM-03: --print one-shot mode ──────────────────────────────────────────────

/// Collect stdin when it is not a TTY (pipe / redirect) and prepend it to the
/// prompt so that `vex -p "summarise" < file.txt` works naturally.
fn read_stdin_if_piped() -> Option<String> {
    use std::io::Read;
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    if buf.trim().is_empty() {
        None
    } else {
        Some(buf)
    }
}

async fn run_print(
    prompt: String,
    config: Config,
    resume_state: Option<TaskState>,
) -> Result<ExitCode> {
    let full_prompt = match read_stdin_if_piped() {
        Some(stdin_content) => format!("{stdin_content}\n{prompt}"),
        None => prompt,
    };

    let opts = BatchRunOpts {
        max_turns: Some(1),
        auto_approve: None,
        format: OutputFormat::Text,
        resume_state,
    };

    let result = run_batch(full_prompt, opts, &config).await?;
    print!("{}", result.output_lines.join("\n"));

    Ok(exit_code_for_status(result.status))
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

// ── main ───────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<ExitCode> {
    let cli = Cli::parse();

    // Subcommands take unconditional priority.
    match cli.command {
        Some(Commands::Exec {
            task,
            task_file,
            max_turns,
            auto_approve,
            output,
            format,
        }) => {
            let exec_args =
                parse_exec_command(task, task_file, max_turns, auto_approve, output, format)?;
            let config = Config::load()?;
            config.validate()?;
            return run_exec(exec_args, config).await;
        }
        Some(Commands::Completions { shell }) => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::InstallHooks) => {
            let cwd = std::env::current_dir()?;
            vexcoder::git_hooks::install_hooks(&cwd)?;
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::UninstallHooks) => {
            let cwd = std::env::current_dir()?;
            vexcoder::git_hooks::uninstall_hooks(&cwd)?;
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::Skills { sub }) => {
            let cwd = std::env::current_dir()?;
            let mut registry = vexcoder::skills::SkillsRegistry::load(&cwd)?;
            match sub {
                SkillsCommands::List => registry.list(),
                SkillsCommands::Install { source, subdir } => {
                    registry.install(&source, subdir.as_deref())?;
                }
                SkillsCommands::Remove { name } => {
                    registry.remove(&name)?;
                }
            }
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::Init { dir }) => {
            let cwd = match dir {
                Some(path) => path,
                None => std::env::current_dir()?,
            };
            let summary = run_init(&cwd)?;
            print_lines(&summary);
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::Branch { name }) => {
            let cwd = std::env::current_dir()?;
            let summary = run_branch(&cwd, &name).await?;
            print_lines(&summary);
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::PrSummary) => {
            let cwd = std::env::current_dir()?;
            let config = Config::load()?;
            config.validate()?;
            let rendered = run_pr_summary(&cwd, &config).await?;
            print!("{rendered}");
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::Migrate { sub }) => match sub {
            MigrateCommands::Config { output } => {
                emit_migrate_config_output(output.as_deref())?;
                return Ok(ExitCode::SUCCESS);
            }
        },
        Some(Commands::Doctor { json }) => {
            let cwd = std::env::current_dir()?;
            let report = run_doctor(&cwd).await;
            let rendered = if json {
                report.render_json()?
            } else {
                report.render_text()
            };
            println!("{rendered}");
            return Ok(if report.has_failures() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            });
        }
        Some(Commands::Export {
            task_id,
            format,
            output,
            force,
        }) => {
            let format = ExportFormat::parse(&format)?;
            let state = TaskState::load_from_search_dirs(&task_id)?;
            let rendered = render_task_export(&state, format)?;
            if write_export_output(&rendered, output.as_deref(), force)?.is_none() {
                print!("{rendered}");
            }
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::Serve { host, port }) => {
            let config = Config::load()?;
            config.validate()?;
            serve_facade_local_api(config, host, port).await?;
            return Ok(ExitCode::SUCCESS);
        }
        None => {}
    }

    let mut config = Config::load()?;

    let resume_state = match cli.resume.as_deref() {
        Some(task_id) => match resolve_resume_state(task_id)? {
            Some(state) => Some(state),
            None => {
                eprintln!("[resume] no saved tasks found");
                return Ok(ExitCode::FAILURE);
            }
        },
        None => None,
    };

    // PM-03: -p/--print one-shot mode.
    if let Some(prompt) = cli.print_prompt {
        config.validate()?;
        emit_model_endpoint_warnings(&config);
        return run_print(prompt, config, resume_state).await;
    }

    config = prompt_tui_startup_config(config)?;
    config.validate()?;
    emit_model_endpoint_warnings(&config);

    // PM-01: --resume startup flag.
    if let Some(state) = resume_state {
        let mut frontend = ManagedTuiFrontend::new()?;
        run_tui_session(config, Some(state), &mut frontend).await?;
        return Ok(ExitCode::SUCCESS);
    }

    // Default: interactive TUI.
    let mut frontend = ManagedTuiFrontend::new()?;
    run_tui_session(config, None, &mut frontend).await?;
    Ok(ExitCode::SUCCESS)
}

fn exit_code_for_status(status: TaskStatus) -> ExitCode {
    match status {
        TaskStatus::Completed => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        emit_migrate_config_output, resolve_resume_state, Cli, Commands, MigrateCommands,
        SkillsCommands,
    };
    use clap::Parser;
    use clap_complete::Shell;
    use std::path::PathBuf;
    use std::process::Command;
    use vexcoder::app::TuiMode;
    use vexcoder::batch_mode::{BatchResult, OutputFormat};
    use vexcoder::config::Config;
    use vexcoder::init::{
        extract_init_template_keys, run_init, INIT_CONFIG_NORMATIVE_KEYS, INIT_CONFIG_TEMPLATE,
    };
    use vexcoder::pr_summary::{prepare_pr_summary_prompt, run_branch, run_pr_summary_with_batch};
    use vexcoder::runtime::{TaskState, TaskStatus};
    use vexcoder::startup::{looks_like_terminal_transcript, should_ignore_startup_paste_text};
    use vexcoder::tui_frontend::{
        active_file_picker, active_slash_picker, apply_file_picker_selection,
        apply_slash_picker_selection, file_picker_is_dismissed, render_file_picker_hint,
        render_slash_picker_hint, slash_prefix_token,
    };
    use vexcoder::ui::editor::file_mention_range;
    use vexcoder::ui::editor::InputEditor;

    mod test_support {
        pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    }

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
    fn test_migrate_config_maps_legacy_messages_value() {
        let out = vexcoder::config::migrate_config_from_env(&[(
            "VEX_API_PROTOCOL",
            concat!("anth", "ropic"),
        )]);
        assert!(
            out.contains("model_protocol = \"messages-v1\""),
            "migrate output: {out}"
        );
    }

    #[test]
    fn test_migrate_config_maps_structured_tool_protocol_on() {
        let out =
            vexcoder::config::migrate_config_from_env(&[("VEX_STRUCTURED_TOOL_PROTOCOL", "on")]);
        assert!(
            out.contains("tool_call_mode = \"structured\""),
            "migrate output: {out}"
        );
    }

    #[test]
    fn test_migrate_config_header_comment_present() {
        let out = vexcoder::config::migrate_config_from_env(&[]);
        assert!(out.starts_with("# generated by vex migrate config"));
    }

    #[test]
    fn test_migrate_config_cli_parses_output_flag() {
        let cli = Cli::parse_from(["vex", "migrate", "config", "--output", "/tmp/migrate.toml"]);
        match cli.command {
            Some(Commands::Migrate {
                sub: MigrateCommands::Config { output },
            }) => {
                assert_eq!(output, Some(PathBuf::from("/tmp/migrate.toml")));
            }
            _ => panic!("expected migrate config subcommand"),
        }
    }

    #[test]
    fn test_emit_migrate_config_output_writes_requested_file() {
        let temp = tempfile::tempdir().unwrap();
        let output_path = temp.path().join("migrate.toml");

        emit_migrate_config_output(Some(output_path.as_path())).unwrap();

        let content = std::fs::read_to_string(output_path).unwrap();
        assert!(content.starts_with("# generated by vex migrate config"));
    }

    #[test]
    fn transcript_detection_matches_following_view_dump() {
        let input =
            "mode:ready approval:none history:9 view:scrolled\n1 | > list files\ntest result: ok.";
        assert!(looks_like_terminal_transcript(input));
    }

    #[test]
    fn transcript_detection_matches_cargo_test_noise() {
        let input = "Running tests/integration_test.rs (target/debug/deps/integration_test-b458ef4801b11438)\n\
                     test result: ok. 2 passed; 0 failed; 0 ignored;\n\
                     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s";
        assert!(looks_like_terminal_transcript(input));
    }

    #[test]
    fn transcript_detection_keeps_normal_prompt() {
        let input = "list files in this directory and summarize in one sentence";
        assert!(!looks_like_terminal_transcript(input));
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
    fn file_mention_range_tracks_token_under_cursor() {
        let input = "inspect @src/app/inp more";
        let cursor = input.find("inp").unwrap() + 3;
        let range = file_mention_range(input, cursor).expect("mention range");
        assert_eq!(&input[range], "@src/app/inp");
    }

    #[test]
    fn file_picker_hint_marks_selected_entry() {
        let hint = render_file_picker_hint(
            "inp",
            &["src/app/input.rs".into(), "src/app/inline.rs".into()],
            1,
        );
        assert!(hint.contains("> [file] src/app/inline.rs"));
        assert!(hint.contains("  [file] src/app/input.rs"));
    }

    #[test]
    fn apply_file_picker_selection_replaces_partial_token() {
        let mut editor = InputEditor::new();
        editor.insert_str("inspect @inp");
        let range = file_mention_range(editor.buffer(), editor.cursor()).expect("mention range");
        apply_file_picker_selection(&mut editor, &range, "src/app/input.rs");
        assert_eq!(editor.buffer(), "inspect @src/app/input.rs ");
    }

    #[test]
    fn active_file_picker_uses_tui_file_matches() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/app")).unwrap();
        std::fs::write(temp.path().join("src/app/input.rs"), "fn hint() {}\n").unwrap();

        let mut config = Config::default_for_tui();
        config.working_dir = temp.path().to_path_buf();
        let mode = TuiMode::new_with_config(None, config);

        let picker =
            active_file_picker(&mode, "inspect @inp", "inspect @inp".len()).expect("active picker");
        assert_eq!(picker.prefix, "inp");
        assert!(picker.matches.contains(&"src/app/input.rs".to_string()));
    }

    #[test]
    fn dismissed_file_picker_stays_suppressed_until_input_changes() {
        let input = "inspect @inp";
        let range = file_mention_range(input, input.len()).expect("mention range");
        let dismissed = Some((input.to_string(), range));

        assert!(file_picker_is_dismissed(
            dismissed.as_ref(),
            "inspect @inp",
            "inspect @inp".len()
        ));
        assert!(file_picker_is_dismissed(
            dismissed.as_ref(),
            "inspect @inp",
            "inspect @i".len()
        ));
        assert!(!file_picker_is_dismissed(
            dismissed.as_ref(),
            "inspect @input",
            "inspect @input".len()
        ));
    }

    // -- @ file picker interactivity tests ------------------------------------

    #[test]
    fn render_file_picker_hint_empty_matches_no_prefix() {
        let hint = render_file_picker_hint("", &[], 0);
        assert!(hint.contains("[file] no files available"), "hint: {hint}");
    }

    #[test]
    fn render_file_picker_hint_empty_matches_with_prefix() {
        let hint = render_file_picker_hint("nonexist", &[], 0);
        assert!(
            hint.contains("[file] no matches for nonexist"),
            "hint: {hint}"
        );
    }

    #[test]
    fn render_file_picker_hint_clamps_selected_past_end() {
        let hint = render_file_picker_hint(
            "x",
            &["src/x.rs".into(), "src/xy.rs".into()],
            999, // way past end
        );
        assert!(
            hint.contains("> [file] src/xy.rs"),
            "should clamp to last entry: {hint}"
        );
    }

    #[test]
    fn render_file_picker_hint_single_match() {
        let hint = render_file_picker_hint("exact", &["src/exact.rs".into()], 0);
        assert!(hint.contains("[file] 1 match(es)"));
        assert!(hint.contains("> [file] src/exact.rs"));
    }

    #[test]
    fn apply_file_picker_selection_bare_at_replaces_correctly() {
        let mut editor = InputEditor::new();
        editor.insert_str("@");
        let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
        apply_file_picker_selection(&mut editor, &range, "src/main.rs");
        assert_eq!(editor.buffer(), "@src/main.rs ");
    }

    #[test]
    fn apply_file_picker_selection_mid_sentence() {
        let mut editor = InputEditor::new();
        editor.insert_str("look at @inp and fix");
        // Move cursor to be inside "@inp" token
        editor.input_state.cursor = "look at @inp".len();
        let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
        apply_file_picker_selection(&mut editor, &range, "src/app/input.rs");
        assert_eq!(editor.buffer(), "look at @src/app/input.rs and fix");
    }

    #[test]
    fn apply_file_picker_selection_already_has_trailing_space() {
        let mut editor = InputEditor::new();
        editor.insert_str("@src ");
        editor.input_state.cursor = 4; // cursor on "src" before space
        let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
        apply_file_picker_selection(&mut editor, &range, "src/lib.rs");
        // Should not double-space
        assert_eq!(editor.buffer(), "@src/lib.rs ");
    }

    #[test]
    fn file_picker_is_dismissed_none_returns_false() {
        assert!(!file_picker_is_dismissed(None, "@test", 5));
    }

    #[test]
    fn active_file_picker_no_at_returns_none() {
        let config = Config::default_for_tui();
        let mode = TuiMode::new_with_config(None, config);

        assert!(active_file_picker(&mode, "hello world", 5).is_none());
    }

    #[test]
    fn active_file_picker_bare_at_returns_all_matches() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/a.rs"), "").unwrap();
        std::fs::write(temp.path().join("src/b.rs"), "").unwrap();

        let mut config = Config::default_for_tui();
        config.working_dir = temp.path().to_path_buf();
        let mode = TuiMode::new_with_config(None, config);

        let picker = active_file_picker(&mode, "@", 1).expect("bare @ picker");
        assert_eq!(picker.prefix, "");
        assert!(picker.matches.len() >= 2, "matches: {:?}", picker.matches);
    }

    #[test]
    fn dismissed_file_picker_clears_on_new_at_token() {
        let input = "inspect @inp";
        let range = file_mention_range(input, input.len()).expect("range");
        let dismissed = Some((input.to_string(), range));

        // Completely different input — no longer dismissed.
        assert!(!file_picker_is_dismissed(
            dismissed.as_ref(),
            "review @other",
            "review @other".len()
        ));
    }

    // -- @ file picker: directory entries ----------------------------------------

    #[test]
    fn active_file_picker_includes_directory_entries() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/ui")).unwrap();
        std::fs::write(temp.path().join("src/ui/editor.rs"), "").unwrap();

        let mut config = Config::default_for_tui();
        config.working_dir = temp.path().to_path_buf();
        let mode = TuiMode::new_with_config(None, config);

        let picker = active_file_picker(&mode, "@", 1).expect("bare @ picker");
        assert!(
            picker.matches.iter().any(|m| m == "src/"),
            "should include src/ dir: {:?}",
            picker.matches
        );
        assert!(
            picker.matches.iter().any(|m| m == "src/ui/"),
            "should include src/ui/ dir: {:?}",
            picker.matches
        );
    }

    #[test]
    fn file_picker_directory_entry_matches_prefix() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/ui")).unwrap();
        std::fs::write(temp.path().join("src/ui/editor.rs"), "").unwrap();

        let mut config = Config::default_for_tui();
        config.working_dir = temp.path().to_path_buf();
        let mode = TuiMode::new_with_config(None, config);

        let picker = active_file_picker(&mode, "@src", 4).expect("prefix picker");
        assert!(
            picker.matches.iter().any(|m| m == "src/"),
            "should match src/ directory: {:?}",
            picker.matches
        );
        assert!(
            picker.matches.iter().any(|m| m == "src/ui/"),
            "should match src/ui/ directory: {:?}",
            picker.matches
        );
    }

    #[test]
    fn apply_file_picker_selection_directory_keeps_picker_open() {
        let mut editor = InputEditor::new();
        editor.insert_str("@src");
        let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
        apply_file_picker_selection(&mut editor, &range, "src/");
        // Directory selection should NOT add trailing space — keeps picker open
        assert_eq!(editor.buffer(), "@src/");
    }

    #[test]
    fn apply_file_picker_selection_file_adds_space() {
        let mut editor = InputEditor::new();
        editor.insert_str("@src/ui/ed");
        let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
        apply_file_picker_selection(&mut editor, &range, "src/ui/editor.rs");
        // File selection adds trailing space
        assert_eq!(editor.buffer(), "@src/ui/editor.rs ");
    }

    #[test]
    fn file_picker_directory_drill_shows_children() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/ui")).unwrap();
        std::fs::write(temp.path().join("src/ui/editor.rs"), "").unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "").unwrap();

        let mut config = Config::default_for_tui();
        config.working_dir = temp.path().to_path_buf();
        let mode = TuiMode::new_with_config(None, config);

        // @src/ should show immediate children only
        let picker = active_file_picker(&mode, "@src/", "@src/".len()).expect("picker");
        assert_eq!(picker.prefix, "src/");
        assert!(
            picker.matches.iter().any(|m| m == "src/ui/"),
            "should include dir: {:?}",
            picker.matches
        );
        assert!(
            picker.matches.iter().any(|m| m == "src/lib.rs"),
            "should include file: {:?}",
            picker.matches
        );
        assert!(
            !picker.matches.iter().any(|m| m == "src/ui/editor.rs"),
            "should NOT include nested file: {:?}",
            picker.matches
        );
    }

    // -- / slash picker interactivity tests ------------------------------------

    #[test]
    fn slash_prefix_token_bare_slash() {
        assert_eq!(slash_prefix_token("/"), Some("/"));
    }

    #[test]
    fn slash_prefix_token_with_command() {
        assert_eq!(slash_prefix_token("/edit something"), Some("/edit"));
    }

    #[test]
    fn slash_prefix_token_leading_whitespace() {
        assert_eq!(slash_prefix_token("  /ed"), Some("/ed"));
    }

    #[test]
    fn slash_prefix_token_no_slash() {
        assert!(slash_prefix_token("hello world").is_none());
    }

    #[test]
    fn slash_prefix_token_empty() {
        assert!(slash_prefix_token("").is_none());
    }

    #[test]
    fn active_slash_picker_bare_slash_returns_all() {
        let config = Config::default_for_tui();
        let mode = TuiMode::new_with_config(None, config);

        let picker = active_slash_picker(&mode, "/").expect("bare / picker");
        assert_eq!(picker.prefix, "/");
        assert!(
            picker.matches.len() > 5,
            "should return many commands: {:?}",
            picker.matches.len()
        );
    }

    #[test]
    fn active_slash_picker_partial_filters() {
        let config = Config::default_for_tui();
        let mode = TuiMode::new_with_config(None, config);

        let picker = active_slash_picker(&mode, "/ed").expect("partial picker");
        assert!(
            picker
                .matches
                .iter()
                .any(|m| m.command.starts_with("/edit")),
            "should contain /edit: {:?}",
            picker.matches
        );
        assert!(
            !picker
                .matches
                .iter()
                .any(|m| m.command.starts_with("/quit")),
            "should not contain /quit"
        );
    }

    #[test]
    fn active_slash_picker_no_match_returns_none() {
        let config = Config::default_for_tui();
        let mode = TuiMode::new_with_config(None, config);

        assert!(active_slash_picker(&mode, "/zzzznotexist").is_none());
    }

    #[test]
    fn active_slash_picker_non_slash_returns_none() {
        let config = Config::default_for_tui();
        let mode = TuiMode::new_with_config(None, config);

        assert!(active_slash_picker(&mode, "hello").is_none());
    }

    #[test]
    fn render_slash_picker_hint_shows_commands() {
        use vexcoder::app::SlashPickerMatch;

        let matches = vec![
            SlashPickerMatch {
                command: "/edit ".into(),
                label: "[slash] /edit <instruction> · start an edit loop".into(),
            },
            SlashPickerMatch {
                command: "/explain ".into(),
                label: "[slash] /explain [path] · explain a file".into(),
            },
        ];
        let hint = render_slash_picker_hint(&matches, 0);
        assert!(hint.contains("mode: slash"), "hint: {hint}");
        assert!(hint.contains("> [slash] /edit"), "selected marker: {hint}");
        assert!(hint.contains("  [slash] /explain"), "unselected: {hint}");
    }

    #[test]
    fn render_slash_picker_hint_empty() {
        let hint = render_slash_picker_hint(&[], 0);
        assert!(hint.contains("mode: slash"), "hint: {hint}");
        assert!(!hint.contains(">"), "no selection when empty: {hint}");
    }

    #[test]
    fn render_slash_picker_hint_clamps_selected() {
        use vexcoder::app::SlashPickerMatch;

        let matches = vec![SlashPickerMatch {
            command: "/edit ".into(),
            label: "[slash] /edit".into(),
        }];
        let hint = render_slash_picker_hint(&matches, 999);
        assert!(hint.contains("> [slash] /edit"), "should clamp: {hint}");
    }

    #[test]
    fn apply_slash_picker_selection_replaces_input() {
        let mut editor = InputEditor::new();
        editor.insert_str("/ed");
        apply_slash_picker_selection(&mut editor, "/edit ");
        assert_eq!(editor.buffer(), "/edit ");
    }

    #[test]
    fn apply_slash_picker_selection_from_bare_slash() {
        let mut editor = InputEditor::new();
        editor.insert_str("/");
        apply_slash_picker_selection(&mut editor, "/explain ");
        assert_eq!(editor.buffer(), "/explain ");
    }

    // -- PM-01 ----------------------------------------------------------------

    #[test]
    fn test_resume_flag_cli_parses_with_id() {
        let cli = Cli::parse_from(["vex", "--resume", "task-1234"]);
        assert_eq!(cli.resume, Some("task-1234".to_string()));
        assert!(cli.print_prompt.is_none());
    }

    #[test]
    fn test_resume_flag_cli_parses_without_id() {
        // --resume with no argument should default to empty string (most-recent path).
        let cli = Cli::parse_from(["vex", "--resume"]);
        assert_eq!(cli.resume, Some(String::new()));
    }

    #[test]
    fn test_resume_flag_absent_is_none() {
        let cli = Cli::parse_from(["vex"]);
        assert!(cli.resume.is_none());
    }

    #[test]
    fn test_resume_flag_can_be_combined_with_print() {
        let cli = Cli::parse_from(["vex", "--resume", "task-1", "-p", "hello"]);
        assert_eq!(cli.resume, Some("task-1".to_string()));
        assert_eq!(cli.print_prompt, Some("hello".to_string()));
    }

    #[test]
    fn test_resolve_resume_state_unknown_id_errors() {
        let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let result = resolve_resume_state("does-not-exist");
        assert!(result.is_err(), "unknown task id must produce an error");

        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_resolve_resume_state_empty_dir_returns_none() {
        let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let result = resolve_resume_state("").expect("empty-dir must not error");
        assert!(result.is_none(), "empty state dir must return None");

        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_resolve_resume_state_most_recent() {
        use filetime::{set_file_mtime, FileTime};
        use vexcoder::runtime::TaskState;
        let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

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

        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_resolve_resume_state_explicit_id() {
        use vexcoder::runtime::TaskState;
        let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let state = TaskState::new("task-explicit".to_string());
        state.save(temp.path()).unwrap();

        let loaded = resolve_resume_state("task-explicit")
            .expect("must succeed")
            .expect("must find the task");
        assert_eq!(loaded.id, "task-explicit");

        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_resolve_resume_state_explicit_id_falls_back_to_legacy_subdir() {
        use vexcoder::runtime::TaskState;

        let _env_lock = crate::tests::test_support::ENV_LOCK.blocking_lock();
        let old_cwd = std::env::current_dir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        let legacy_state_dir = nested.join(".vex/state");
        std::fs::create_dir_all(&legacy_state_dir).unwrap();

        let state = TaskState::new("task-legacy".to_string());
        state.save(&legacy_state_dir).unwrap();

        std::env::remove_var("VEX_STATE_DIR");
        std::env::set_current_dir(&nested).unwrap();

        let loaded = resolve_resume_state("task-legacy")
            .expect("must succeed")
            .expect("must find the legacy task");
        assert_eq!(loaded.id, "task-legacy");

        std::env::set_current_dir(old_cwd).unwrap();
    }

    // -- PM-03 ----------------------------------------------------------------

    #[test]
    fn test_print_flag_cli_parses() {
        let cli = Cli::parse_from(["vex", "-p", "hello world"]);
        assert_eq!(cli.print_prompt, Some("hello world".to_string()));
        assert!(cli.resume.is_none());
    }

    #[test]
    fn test_print_long_form_parses() {
        let cli = Cli::parse_from(["vex", "--print", "hello world"]);
        assert_eq!(cli.print_prompt, Some("hello world".to_string()));
    }

    #[test]
    fn test_print_flag_can_be_combined_with_resume() {
        let cli = Cli::parse_from(["vex", "-p", "hello", "--resume"]);
        assert_eq!(cli.print_prompt, Some("hello".to_string()));
        assert_eq!(cli.resume, Some(String::new()));
    }

    // -- PB-01 ----------------------------------------------------------------

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

    // -- PB-02 ----------------------------------------------------------------

    #[test]
    fn test_install_hooks_cli_parses() {
        let cli = Cli::parse_from(["vex", "install-hooks"]);
        assert!(matches!(cli.command, Some(Commands::InstallHooks)));
    }

    #[test]
    fn test_doctor_cli_parses_json_flag() {
        let cli = Cli::parse_from(["vex", "doctor", "--json"]);
        assert!(matches!(cli.command, Some(Commands::Doctor { json: true })));
    }

    #[test]
    fn test_export_cli_parses_output_and_force() {
        let cli = Cli::parse_from([
            "vex",
            "export",
            "task-123",
            "--format",
            "markdown",
            "--output",
            "/tmp/export.md",
            "--force",
        ]);
        match cli.command {
            Some(Commands::Export {
                task_id,
                format,
                output,
                force,
            }) => {
                assert_eq!(task_id, "task-123");
                assert_eq!(format, "markdown");
                assert_eq!(output, Some(PathBuf::from("/tmp/export.md")));
                assert!(force);
            }
            _ => panic!("expected export subcommand"),
        }
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

    // -- PB-03 ----------------------------------------------------------------

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
    fn test_skills_install_cli_parses() {
        let cli = Cli::parse_from([
            "vex",
            "skills",
            "install",
            "https://github.com/example/skills.git",
        ]);
        match cli.command {
            Some(Commands::Skills {
                sub: SkillsCommands::Install { source, subdir },
            }) => {
                assert_eq!(source, "https://github.com/example/skills.git");
                assert!(subdir.is_none());
            }
            _ => panic!("expected skills install"),
        }
    }

    #[test]
    fn test_skills_install_cli_parses_subdir() {
        let cli = Cli::parse_from([
            "vex",
            "skills",
            "install",
            "https://github.com/example/skills.git",
            "--subdir",
            "skills/edit-loop",
        ]);
        match cli.command {
            Some(Commands::Skills {
                sub: SkillsCommands::Install { subdir, .. },
            }) => {
                assert_eq!(subdir, Some("skills/edit-loop".to_string()));
            }
            _ => panic!("expected skills install with subdir"),
        }
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

    // -- PJ-04: vex init ------------------------------------------------------

    #[test]
    fn test_init_cli_parses_dir_flag() {
        let cli = Cli::parse_from(["vex", "init", "--dir", "/tmp/example"]);
        match cli.command {
            Some(Commands::Init { dir }) => {
                assert_eq!(dir, Some(PathBuf::from("/tmp/example")));
            }
            _ => panic!("expected init command"),
        }
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
        assert!(summary
            .iter()
            .any(|line| line == "[init] skip (exists): .vex/config.toml"));
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

    // -- PK-08: vex branch / vex pr-summary ------------------------------------

    #[tokio::test]
    async fn test_vex_branch_creates_git_branch() {
        let _env_lock = crate::tests::test_support::ENV_LOCK.lock().await;
        let repo = init_git_repo();
        let state_dir = repo.path().join("state");
        std::env::set_var("VEX_STATE_DIR", state_dir.as_os_str());

        let summary = run_branch(repo.path(), "feature/demo").await.unwrap();
        let branch = git_stdout(repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);

        assert_eq!(branch, "feature/demo");
        assert!(summary
            .iter()
            .any(|line| line == "[branch] created: feature/demo"));

        std::env::remove_var("VEX_STATE_DIR");
    }

    #[tokio::test]
    async fn test_vex_branch_records_in_task_state() {
        let _env_lock = crate::tests::test_support::ENV_LOCK.lock().await;
        let repo = init_git_repo();
        let state_dir = repo.path().join("state");
        std::env::set_var("VEX_STATE_DIR", state_dir.as_os_str());

        let state = TaskState::new("task-branch".to_string());
        state.save(&state_dir).unwrap();

        run_branch(repo.path(), "feature/task-state").await.unwrap();

        let loaded = TaskState::load(&state_dir, "task-branch").unwrap();
        assert_eq!(loaded.branch_name.as_deref(), Some("feature/task-state"));

        std::env::remove_var("VEX_STATE_DIR");
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
}
