use anyhow::{anyhow, Context, Result};
use bstr::ByteSlice as _;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::task;
use tokio::time;

#[derive(Debug, Clone, Default)]
pub(crate) struct GitSnapshot {
    pub(crate) git_status_summary: Option<String>,
    pub(crate) recent_diff: Option<String>,
}

#[derive(Default)]
pub(crate) struct GitCommandResult {
    pub(crate) output: Option<String>,
    pub(crate) non_git_repo: bool,
    pub(crate) timed_out: bool,
}

pub(crate) fn collect_git_snapshot(
    working_dir: PathBuf,
    timeout_ms: u64,
    max_diff_lines: usize,
) -> Result<GitSnapshot> {
    let git_status = block_on_context_task(run_git_command_with_timeout(
        working_dir.clone(),
        vec!["status".to_string(), "--short".to_string()],
        timeout_ms,
    ))?;

    if git_status.non_git_repo {
        return Ok(GitSnapshot::default());
    }

    let git_diff = block_on_context_task(run_git_command_with_timeout(
        working_dir,
        vec!["diff".to_string(), "HEAD".to_string()],
        timeout_ms,
    ))?;

    let mut git_status_summary = git_status.output;
    let recent_diff = git_diff
        .output
        .map(|value| limit_lines(&value, max_diff_lines));

    if git_status.timed_out {
        append_annotation(
            &mut git_status_summary,
            format!("[context: git status timed out after {}ms]", timeout_ms),
        );
    }

    if git_diff.timed_out {
        append_annotation(
            &mut git_status_summary,
            format!("[context: git diff timed out after {}ms]", timeout_ms),
        );
    }

    Ok(GitSnapshot {
        git_status_summary,
        recent_diff,
    })
}

fn append_annotation(summary: &mut Option<String>, annotation: String) {
    *summary = Some(match summary.take() {
        Some(existing) if !existing.is_empty() => format!("{existing}\n{annotation}"),
        _ => annotation,
    });
}

pub(crate) async fn run_git_command_with_timeout(
    working_dir: PathBuf,
    args: Vec<String>,
    timeout_ms: u64,
) -> Result<GitCommandResult> {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_task = Arc::clone(&cancel);
    let mut job =
        task::spawn_blocking(move || run_git_command_blocking(working_dir, args, cancel_for_task));

    match time::timeout(Duration::from_millis(timeout_ms), &mut job).await {
        Ok(join_result) => join_result.context("git command task join failed")?,
        Err(_) => {
            cancel.store(true, Ordering::SeqCst);
            match job.await {
                Ok(Ok(mut result)) => {
                    result.timed_out = true;
                    Ok(result)
                }
                Ok(Err(_)) => Ok(GitCommandResult {
                    timed_out: true,
                    ..GitCommandResult::default()
                }),
                Err(_) => Ok(GitCommandResult {
                    timed_out: true,
                    ..GitCommandResult::default()
                }),
            }
        }
    }
}

fn run_git_command_blocking(
    working_dir: PathBuf,
    args: Vec<String>,
    cancel: Arc<AtomicBool>,
) -> Result<GitCommandResult> {
    let mut child = Command::new("git")
        .current_dir(working_dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn git command")?;

    let mut stdout = child.stdout.take().context("missing git stdout pipe")?;
    let mut stderr = child.stderr.take().context("missing git stderr pipe")?;
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Ok(GitCommandResult {
                timed_out: true,
                ..GitCommandResult::default()
            });
        }

        if let Some(status) = child.try_wait().context("failed waiting for git command")? {
            let stdout_bytes = stdout_thread.join().unwrap_or_default();
            let stderr_bytes = stderr_thread.join().unwrap_or_default();

            if status.success() {
                return Ok(GitCommandResult {
                    output: Some(
                        stdout_bytes
                            .as_bstr()
                            .trim()
                            .as_bstr()
                            .to_str_lossy()
                            .into_owned(),
                    ),
                    ..GitCommandResult::default()
                });
            }

            if stderr_bytes
                .as_bstr()
                .to_ascii_lowercase()
                .contains_str("not a git repository")
            {
                return Ok(GitCommandResult {
                    non_git_repo: true,
                    ..GitCommandResult::default()
                });
            }

            return Ok(GitCommandResult::default());
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn block_on_context_task<F, T>(future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    if let Ok(handle) = Handle::try_current() {
        return match handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => {
                Ok(tokio::task::block_in_place(|| handle.block_on(future))?)
            }
            tokio::runtime::RuntimeFlavor::CurrentThread => block_on_new_runtime_thread(future),
            _ => block_on_new_runtime_thread(future),
        };
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to build runtime for ContextAssembler")?;
    runtime.block_on(future)
}

fn block_on_new_runtime_thread<F, T>(future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build runtime for ContextAssembler")?;
        runtime.block_on(future)
    })
    .join()
    .map_err(|_| anyhow!("failed to join ContextAssembler runtime thread"))?
}

pub(crate) fn resolve_git_timeout_ms(default_ms: u64) -> u64 {
    match std::env::var("VEX_CONTEXT_GIT_TIMEOUT_MS") {
        Ok(value) => match value.trim().parse::<u64>() {
            Ok(parsed) => parsed,
            Err(_) => {
                eprintln!(
                    "[context] invalid VEX_CONTEXT_GIT_TIMEOUT_MS={value:?}; using default {default_ms}ms"
                );
                default_ms
            }
        },
        Err(_) => default_ms,
    }
}

fn limit_lines(text: &str, max_lines: usize) -> String {
    if max_lines == 0 {
        return String::new();
    }
    text.lines().take(max_lines).collect::<Vec<_>>().join("\n")
}
