use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

use super::{non_empty_trimmed, path_to_repo_relative_string, ToolOperator};

impl ToolOperator {
    pub fn git_status(&self, short: bool, path: Option<&str>) -> Result<String> {
        let mut args = vec!["status".to_string()];
        if short {
            args.push("--short".to_string());
        }
        if let Some(pathspec) = path.and_then(non_empty_trimmed) {
            args.push("--".to_string());
            args.push(self.sanitize_git_pathspec(pathspec)?);
        }
        self.run_git(args)
    }

    pub fn git_diff(&self, cached: bool, path: Option<&str>) -> Result<String> {
        let mut args = vec!["diff".to_string()];
        if cached {
            args.push("--cached".to_string());
        }
        if let Some(pathspec) = path.and_then(non_empty_trimmed) {
            args.push("--".to_string());
            args.push(self.sanitize_git_pathspec(pathspec)?);
        }
        self.run_git(args)
    }

    pub fn git_log(&self, max_count: usize) -> Result<String> {
        let count = max_count.clamp(1, 100);
        self.run_git(vec![
            "log".to_string(),
            "--oneline".to_string(),
            format!("-n{count}"),
        ])
    }

    pub fn git_show(&self, revision: &str) -> Result<String> {
        let revision = non_empty_trimmed(revision)
            .context("git_show requires a non-empty 'revision' field")?;
        self.run_git(vec![
            "show".to_string(),
            "--stat".to_string(),
            "--oneline".to_string(),
            revision.to_string(),
        ])
    }

    pub fn git_add(&self, path: &str) -> Result<String> {
        let pathspec = self.sanitize_git_pathspec(path)?;
        self.run_git(vec!["add".to_string(), "--".to_string(), pathspec])?;
        Ok(format!("Staged {path}"))
    }

    pub fn git_commit(&self, message: &str) -> Result<String> {
        let message = non_empty_trimmed(message)
            .context("git_commit requires a non-empty 'message' field")?;
        self.run_git(vec![
            "commit".to_string(),
            "-m".to_string(),
            message.to_string(),
            "--no-gpg-sign".to_string(),
        ])
    }

    fn sanitize_git_pathspec(&self, path: &str) -> Result<String> {
        let path = non_empty_trimmed(path).context("Path cannot be empty")?;
        if path == "." {
            return Ok(path.to_string());
        }
        let resolved = self.resolve_path(path)?;
        let relative = resolved
            .strip_prefix(&self.working_dir)
            .context("Path escapes working directory")?;
        Ok(path_to_repo_relative_string(relative))
    }

    fn run_git(&self, args: Vec<String>) -> Result<String> {
        let output = Command::new("git")
            .current_dir(&self.working_dir)
            .args(&args)
            .output()
            .context("Failed to execute git command")?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            let details = if stderr.is_empty() { stdout } else { stderr };
            bail!("git {} failed: {}", args.join(" "), details);
        }

        if stdout.is_empty() {
            Ok("OK".to_string())
        } else {
            Ok(stdout)
        }
    }

    /// Locate the repository work-tree root using libgit2's native walk-up discovery.
    ///
    /// Returns the absolute path of the work-tree root (directory containing `.git`),
    /// or an error when the working directory is not inside a git repository.  Uses
    /// `git2::Repository::discover` which respects `GIT_CEILING_DIRECTORIES` and other
    /// standard git environment variables, providing repository detection without
    /// spawning a subprocess.
    pub fn repo_root(&self) -> Result<PathBuf> {
        let repo = git2::Repository::discover(&self.working_dir)
            .context("git2: repository not found; ensure the path is inside a git repository")?;
        repo.workdir()
            .map(|p| p.to_path_buf())
            .context("git2: bare repositories are not supported")
    }
}
