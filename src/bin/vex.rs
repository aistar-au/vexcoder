use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser};
use dialoguer::Password;
use serde::Serialize;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tabled::{Table, Tabled};
use tracing::Instrument;
use vexcoder::app::{
    run_tui_session, task_graph_rollup_path, todos_rollup_path, write_projection_rollup,
};
use vexcoder::batch_mode::{AutoApproveScope, BatchRunOpts, OutputFormat, run_batch};
use vexcoder::config::Config;
use vexcoder::disk_policy::{self, DiskPolicyMode};
use vexcoder::doctor::run_doctor;
use vexcoder::exec::{parse_exec_command, run_exec};
use vexcoder::export::{ExportFormat, render_task_export, write_export_output};
use vexcoder::init::run_init;
use vexcoder::pr_summary::{run_branch, run_pr_summary};
use vexcoder::privacy::render_privacy_markdown;
use vexcoder::runtime::{TaskState, TaskStatus, ToolPolicy};
use vexcoder::serve_local_api;
use vexcoder::startup::emit_model_endpoint_warnings;
use vexcoder::tui_frontend::ManagedTuiFrontend;

#[path = "vex/cli.rs"]
mod cli;
#[cfg(test)]
#[path = "vex/tests.rs"]
mod tests;

use self::cli::{
    Cli, Commands, CredentialsCommands, MigrateCommands, SkillsCommands, TaskCommands,
};

fn emit_migrate_config_output(output_path: Option<&Path>) -> Result<()> {
    let fragment = vexcoder::config::migrate_config_from_env(&[]);
    if let Some(path) = output_path {
        std::fs::write(path, fragment)?;
    } else {
        print!("{}", fragment);
    }
    Ok(())
}

// ── PM-01: resolve task state for --recall-coordinates ───────────────────────

/// Load a `TaskState` for `--recall-coordinates`. An empty `task_id` means
/// "pick the most recent saved task"; a non-empty `task_id` loads that
/// specific task.
/// Returns `None` only when no tasks exist yet (empty-id path).
fn resolve_resume_state(task_id: &str) -> Result<Option<TaskState>> {
    if task_id.is_empty() {
        let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        match TaskState::state_files_from_with_limit(&working_dir, Some(1))
            .into_iter()
            .next()
        {
            Some(file) => Ok(Some(TaskState::load(&file.dir, &file.id)?)),
            None => Ok(None),
        }
    } else {
        Ok(Some(TaskState::load_from_search_dirs(task_id)?))
    }
}

// ── PM-03: --project-map-only one-shot mode ──────────────────────────────────

/// Collect stdin when it is not a TTY (pipe / redirect) and prepend it to the
/// prompt so that `vex -p "summarise" < file.txt` works naturally.
fn read_stdin_if_piped() -> Option<String> {
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

fn trim_single_trailing_line_ending(secret: &mut String) {
    if secret.ends_with('\n') {
        secret.pop();
        if secret.ends_with('\r') {
            secret.pop();
        }
    } else if secret.ends_with('\r') {
        secret.pop();
    }
}

fn read_secret_from_reader(mut reader: impl Read) -> Result<String> {
    let mut secret = String::new();
    reader
        .read_to_string(&mut secret)
        .context("failed to read credential secret")?;
    trim_single_trailing_line_ending(&mut secret);
    Ok(secret)
}

fn read_secret_from_stdin() -> Result<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        bail!("--stdin requires piped or redirected stdin");
    }
    read_secret_from_reader(stdin.lock())
}

fn read_secret_from_env_var(name: &str) -> Result<String> {
    std::env::var(name)
        .with_context(|| format!("environment variable '{name}' is not set or is not valid UTF-8"))
}

fn can_prompt_for_secret() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn read_secret_from_tty_prompt(account: &str) -> Result<String> {
    Password::new()
        .with_prompt(format!("Credential secret for {account}"))
        .with_confirmation(
            "Confirm credential secret",
            "credential secrets did not match",
        )
        .interact()
        .context("failed to read credential secret interactively")
}

fn resolve_credentials_secret(
    account: &str,
    stdin: bool,
    from_env: Option<String>,
    can_prompt: bool,
    prompt_secret: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    if stdin {
        read_secret_from_stdin()
    } else if let Some(var_name) = from_env {
        read_secret_from_env_var(&var_name)
    } else if can_prompt {
        prompt_secret(account)
    } else {
        bail!(
            "refusing to read a credential secret from argv; use --stdin, --from-env VAR, or rerun on an interactive TTY"
        )
    }
}

