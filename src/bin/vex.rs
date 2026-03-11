use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::Clear;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};
use vexcoder::app::{build_runtime, build_runtime_with_resume, TuiMode};
use vexcoder::batch_mode::{run_batch, AutoApproveScope, BatchRunOpts, OutputFormat};
use vexcoder::config::Config;
use vexcoder::runtime::frontend::{FrontendAdapter, ScrollAction, ScrollTarget, UserInputEvent};
use vexcoder::runtime::{TaskState, TaskStatus};
use vexcoder::ui::editor::{InputAction, InputEditor};
use vexcoder::ui::layout::split_three_pane_layout;
use vexcoder::ui::render::{
    history_content_width_for_area, input_visual_rows, render_input, render_messages,
    render_overlay_modal, render_status_line, render_task_layout, OverlayModal,
};

const STARTUP_NOISE_GUARD: Duration = Duration::from_secs(15);

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
    /// Configuration migration utilities.
    Migrate {
        #[command(subcommand)]
        sub: MigrateCommands,
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

fn has_numbered_transcript_prefix(line: &str) -> bool {
    let mut saw_digit = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.peek() {
        if ch.is_ascii_digit() {
            saw_digit = true;
            chars.next();
            continue;
        }
        break;
    }
    saw_digit && chars.next() == Some(' ') && chars.next() == Some('|') && chars.next() == Some(' ')
}

fn transcript_signature_hits(text: &str) -> usize {
    let lower = text.to_ascii_lowercase();
    let signatures = [
        "mode:ready approval:",
        "view:scrolled",
        "view:following",
        "running tests/",
        "target/debug/deps/",
        "finished `dev` profile",
        "running `target/debug/vex`",
        "test result:",
        "[error] error sending request for url",
    ];
    signatures
        .iter()
        .filter(|pattern| lower.contains(*pattern))
        .count()
}

fn looks_like_terminal_transcript(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let signature_hits = transcript_signature_hits(trimmed);
    let numbered_lines = trimmed
        .lines()
        .take(64)
        .filter(|line| has_numbered_transcript_prefix(line))
        .count();

    signature_hits >= 2 || (signature_hits >= 1 && numbered_lines >= 2)
}

struct ManagedTuiFrontend {
    terminal: vexcoder::terminal::TerminalType,
    quit: bool,
    editor: InputEditor,
    started_at: Instant,
}

impl ManagedTuiFrontend {
    fn new() -> Result<Self> {
        let terminal = vexcoder::terminal::setup()?;
        Self::drain_startup_events();
        Ok(Self {
            terminal,
            quit: false,
            editor: InputEditor::new(),
            started_at: Instant::now(),
        })
    }

    fn drain_startup_events() {
        for _ in 0..1024 {
            match event::poll(Duration::from_millis(0)) {
                Ok(true) => {
                    if event::read().is_err() {
                        break;
                    }
                }
                Ok(false) | Err(_) => break,
            }
        }
    }

    fn should_ignore_startup_paste(&self, text: &str) -> bool {
        if text.contains('\u{1b}') || looks_like_terminal_transcript(text) {
            return true;
        }

        if self.started_at.elapsed() > STARTUP_NOISE_GUARD {
            return false;
        }

        text.lines().take(64).count() > 12
    }

    fn should_ignore_startup_submission(&self, text: &str) -> bool {
        self.started_at.elapsed() <= STARTUP_NOISE_GUARD && looks_like_terminal_transcript(text)
    }

    fn map_editor_action(&mut self, action: InputAction) -> Option<UserInputEvent> {
        match action {
            InputAction::None => None,
            InputAction::Interrupt => Some(UserInputEvent::Interrupt),
            InputAction::Quit => {
                self.quit = true;
                None
            }
            InputAction::Submit(value) => {
                if self.should_ignore_startup_submission(&value) {
                    None
                } else {
                    Some(UserInputEvent::Text(value))
                }
            }
        }
    }

    fn map_overlay_key(&mut self, key: KeyEvent) -> Option<UserInputEvent> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Interrupt)
            }
            KeyCode::Up => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineUp,
            }),
            KeyCode::Down => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineDown,
            }),
            KeyCode::PageUp => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Home => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::Home,
            }),
            KeyCode::End => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::End,
            }),
            KeyCode::Esc => Some(UserInputEvent::Text("esc".to_string())),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                Some(UserInputEvent::Text(ch.to_string()))
            }
            _ => None,
        }
    }

    fn map_regular_key(&mut self, key: KeyEvent) -> Option<UserInputEvent> {
        match key.code {
            KeyCode::PageUp => Some(UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::History,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::History,
                    action: ScrollAction::LineDown,
                })
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::History,
                    action: ScrollAction::Home,
                })
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::History,
                    action: ScrollAction::End,
                })
            }
            _ => {
                let action = self.editor.apply_key(key);
                self.map_editor_action(action)
            }
        }
    }
}

