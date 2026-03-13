use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::Clear;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};
use vexcoder::app::{build_runtime, build_runtime_with_resume, TuiMode};
use vexcoder::batch_mode::{run_batch, AutoApproveScope, BatchResult, BatchRunOpts, OutputFormat};
use vexcoder::config::Config;
use vexcoder::doctor::run_doctor;
use vexcoder::export::{render_task_export, write_export_output, ExportFormat};
use vexcoder::prompts::render_pr_summary_prompt;
use vexcoder::runtime::frontend::{FrontendAdapter, ScrollAction, ScrollTarget, UserInputEvent};
use vexcoder::runtime::{ContextAssembler, TaskState, TaskStatus};
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

// ── PJ-04: vex init ────────────────────────────────────────────────────────────

/// Non-destructive workspace scaffolding.  Creates `.vex/config.toml`,
/// `AGENTS.md`, and `.vex/validate.toml` if they do not already exist.
const INIT_CONFIG_TEMPLATE: &str = concat!(
    "# vex workspace config\n",
    "# uncomment only the keys you need for this workspace\n",
    "# model_name = \"local/default\"\n",
    "# model_url = \"http://localhost:11434/v1\"\n",
    "# working_dir = \".\"\n",
    "# model_backend = \"local-runtime\"\n",
    "# model_protocol = \"chat-compat\"\n",
    "# tool_call_mode = \"tagged-fallback\"\n",
    "# max_project_instructions_tokens = 4096\n",
    "# max_memory_tokens = 2048\n",
    "# notes_path = \"~/.config/vex/memory.md\"\n",
    "# sandbox = \"passthrough\"\n",
    "# sandbox_profile = \"\"\n",
    "# sandbox_require = false\n",
    "# model_headers = '{\"X-Client-Id\":\"vexcoder\"}'\n",
    "\n",
    "# [api]\n",
    "# transport = \"http\"\n",
    "# host = \"127.0.0.1\"\n",
    "# port = 6274\n",
    "# socket = \"\"\n",
    "# key = \"${VEX_API_KEY}\"\n",
    "\n",
    "# user config only:\n",
    "# [[hooks]]\n",
    "# event = \"post_tool\"\n",
    "# tool = \"apply_patch\"\n",
    "# command = \"cargo\"\n",
    "# args = [\"fmt\"]\n",
    "# on_fail = \"warn\"\n",
    "\n",
    "# user config only:\n",
    "# [[mcp_servers]]\n",
    "# name = \"filesystem\"\n",
    "# transport = \"stdio\"\n",
    "# command = \"npx\"\n",
    "# args = [\"-y\", \"@modelcontextprotocol/server-filesystem\", \"/tmp\"]\n",
    "# url = \"http://localhost:3000/mcp\"\n",
    "\n",
    "# [mcp_servers.headers]\n",
    "# Authorization = \"${MCP_PRIVATE_SEARCH_TOKEN}\"\n",
);

const INIT_AGENTS_TEMPLATE: &str = concat!(
    "# Project Agents\n",
    "\n",
    "Fill in project-specific guidance for coding agents working in this repository.\n",
);

const INIT_VALIDATE_TEMPLATE: &str = concat!(
    "# validation commands applied by `vex validate`\n",
    "# [[commands]]\n",
    "# name = \"example\"\n",
    "# command = \"cargo test --all-targets\"\n",
);

#[cfg(test)]
const INIT_CONFIG_NORMATIVE_KEYS: &[&str] = &[
    "model_name",
    "model_url",
    "working_dir",
    "model_backend",
    "model_protocol",
    "tool_call_mode",
    "max_project_instructions_tokens",
    "max_memory_tokens",
    "notes_path",
    "sandbox",
    "sandbox_profile",
    "sandbox_require",
    "model_headers",
    "api",
    "api.transport",
    "api.host",
    "api.port",
    "api.socket",
    "api.key",
    "hooks",
    "hooks.event",
    "hooks.tool",
    "hooks.command",
    "hooks.args",
    "hooks.on_fail",
    "mcp_servers",
    "mcp_servers.name",
    "mcp_servers.transport",
    "mcp_servers.command",
    "mcp_servers.args",
    "mcp_servers.url",
    "mcp_servers.headers",
    "mcp_servers.headers.Authorization",
];

