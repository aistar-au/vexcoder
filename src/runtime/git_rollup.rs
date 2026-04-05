use anyhow::{anyhow, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::task;
use tokio::time;

use super::git_parse::{parse_git_status, ParsedGitStatus};

#[derive(Debug, Clone, Default)]
pub(crate) struct GitRollup {
    pub(crate) git_status_summary: Option<String>,
    pub(crate) recent_diff: Option<String>,
    /// Structured parse of the raw `git status --porcelain` output.
    /// Available for downstream consumers (context assembler, tool operators).
    #[allow(dead_code)]
    pub(crate) parsed_status: Option<ParsedGitStatus>,
}

#[derive(Default)]
pub(crate) struct GitCommandResult {
    pub(crate) output: Option<String>,
    pub(crate) non_git_repo: bool,
    pub(crate) timed_out: bool,
}

/// Resolve the path to the `git` executable, returning an error with a
/// concrete message when `git` is not found on `$PATH`.
///
/// Uses `which::which("git")` so the caller gets a diagnosis ("git not found
/// on PATH") rather than a cryptic spawn failure.
pub(crate) fn resolve_git_path() -> Result<PathBuf> {
    which::which("git").map_err(|err| {
        anyhow!(
            "git executable not found on PATH: {}. \
             Install git or add it to PATH before running vexcoder.",
            err
        )
    })
}

pub(crate) fn collect_git_rollup(
    working_dir: PathBuf,
    timeout_ms: u64,
    max_diff_lines: usize,
) -> Result<GitRollup> {
    let git_status = block_on_context_task(run_git_command_with_timeout(
        working_dir.clone(),
        vec!["status".to_string(), "--short".to_string()],
        timeout_ms,
    ))?;

    if git_status.non_git_repo {
        return Ok(GitRollup::default());
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

    let parsed_status = git_status_summary.as_deref().map(parse_git_status);

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

    Ok(GitRollup {
        git_status_summary,
        recent_diff,
        parsed_status,
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
    let git_path = resolve_git_path()?;
    let mut child = Command::new(git_path)
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
            let stdout_buf = String::from_utf8_lossy(&stdout_bytes);
            let stderr_buf = String::from_utf8_lossy(&stderr_bytes);

            if status.success() {
                return Ok(GitCommandResult {
                    output: Some(stdout_buf.trim().to_string()),
                    ..GitCommandResult::default()
                });
            }

            if stderr_buf
                .to_ascii_lowercase()
                .contains("not a git repository")
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

/// Start a filesystem watcher on `working_dir` and invoke `on_change` each
/// time any file in the directory tree is created, modified, or removed.
///
/// The watcher runs until the returned `notify::RecommendedWatcher` is
/// dropped.  Callers retain ownership of the watcher so the watch lifetime
/// is tied to the owning context (e.g., a long-lived task or session).
///
/// This is the integration seam for `notify`-based watch mode in the git
/// rollup layer.  It is intentionally kept narrow: the callback receives only
/// a `Vec<PathBuf>` of changed paths so the caller decides how to respond
/// (e.g., schedule a fresh `collect_git_rollup` call).
#[allow(dead_code)]
pub(crate) fn watch_working_dir<F>(
    working_dir: &Path,
    on_change: F,
) -> Result<notify::RecommendedWatcher>
where
    F: Fn(Vec<PathBuf>) + Send + 'static,
{
    use notify::{Config, Event, RecursiveMode, Watcher};

    let mut watcher = notify::RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let paths = event.paths;
                if !paths.is_empty() {
                    on_change(paths);
                }
            }
        },
        Config::default(),
    )
    .context("failed to initialize filesystem watcher")?;

    watcher
        .watch(working_dir, RecursiveMode::Recursive)
        .with_context(|| format!("failed to watch directory: {}", working_dir.display()))?;

    Ok(watcher)
}

fn limit_lines(text: &str, max_lines: usize) -> String {
    if max_lines == 0 {
        return String::new();
    }
    text.lines().take(max_lines).collect::<Vec<_>>().join("\n")
}
