use anyhow::Result;
use clap::{CommandFactory, Parser};
use serde::Serialize;
use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;
use vexcoder::app::{
    run_tui_session, task_graph_snapshot_path, todos_snapshot_path, write_projection_snapshot,
};
use vexcoder::batch_mode::{run_batch, BatchRunOpts, OutputFormat};
use vexcoder::config::Config;
use vexcoder::doctor::run_doctor;
use vexcoder::exec::{parse_exec_command, run_exec};
use vexcoder::export::{render_task_export, write_export_output, ExportFormat};
use vexcoder::init::run_init;
use vexcoder::pr_summary::{run_branch, run_pr_summary};
use vexcoder::runtime::{TaskState, TaskStatus};
use vexcoder::serve_local_api;
use vexcoder::startup::{emit_model_endpoint_warnings, prompt_tui_startup_config};
use vexcoder::tui_frontend::ManagedTuiFrontend;

#[path = "vex/cli.rs"]
mod cli;
#[cfg(test)]
#[path = "vex/tests.rs"]
mod tests;

use self::cli::{Cli, Commands, MigrateCommands, SkillsCommands, TaskCommands};

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

#[derive(Serialize)]
struct TaskListEntry {
    id: String,
    status: String,
    kind: &'static str,
    parent_task_id: Option<String>,
    agent_id: Option<String>,
}

fn collect_task_entries(working_dir: &Path) -> Result<Vec<TaskListEntry>> {
    let mut entries = Vec::new();
    for file in TaskState::state_files_from(working_dir) {
        let state = TaskState::load(&file.dir, &file.id)?;
        entries.push(TaskListEntry {
            id: state.id.clone(),
            status: state.status.to_string(),
            kind: "task",
            parent_task_id: state.parent_task_id.clone(),
            agent_id: state.agent_id.clone(),
        });
        for session_task in &state.session_tasks {
            entries.push(TaskListEntry {
                id: session_task.id.clone(),
                status: session_task.lifecycle_state.to_string(),
                kind: "session-task",
                parent_task_id: Some(state.id.clone()),
                agent_id: Some(session_task.agent_id.clone()),
            });
        }
    }
    Ok(entries)
}

fn run_tasks_list(working_dir: &Path, json: bool) -> Result<ExitCode> {
    let entries = collect_task_entries(working_dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if entries.is_empty() {
        println!("[tasks] no saved tasks found");
    } else {
        for entry in entries {
            match (entry.kind, entry.parent_task_id, entry.agent_id) {
                ("task", _, _) => println!("task {} status={}", entry.id, entry.status),
                (_, Some(parent), Some(agent)) => println!(
                    "session-task {} parent={} agent={} status={}",
                    entry.id, parent, agent, entry.status
                ),
                _ => println!("{} {} status={}", entry.kind, entry.id, entry.status),
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_tasks_watch(working_dir: &Path, id: &str, json: bool) -> Result<ExitCode> {
    if let Ok(state) = TaskState::load_from_search_dirs_from(working_dir, id) {
        if json {
            println!("{}", serde_json::to_string_pretty(&state)?);
        } else {
            println!("task {} status={}", state.id, state.status);
            for session_task in state.session_tasks {
                println!(
                    "  session-task {} agent={} status={}",
                    session_task.id, session_task.agent_id, session_task.lifecycle_state
                );
            }
        }
        return Ok(ExitCode::SUCCESS);
    }

    if let Some((state, session_task)) =
        TaskState::find_session_task_in_saved_states(working_dir, id)?
    {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "parent_task_id": state.id,
                    "session_task": session_task,
                }))?
            );
        } else {
            println!(
                "session-task {} parent={} agent={} status={}",
                session_task.id, state.id, session_task.agent_id, session_task.lifecycle_state
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!("[watch] task or session-task '{}' not found", id);
    Ok(ExitCode::FAILURE)
}

fn run_tasks_export_graph(working_dir: &Path) -> Result<ExitCode> {
    write_projection_snapshot(working_dir)?;
    println!("{}", task_graph_snapshot_path(working_dir).display());
    Ok(ExitCode::SUCCESS)
}

fn run_tasks_export_todos(working_dir: &Path) -> Result<ExitCode> {
    write_projection_snapshot(working_dir)?;
    println!("{}", todos_snapshot_path(working_dir).display());
    Ok(ExitCode::SUCCESS)
}

// ── main ───────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<ExitCode> {
    if std::env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }

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
        Some(Commands::Tasks { sub }) => {
            let cwd = std::env::current_dir()?;
            return match sub {
                TaskCommands::List { json } => run_tasks_list(&cwd, json),
                TaskCommands::Watch { id, json } => run_tasks_watch(&cwd, &id, json),
                TaskCommands::ExportGraph => run_tasks_export_graph(&cwd),
                TaskCommands::ExportTodos => run_tasks_export_todos(&cwd),
            };
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
            serve_local_api(config, host, port).await?;
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