fn run_init(cwd: &Path) -> Result<Vec<String>> {
    let vex_dir = cwd.join(".vex");
    std::fs::create_dir_all(&vex_dir)?;

    let files: &[(&str, &str)] = &[
        (".vex/config.toml", INIT_CONFIG_TEMPLATE),
        ("AGENTS.md", INIT_AGENTS_TEMPLATE),
        (".vex/validate.toml", INIT_VALIDATE_TEMPLATE),
    ];
    let mut summary = Vec::new();

    for (rel_path, content) in files {
        let full = cwd.join(rel_path);
        if full.exists() {
            summary.push(format!("[init] skip (exists): {rel_path}"));
        } else {
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full, content)?;
            summary.push(format!("[init] created: {rel_path}"));
        }
    }

    summary.push("[init] done".to_string());
    Ok(summary)
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

// ── PK-08: vex branch / vex pr-summary ────────────────────────────────────────

async fn run_git_capture(cwd: PathBuf, args: Vec<String>) -> Result<String> {
    let command_display = format!("git {}", args.join(" "));
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("git")
            .current_dir(cwd)
            .args(&args)
            .output()
    })
    .await
    .context("git command task join failed")?
    .with_context(|| format!("failed to run `{command_display}`"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit status {}", output.status)
        };
        bail!("{command_display} failed: {detail}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn truncate_lines(text: &str, max_lines: usize) -> (String, bool) {
    let lines = text.lines().collect::<Vec<_>>();
    let truncated = lines.len() > max_lines;
    let mut rendered = lines
        .iter()
        .take(max_lines)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') && !rendered.is_empty() {
        rendered.push('\n');
    }
    (rendered, truncated)
}

fn record_branch_on_active_task(cwd: &Path, branch_name: &str) -> Result<Option<String>> {
    let Some(file) = TaskState::state_files_from(cwd).into_iter().next() else {
        return Ok(None);
    };

    let mut state = TaskState::load(&file.dir, &file.id)?;
    state.branch_name = Some(branch_name.to_string());
    let task_id = state.id.clone();
    state.save(&file.dir)?;
    Ok(Some(task_id))
}

async fn run_branch(cwd: &Path, name: &str) -> Result<Vec<String>> {
    run_git_capture(
        cwd.to_path_buf(),
        vec!["checkout".to_string(), "-b".to_string(), name.to_string()],
    )
    .await?;

    let mut summary = vec![format!("[branch] created: {name}")];
    match record_branch_on_active_task(cwd, name)? {
        Some(task_id) => summary.push(format!("[branch] recorded in task: {task_id}")),
        None => summary.push("[branch] no saved task state found".to_string()),
    }
    Ok(summary)
}

async fn prepare_pr_summary_prompt(cwd: &Path) -> Result<String> {
    let base_ref = run_git_capture(
        cwd.to_path_buf(),
        vec![
            "symbolic-ref".to_string(),
            "--quiet".to_string(),
            "refs/remotes/origin/HEAD".to_string(),
        ],
    )
    .await
    .context(
        "failed to detect origin/HEAD; set it first (for example: `git remote set-head origin -a`)",
    )?
    .trim()
    .to_string();
    if base_ref.is_empty() {
        bail!("origin/HEAD resolved to an empty ref");
    }

    let head_ref = run_git_capture(
        cwd.to_path_buf(),
        vec![
            "rev-parse".to_string(),
            "--abbrev-ref".to_string(),
            "HEAD".to_string(),
        ],
    )
    .await?
    .trim()
    .to_string();
    let merge_base = run_git_capture(
        cwd.to_path_buf(),
        vec![
            "merge-base".to_string(),
            "HEAD".to_string(),
            base_ref.clone(),
        ],
    )
    .await?
    .trim()
    .to_string();
    let diff_stat = run_git_capture(
        cwd.to_path_buf(),
        vec![
            "diff".to_string(),
            "--stat".to_string(),
            "--find-renames".to_string(),
            merge_base.clone(),
            "HEAD".to_string(),
        ],
    )
    .await?;
    let diff = run_git_capture(
        cwd.to_path_buf(),
        vec![
            "diff".to_string(),
            "--find-renames".to_string(),
            merge_base.clone(),
            "HEAD".to_string(),
        ],
    )
    .await?;

    if diff.trim().is_empty() {
        bail!("[pr-summary] no diff from {base_ref}");
    }

    let max_diff_lines = ContextAssembler::default().max_diff_lines;
    let (diff_excerpt, truncated) = truncate_lines(&diff, max_diff_lines);
    let mut diff_context = String::new();
    diff_context.push_str("## Diff stat\n```text\n");
    if diff_stat.trim().is_empty() {
        diff_context.push_str("[pr-summary] diff stat unavailable\n");
    } else {
        diff_context.push_str(diff_stat.trim_end());
        diff_context.push('\n');
    }
    diff_context.push_str("```\n\n## Diff\n```diff\n");
    diff_context.push_str(diff_excerpt.trim_end());
    if !diff_excerpt.ends_with('\n') {
        diff_context.push('\n');
    }
    diff_context.push_str("```\n");
    if truncated {
        diff_context.push_str(&format!(
            "\n[diff truncated — showing first {max_diff_lines} lines]\n"
        ));
    }

    let instruction = format!(
        "Generate a concise pull request title and body draft for `{head_ref}` relative to `{base_ref}`."
    );
    let context = format!("Base ref: {base_ref}\nHead ref: {head_ref}\nMerge base: {merge_base}");
    Ok(render_pr_summary_prompt(
        &instruction,
        &context,
        &diff_context,
    ))
}