impl Drop for ManagedTuiFrontend {
    fn drop(&mut self) {
        let _ = vexcoder::terminal::restore();
    }
}

impl FrontendAdapter<TuiMode> for ManagedTuiFrontend {
    fn poll_user_input(&mut self, mode: &TuiMode) -> Option<UserInputEvent> {
        if mode.quit_requested() {
            self.quit = true;
            return None;
        }

        let Ok(has_event) = event::poll(Duration::from_millis(16)) else {
            self.quit = true;
            return None;
        };
        if !has_event {
            return None;
        }

        let Ok(ev) = event::read() else {
            self.quit = true;
            return None;
        };

        match ev {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    return None;
                }
                if mode.overlay_active() {
                    self.map_overlay_key(key)
                } else {
                    self.map_regular_key(key)
                }
            }
            Event::Paste(text) => {
                if mode.overlay_active() {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(UserInputEvent::Text(trimmed.to_string()))
                    }
                } else {
                    if self.should_ignore_startup_paste(&text) {
                        return None;
                    }
                    self.editor.insert_str(&text);
                    None
                }
            }
            _ => None,
        }
    }

    fn render(&mut self, mode: &TuiMode) {
        let input = self.editor.buffer().to_string();
        let cursor = self.editor.cursor();

        let _ = self.terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Clear, area);
            if let Some(task_state) = mode.task_layout_state() {
                render_task_layout(frame, &task_state);
            } else {
                let input_width = area.width.saturating_sub(2).max(1) as usize;
                let input_rows = input_visual_rows(&input, input_width).max(1) as u16;
                let panes = split_three_pane_layout(area, input_rows);
                let history_width =
                    history_content_width_for_area(mode.history_lines(), panes.history);
                mode.set_history_content_width(history_width);

                let status = mode.status_line();
                let history_scroll = mode.history_scroll_offset();

                render_status_line(frame, panes.header, &status);
                render_messages(frame, panes.history, mode.history_lines(), history_scroll);
                render_input(frame, panes.input, &input, cursor);

                if let Some((patch_preview, scroll_offset)) = mode.pending_patch_overlay() {
                    render_overlay_modal(
                        frame,
                        OverlayModal::PatchApprove {
                            patch_preview,
                            scroll_offset,
                            viewport_rows: panes.history.height.max(1) as usize,
                        },
                    );
                } else if let Some((tool_name, input_preview, auto_approve_enabled)) =
                    mode.pending_tool_overlay()
                {
                    render_overlay_modal(
                        frame,
                        OverlayModal::ToolPermission {
                            tool_name,
                            input_preview,
                            auto_approve_enabled,
                        },
                    );
                } else if mode.pending_memory_clear_overlay() {
                    render_overlay_modal(
                        frame,
                        OverlayModal::ToolPermission {
                            tool_name: "memory clear",
                            input_preview: "clear all notes? type y to confirm, n to cancel",
                            auto_approve_enabled: false,
                        },
                    );
                }
            }
        });
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}

// ── vex exec subcommand ────────────────────────────────────────────────────────