fn credentials_action_from_cli(
    sub: CredentialsCommands,
) -> Result<vexcoder::credentials::CredentialsAction> {
    use vexcoder::credentials::CredentialsAction;

    match sub {
        CredentialsCommands::Set {
            account,
            stdin,
            from_env,
        } => {
            let secret = resolve_credentials_secret(
                &account,
                stdin,
                from_env,
                can_prompt_for_secret(),
                read_secret_from_tty_prompt,
            )?;
            Ok(CredentialsAction::Set { account, secret })
        }
        CredentialsCommands::Get { account } => Ok(CredentialsAction::Get { account }),
        CredentialsCommands::Delete { account } => Ok(CredentialsAction::Delete { account }),
        CredentialsCommands::List => Ok(CredentialsAction::List),
    }
}

async fn run_print(
    prompt: String,
    config: Config,
    resume_state: Option<TaskState>,
    format: OutputFormat,
) -> Result<ExitCode> {
    let stdin_content = read_stdin_if_piped();
    let stdin_attached = stdin_content.is_some();
    let full_prompt = match stdin_content {
        Some(stdin_content) => format!("{stdin_content}\n{prompt}"),
        None => prompt,
    };
    let request_id = vexcoder::observability::new_request_id();
    let print_span = tracing::info_span!(
        "cli_print_request",
        request_id = %request_id,
        trace_id = tracing::field::Empty,
        otel_span_id = tracing::field::Empty,
        prompt_chars = full_prompt.chars().count(),
        stdin_attached,
        output_format = "text",
    );
    vexcoder::observability::attach_standard_baggage(
        &print_span,
        "cli",
        "print",
        Some(&request_id),
        resume_state.as_ref().map(|state| state.id.as_str()),
    );

    let opts = BatchRunOpts {
        max_turns: Some(1),
        auto_approve: default_auto_approve_scope(&config, None),
        format,
        resume_state,
    };

    let result = run_batch(full_prompt, opts, &config)
        .instrument(print_span)
        .await?;
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

#[derive(Tabled)]
struct TaskListTableRow {
    kind: &'static str,
    id: String,
    status: String,
    origin: String,
    agent: String,
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

fn render_task_entries(entries: &[TaskListEntry], interactive: bool) -> String {
    if entries.is_empty() {
        return "[tasks] no saved tasks found".to_string();
    }

    if interactive {
        return format_task_entries_table(entries);
    }

    entries
        .iter()
        .map(|entry| {
            match (
                entry.kind,
                entry.parent_task_id.as_deref(),
                entry.agent_id.as_deref(),
            ) {
                ("task", _, _) => format!("task {} status={}", entry.id, entry.status),
                (_, Some(origin), Some(agent)) => format!(
                    "session-task {} origin={} agent={} status={}",
                    entry.id, origin, agent, entry.status
                ),
                _ => format!("{} {} status={}", entry.kind, entry.id, entry.status),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_task_entries_table(entries: &[TaskListEntry]) -> String {
    let rows = entries
        .iter()
        .map(|entry| TaskListTableRow {
            kind: entry.kind,
            id: entry.id.clone(),
            status: entry.status.clone(),
            origin: entry
                .parent_task_id
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            agent: entry.agent_id.clone().unwrap_or_else(|| "-".to_string()),
        })
        .collect::<Vec<_>>();

    Table::new(rows).to_string()
}

fn run_tasks_list(working_dir: &Path, json: bool) -> Result<ExitCode> {
    let entries = collect_task_entries(working_dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        println!(
            "{}",
            render_task_entries(&entries, std::io::stdout().is_terminal())
        );
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
                "session-task {} origin={} agent={} status={}",
                session_task.id, state.id, session_task.agent_id, session_task.lifecycle_state
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!("[watch] task or session-task '{}' not found", id);
    Ok(ExitCode::FAILURE)
}

fn run_tasks_export_graph(working_dir: &Path) -> Result<ExitCode> {
    write_projection_rollup(working_dir)?;
    println!("{}", task_graph_rollup_path(working_dir).display());
    Ok(ExitCode::SUCCESS)
}

fn run_tasks_export_todos(working_dir: &Path) -> Result<ExitCode> {
    write_projection_rollup(working_dir)?;
    println!("{}", todos_rollup_path(working_dir).display());
    Ok(ExitCode::SUCCESS)
}

// ── main ───────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<ExitCode> {
    // Keep human-panic for unexpected release crashes while color-eyre formats
    // recoverable Result-based failures.
    human_panic::setup_panic!();
    let (_, eyre_hook) = color_eyre::config::HookBuilder::default().into_hooks();
    let _ = eyre_hook.install();

    let cli = Cli::parse();
    let _telemetry_guard = vexcoder::observability::init_cli_observability(
        vexcoder::observability::CliTelemetryOptions {
            display_internal_telemetry: cli.display_internal_telemetry,
            telemetry_json: cli.telemetry_json,
        },
    )?;
    let tool_policy =
        tool_policy_from_cli(cli.view_intended_trajectory, cli.restrict_payload_tools);

    let overrides = CliOverrides {
        tool_policy,
        use_alternate_navigator: cli.use_alternate_navigator.clone(),
        expand_sector_view: cli.expand_sector_view,
        force_unstable_alignment: cli.force_unstable_alignment,
        bypass_integrity_locks: cli.bypass_integrity_locks,
        recall_coordinates: cli.recall_coordinates.clone(),
        project_map_only: cli.project_map_only.clone(),
        set_map_encoding: cli.set_map_encoding.clone(),
    };

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
            let mut config = Config::load()?;
            apply_cli_overrides(&overrides, &mut config);
            apply_process_policy_overrides(&config);
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
        Some(Commands::Privacy) => {
            print!("{}", render_privacy_markdown());
            return Ok(ExitCode::SUCCESS);
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
            let mut config = Config::load()?;
            apply_cli_overrides(&overrides, &mut config);
            apply_process_policy_overrides(&config);
            config.validate()?;
            serve_local_api(config, host, port).await?;
            return Ok(ExitCode::SUCCESS);
        }
        Some(Commands::Credentials { sub }) => {
            use vexcoder::credentials::run_credentials;
            let action = credentials_action_from_cli(sub)?;
            run_credentials(action)?;
            return Ok(ExitCode::SUCCESS);
        }
        None => {}
    }

    let mut config = Config::load()?;
    apply_cli_overrides(&overrides, &mut config);
    apply_process_policy_overrides(&config);

    let resume_state = match overrides.recall_coordinates.as_deref() {
        Some(task_id) => match resolve_resume_state(task_id)? {
            Some(state) => Some(state),
            None => {
                eprintln!("[resume] no saved tasks found");
                return Ok(ExitCode::FAILURE);
            }
        },
        None => None,
    };

    // -p/--project-map-only one-shot mode.
    if let Some(prompt) = overrides.project_map_only {
        config.validate()?;
        emit_model_endpoint_warnings(&config);
        let fmt = map_encoding_to_output_format(&overrides.set_map_encoding);
        return run_print(prompt, config, resume_state, fmt).await;
    }

    config.validate()?;
    emit_model_endpoint_warnings(&config);

    // PM-01: --recall-coordinates startup option.
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

struct CliOverrides {
    tool_policy: ToolPolicy,
    use_alternate_navigator: Option<String>,
    expand_sector_view: bool,
    force_unstable_alignment: bool,
    bypass_integrity_locks: bool,
    recall_coordinates: Option<String>,
    project_map_only: Option<String>,
    set_map_encoding: String,
}

fn tool_policy_from_cli(
    view_intended_trajectory: bool,
    restrict_payload_tools: bool,
) -> ToolPolicy {
    if view_intended_trajectory || restrict_payload_tools {
        ToolPolicy::Plan
    } else {
        ToolPolicy::Full
    }
}

fn apply_cli_overrides(o: &CliOverrides, config: &mut Config) {
    config.tool_policy = o.tool_policy;
    if let Some(ref name) = o.use_alternate_navigator {
        config.model_name = name.clone();
    }
    if o.expand_sector_view {
        config.expand_context = true;
    }
    if o.force_unstable_alignment {
        config.force = true;
    }
    if o.bypass_integrity_locks {
        config.bypass_policy = true;
    }
}

fn apply_process_policy_overrides(config: &Config) {
    let mode = config.bypass_policy.then_some(DiskPolicyMode::Off);
    disk_policy::set_process_policy_override(mode);
}

fn default_auto_approve_scope(
    config: &Config,
    explicit: Option<AutoApproveScope>,
) -> Option<AutoApproveScope> {
    explicit.or(config.force.then_some(AutoApproveScope::Task))
}

fn map_encoding_to_output_format(encoding: &str) -> OutputFormat {
    match encoding {
        "json" | "jsonl" => OutputFormat::Jsonl,
        _ => OutputFormat::Text,
    }
}

fn exit_code_for_status(status: TaskStatus) -> ExitCode {
    match status {
        TaskStatus::Completed => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
