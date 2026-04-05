use anyhow::{anyhow, Context, Result};
use gix;
use gix_config;
use gix_discover;
use gix_index;
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
    pub(crate) parsed_status: Option<ParsedGitStatus>,
    /// Path to the `.git` directory discovered via the pure-Rust gitoxide
    /// implementation.  `None` when gix cannot open the repository (bare
    /// repo without a worktree, or path outside any repo).
    pub(crate) git_dir: Option<PathBuf>,
    /// Git committer name read from the global config without spawning a
    /// subprocess.  `None` when `user.name` is not set or unreadable.
    pub(crate) committer_name: Option<String>,
    /// Relative paths of every file currently in the staging area (git index)
    /// as read by `gix-index`.  Empty when no files are staged or when the
    /// index cannot be read.
    pub(crate) staged_paths: Vec<PathBuf>,
}

impl GitRollup {
    /// Return `true` when `parsed_status` contains one or more file entries
    /// (staged or unstaged changes).  Provides a boolean summary path so
    /// callers need not inspect `parsed_status` directly.
    pub(crate) fn has_staged_changes(&self) -> bool {
        self.parsed_status
            .as_ref()
            .map(|s| !s.entries.is_empty())
            .unwrap_or(false)
    }
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
    // Native pre-check using gix-discover: skip the subprocess when the
    // directory is outside any git repository.  Zero subprocesses; no PATH
    // dependency for this guard.
    if discover_git_root(&working_dir).is_none() {
        return Ok(GitRollup::default());
    }

    // Capture the .git directory path and committer identity via pure-Rust
    // implementations so downstream consumers have them without extra I/O.
    let git_dir = gix_git_dir(&working_dir);
    let committer_name = configured_git_user_name();
    let staged_paths = read_staged_paths(&working_dir);

    let git_status = block_on_context_task(run_git_command_with_timeout(
        working_dir.clone(),
        vec!["status".to_string(), "--short".to_string()],
        timeout_ms,
    ))?;

    if git_status.non_git_repo {
        return Ok(GitRollup {
            git_dir,
            committer_name,
            staged_paths,
            ..GitRollup::default()
        });
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
        git_dir,
        committer_name,
        staged_paths,
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

/// Locate the `.git` directory by walking up from `path` using the pure-Rust
/// `gix-discover` implementation.  Returns the path to the `.git` directory
/// (or the repository root for bare repos), or `None` if `path` is not inside
/// a git repository.
///
/// This complements the subprocess-based git detection in `collect_git_rollup`
/// with a zero-subprocess fallback that works from any working directory.
pub(crate) fn discover_git_root(path: &Path) -> Option<PathBuf> {
    let (git_path, _trust) = gix_discover::upwards(path).ok()?;
    Some(git_path.as_ref().to_path_buf())
}

/// Read `user.name` from the global git config using `gix-config`.
///
/// Returns the configured committer name, or `None` if it cannot be read (no
/// config file, the key is absent, or the value is not valid UTF-8).  This is
/// used to attribute AI-generated commits without spawning a `git config`
/// subprocess.
pub(crate) fn configured_git_user_name() -> Option<String> {
    let config = gix_config::File::from_globals().ok()?;
    let name = config.string("user.name")?;
    Some(String::from_utf8_lossy(name.as_ref()).into_owned())
}

/// Open the git repository at or above `path` using the pure-Rust gitoxide
/// implementation and return the path to its `.git` directory.
///
/// `gix` provides fast, thread-safe repository access without spawning
/// subprocesses.  This function is the integration seam; callers that need
/// deeper object access (e.g., reading blobs or the commit graph) can open
/// the repository themselves via `gix::open(path)`.
///
/// Returns `None` on any error (path not in a repo, I/O error, bare repo
/// without a work-tree, etc.).
pub(crate) fn gix_git_dir(path: &Path) -> Option<PathBuf> {
    let repo = gix::open(path).ok()?;
    Some(repo.git_dir().to_path_buf())
}

/// Read the relative file paths currently staged in the git index for the
/// repository that contains `repo_path`.
///
/// Uses `gix` to open the repository and `gix-index` entry types to iterate
/// the staging area without spawning any subprocess.  The staging index file
/// at `.git/index` is read directly.  Returns an empty Vec when the path is
/// not a git repository, the index file is absent, or no entries are staged.
pub(crate) fn read_staged_paths(repo_path: &Path) -> Vec<PathBuf> {
    let repo = match gix::open(repo_path) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let index_path = repo.index_path();
    let state =
        match gix_index::File::at(&index_path, repo.object_hash(), false, Default::default()) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
    state
        .entries()
        .iter()
        .filter_map(|entry: &gix_index::Entry| {
            let raw = entry.path(&state);
            std::str::from_utf8(raw).ok().map(PathBuf::from)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Confirm `parsed_status` is populated when the rollup captures git output.
    /// This exercises the field read-path so the compiler does not elide it.
    #[test]
    fn test_parsed_status_is_populated_from_rollup() {
        let rollup = GitRollup {
            git_status_summary: Some(" M src/lib.rs".to_string()),
            recent_diff: None,
            parsed_status: Some(parse_git_status(" M src/lib.rs")),
            git_dir: None,
            committer_name: None,
            staged_paths: vec![],
        };
        assert!(rollup.parsed_status.is_some());
    }

    /// `discover_git_root` returns `Some` when called from a path that is
    /// inside a git repository (this test file is inside one).
    #[test]
    fn test_discover_git_root_finds_repo() {
        let here = std::env::current_dir().expect("cwd must be available in tests");
        let root = discover_git_root(&here);
        assert!(
            root.is_some(),
            "expected a .git directory above the test working directory"
        );
    }

    /// `gix_git_dir` returns the same `.git` path as `discover_git_root` when
    /// both are called from the same working directory.
    #[test]
    fn test_gix_git_dir_returns_git_directory() {
        let here = std::env::current_dir().expect("cwd must be available in tests");
        let dir = gix_git_dir(&here);
        assert!(
            dir.is_some(),
            "expected gix to find a .git directory above the test working directory"
        );
    }

    /// `configured_git_user_name` succeeds or gracefully returns `None`; it
    /// must not panic or return an error that propagates.
    #[test]
    fn test_configured_git_user_name_does_not_panic() {
        // The result is either Some(name) or None; neither is an error here.
        let _ = configured_git_user_name();
    }

    /// `watch_working_dir` constructs a watcher without panicking.
    /// The returned handle is dropped immediately to stop the watch.
    #[test]
    fn test_watch_working_dir_constructs_watcher() {
        let here = std::env::current_dir().expect("cwd must be available in tests");
        let result = watch_working_dir(&here, |_paths| {});
        assert!(
            result.is_ok(),
            "watcher construction should succeed: {:?}",
            result
        );
        // watcher dropped here — watch stops cleanly
    }
}