struct ExecArgs {
    task: String,
    max_turns: Option<usize>,
    auto_approve: Option<AutoApproveScope>,
    output: Option<String>,
    format: OutputFormat,
}

fn parse_exec_command(
    task: Option<String>,
    task_file: Option<String>,
    max_turns: Option<usize>,
    auto_approve: Option<String>,
    output: Option<String>,
    format: String,
) -> Result<ExecArgs> {
    let task = match (task, task_file) {
        (Some(task), None) => task,
        (None, Some(path)) => std::fs::read_to_string(path)?,
        (None, None) => {
            anyhow::bail!("vex exec requires --task <TEXT> or --task-file <PATH>")
        }
        (Some(_), Some(_)) => unreachable!("clap enforces task/task-file exclusivity"),
    };

    let auto_approve = match auto_approve.as_deref() {
        Some("once") => Some(AutoApproveScope::Once),
        Some("task") => Some(AutoApproveScope::Task),
        Some(other) => anyhow::bail!("--auto-approve must be 'once' or 'task', got: {other}"),
        None => None,
    };

    let format = match format.as_str() {
        "jsonl" => OutputFormat::Jsonl,
        "text" => OutputFormat::Text,
        other => anyhow::bail!("--format must be 'jsonl' or 'text', got: {other}"),
    };

    Ok(ExecArgs {
        task,
        max_turns,
        auto_approve,
        output,
        format,
    })
}

async fn run_exec(exec: ExecArgs, config: Config) -> Result<ExitCode> {
    let opts = BatchRunOpts {
        max_turns: exec.max_turns,
        auto_approve: exec.auto_approve,
        format: exec.format,
        resume_state: None,
    };

    let result = run_batch(exec.task, opts, &config).await?;

    let text = result.output_lines.join("\n");

    if let Some(path) = exec.output {
        std::fs::write(&path, &text)?;
    } else {
        print!("{}", text);
    }

    Ok(exit_code_for_status(result.status))
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
    let dir = TaskState::state_dir();
    if task_id.is_empty() {
        // Find the most recently modified JSON file in the state dir.
        let most_recent = std::fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("json") {
                    return None;
                }
                let stem = path.file_stem()?.to_str()?.to_string();
                let modified = e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                Some((modified, stem))
            })
            .max_by_key(|(ts, _)| *ts);

        match most_recent {
            Some((_, id)) => Ok(Some(TaskState::load(&dir, &id)?)),
            None => Ok(None),
        }
    } else {
        Ok(Some(TaskState::load(&dir, task_id)?))
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
        Some(Commands::Migrate { sub }) => match sub {
            MigrateCommands::Config { output } => {
                emit_migrate_config_output(output.as_deref())?;
                return Ok(ExitCode::SUCCESS);
            }
        },
        None => {}
    }

    let config = Config::load()?;
    config.validate()?;

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
        return run_print(prompt, config, resume_state).await;
    }

    // PM-01: --resume startup flag.
    if let Some(state) = resume_state {
        let (mut runtime, mut ctx) = build_runtime_with_resume(config, state)?;
        let mut frontend = ManagedTuiFrontend::new()?;
        runtime.run(&mut frontend, &mut ctx).await;
        return Ok(ExitCode::SUCCESS);
    }

    // Default: interactive TUI.
    let (mut runtime, mut ctx) = build_runtime(config)?;
    let mut frontend = ManagedTuiFrontend::new()?;
    runtime.run(&mut frontend, &mut ctx).await;
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
        emit_migrate_config_output, looks_like_terminal_transcript, resolve_resume_state, Cli,
        Commands, MigrateCommands, SkillsCommands,
    };
    use clap::Parser;
    use clap_complete::Shell;
    use std::path::PathBuf;

    mod test_support {
        pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    }

    #[test]
    fn test_migrate_config_maps_anthropic() {
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

    // -- PB-02 ----------------------------------------------------------------

    #[test]
    fn test_install_hooks_cli_parses() {
        let cli = Cli::parse_from(["vex", "install-hooks"]);
        assert!(matches!(cli.command, Some(Commands::InstallHooks)));
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
}
