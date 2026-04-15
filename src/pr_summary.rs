use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::batch_mode::{BatchResult, BatchRunOpts, OutputFormat, run_batch};
use crate::config::Config;
use crate::prompts::render_pr_summary_prompt;
use crate::runtime::tokio::task::spawn_blocking;
use crate::runtime::{ContextAssembler, TaskState};

pub async fn run_git_capture(cwd: PathBuf, args: Vec<String>) -> Result<String> {
    let command_display = format!("git {}", args.join(" "));
    let output = spawn_blocking(move || {
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

pub fn truncate_lines(text: &str, max_lines: usize) -> (String, bool) {
    let lines = text.lines().collect::<Vec<_>>();
    let was_limited = lines.len() > max_lines;
    let mut rendered = lines
        .iter()
        .take(max_lines)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') && !rendered.is_empty() {
        rendered.push('\n');
    }
    (rendered, was_limited)
}

pub fn record_branch_on_active_task(cwd: &Path, branch_name: &str) -> Result<Option<String>> {
    let Some(file) = TaskState::state_files_from_with_limit(cwd, Some(1))
        .into_iter()
        .next()
    else {
        return Ok(None);
    };

    let mut state = TaskState::load(&file.dir, &file.id)?;
    state.branch_name = Some(branch_name.to_string());
    let task_id = state.id.clone();
    state.save(&file.dir)?;
    Ok(Some(task_id))
}

pub async fn run_branch(cwd: &Path, name: &str) -> Result<Vec<String>> {
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

pub async fn prepare_pr_summary_prompt(cwd: &Path) -> Result<String> {
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
    let (diff_excerpt, diff_limited) = truncate_lines(&diff, max_diff_lines);
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
    if diff_limited {
        diff_context.push_str(&format!(
            "\n[diff limited to first {max_diff_lines} lines]\n"
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

pub async fn run_pr_summary_with_batch<F, Fut>(
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

pub async fn run_pr_summary(cwd: &Path, config: &Config) -> Result<String> {
    run_pr_summary_with_batch(cwd, config.clone(), |task, opts, config| async move {
        run_batch(task, opts, &config).await
    })
    .await
}
