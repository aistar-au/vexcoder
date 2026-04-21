use anyhow::Result;
use std::process::ExitCode;

use crate::batch_mode::{AutoApproveScope, BatchRunOpts, OutputFormat, run_batch};
use crate::config::Config;
use crate::runtime::TaskStatus;

pub struct ExecArgs {
    task: String,
    max_turns: Option<usize>,
    auto_approve: Option<AutoApproveScope>,
    output: Option<String>,
    format: OutputFormat,
}

pub fn parse_exec_command(
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

pub async fn run_exec(exec: ExecArgs, config: Config) -> Result<ExitCode> {
    let opts = BatchRunOpts {
        max_turns: exec.max_turns,
        auto_approve: exec
            .auto_approve
            .or(config.force.then_some(AutoApproveScope::Task)),
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

    Ok(crate::exec::exit_code_for_status(result.status))
}

pub fn exit_code_for_status(status: TaskStatus) -> ExitCode {
    match status {
        TaskStatus::Completed => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