async fn run_pr_summary_with_batch<F, Fut>(
    cwd: &Path,
    config: Config,
    batch_runner: F,
) -> Result<String>
where
    F: FnOnce(String, BatchRunOpts, Config) -> Fut,
    Fut: std::future::Future<Output = Result<BatchResult>>,
{
    let prompt = prepare_pr_summary_prompt(cwd).await?;
    let opts = BatchRunOpts {
        max_turns: Some(1),
        auto_approve: None,
        format: OutputFormat::Text,
        resume_state: None,
    };
    let result = batch_runner(prompt, opts, config).await?;
    Ok(result.output_lines.join("\n"))
}

async fn run_pr_summary(cwd: &Path, config: &Config) -> Result<String> {
    run_pr_summary_with_batch(cwd, config.clone(), |task, opts, config| async move {
        run_batch(task, opts, &config).await
    })
    .await
}

#[cfg(test)]
fn extract_init_template_keys(content: &str) -> std::collections::BTreeSet<String> {
    let mut section: Option<&str> = None;
    let mut keys = std::collections::BTreeSet::new();

    for raw_line in content.lines() {
        let Some(line) = raw_line.trim().strip_prefix('#') else {
            continue;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match line {
            "[api]" => {
                section = Some("api");
                keys.insert("api".to_string());
                continue;
            }
            "[[hooks]]" => {
                section = Some("hooks");
                keys.insert("hooks".to_string());
                continue;
            }
            "[[mcp_servers]]" => {
                section = Some("mcp_servers");
                keys.insert("mcp_servers".to_string());
                continue;
            }
            "[mcp_servers.headers]" => {
                section = Some("mcp_servers.headers");
                keys.insert("mcp_servers.headers".to_string());
                continue;
            }
            _ => {}
        }

        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let full_key = match section {
            Some(prefix) => format!("{prefix}.{key}"),
            None => key.to_string(),
        };
        keys.insert(full_key);
    }

    keys
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
        emit_migrate_config_output, extract_init_template_keys, looks_like_terminal_transcript,
        prepare_pr_summary_prompt, resolve_resume_state, run_branch, run_pr_summary_with_batch,
        Cli, Commands, MigrateCommands, SkillsCommands, INIT_CONFIG_NORMATIVE_KEYS,
    };
    use clap::Parser;
    use clap_complete::Shell;
    use std::path::PathBuf;
    use std::process::Command;
    use vexcoder::batch_mode::{BatchResult, OutputFormat};
    use vexcoder::config::Config;
    use vexcoder::runtime::{TaskState, TaskStatus};

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
        super::run_init(temp.path()).unwrap();
        assert!(temp.path().join(".vex").is_dir());
    }

    #[test]
    fn test_vex_init_writes_config_toml_skeleton() {
        let temp = tempfile::tempdir().unwrap();
        super::run_init(temp.path()).unwrap();
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
        super::run_init(temp.path()).unwrap();
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

        let summary = super::run_init(temp.path()).unwrap();
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
        let keys = extract_init_template_keys(super::INIT_CONFIG_TEMPLATE);
        let expected = INIT_CONFIG_NORMATIVE_KEYS
            .iter()
            .map(|value| value.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys, expected);
    }

    #[test]
    fn test_vex_init_does_not_start_agent_loop() {
        let temp = tempfile::tempdir().unwrap();
        let summary = super::run_init(temp.path()).unwrap();
        assert!(!temp.path().join(".vex/state").exists());
        assert_eq!(summary.last().map(String::as_str), Some("[init] done"));
    }

    #[test]
    fn test_vex_init_writes_validate_commands_stub() {
        let temp = tempfile::tempdir().unwrap();
        super::run_init(temp.path()).unwrap();
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
        let repo = init_git_repo();

        let summary = run_branch(repo.path(), "feature/demo").await.unwrap();
        let branch = git_stdout(repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);

        assert_eq!(branch, "feature/demo");
        assert!(summary
            .iter()
            .any(|line| line == "[branch] created: feature/demo"));
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
