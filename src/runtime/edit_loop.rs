use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio_util::sync::CancellationToken;

use super::context::RuntimeContext;
use super::task_state::TaskId;
use super::validation::ValidationResult;

const DEFAULT_MAX_TURNS: u8 = 6;
const HARD_MAX_TURNS: u8 = 12;

#[derive(Debug, Clone)]
pub struct EditLoop {
    pub task_id: TaskId,
    pub max_turns: u8,
    pub stop_on_clean_validate: bool,
    last_validation_result: Option<ValidationResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditLoopOutcome {
    Success {
        patch_applied: bool,
        validate_passed: bool,
    },
    MaxTurnsReached {
        last_error: Option<String>,
    },
    ApprovalDenied,
    Cancelled,
}

impl EditLoop {
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            max_turns: DEFAULT_MAX_TURNS,
            stop_on_clean_validate: true,
            last_validation_result: None,
        }
    }

    pub fn with_max_turns(mut self, max_turns: u8) -> Self {
        self.max_turns = clamp_turns(max_turns);
        self
    }

    pub fn last_validation_result(&self) -> Option<&ValidationResult> {
        self.last_validation_result.as_ref()
    }

    pub fn set_last_validation_result(&mut self, result: ValidationResult) {
        self.last_validation_result = Some(result);
    }

    pub async fn run(
        &mut self,
        _instruction: String,
        _ctx: &mut RuntimeContext,
        cancel: &CancellationToken,
    ) -> Result<EditLoopOutcome> {
        // EL-03 skeleton only: loop body wiring lands in EL-04.
        for _ in 0..self.max_turns {
            if cancel.is_cancelled() {
                return Ok(EditLoopOutcome::Cancelled);
            }
        }

        Ok(EditLoopOutcome::MaxTurnsReached {
            last_error: self.last_validation_error(),
        })
    }

    pub fn check_workspace_dirty(root: &Path, paths: &[PathBuf]) -> Result<bool> {
        let mut command = Command::new("git");
        command.current_dir(root).arg("status").arg("--porcelain");
        if !paths.is_empty() {
            command.arg("--");
            for path in paths {
                command.arg(path);
            }
        }

        let output = command
            .output()
            .context("failed to execute git status for workspace-dirty check")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
            if stderr.contains("not a git repository") {
                return Ok(false);
            }
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }

    fn last_validation_error(&self) -> Option<String> {
        self.last_validation_result.as_ref().and_then(|result| {
            if result.passed {
                return None;
            }

            result
                .outputs
                .iter()
                .find(|output| output.exit_code != 0)
                .map(|output| format!("{} exited with {}", output.label, output.exit_code))
                .or_else(|| Some("validation failed".to_string()))
        })
    }
}

fn clamp_turns(turns: u8) -> u8 {
    turns.clamp(1, HARD_MAX_TURNS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::runtime::UiUpdate;
    use crate::state::ConversationManager;
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn make_runtime_context() -> RuntimeContext {
        let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        RuntimeContext::new(conversation, tx, CancellationToken::new())
    }

    #[tokio::test]
    async fn test_edit_loop_terminates_at_max_turns() {
        let mut edit_loop = EditLoop::new("task-001".to_string()).with_max_turns(1);
        let mut ctx = make_runtime_context();
        let cancel = CancellationToken::new();

        let outcome = edit_loop
            .run("edit src/runtime/edit_loop.rs".to_string(), &mut ctx, &cancel)
            .await
            .expect("run should succeed");

        assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
    }

    #[tokio::test]
    async fn test_edit_loop_returns_cancelled_when_token_is_pre_cancelled() {
        let mut edit_loop = EditLoop::new("task-002".to_string());
        let mut ctx = make_runtime_context();
        let cancel = CancellationToken::new();
        cancel.cancel();

        let outcome = edit_loop
            .run("edit src/runtime/edit_loop.rs".to_string(), &mut ctx, &cancel)
            .await
            .expect("run should succeed");

        assert!(matches!(outcome, EditLoopOutcome::Cancelled));
    }

    #[test]
    fn test_edit_loop_detects_dirty_workspace_for_target_paths() {
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::write(workspace.path().join("tracked.txt"), "v1\n").expect("seed file");
        run_git(workspace.path(), &["init"]);
        run_git(workspace.path(), &["add", "tracked.txt"]);
        run_git(
            workspace.path(),
            &[
                "-c",
                "user.name=codex",
                "-c",
                "user.email=codex@example.com",
                "commit",
                "-m",
                "init",
            ],
        );

        let clean = EditLoop::check_workspace_dirty(
            workspace.path(),
            &[PathBuf::from("tracked.txt")],
        )
        .expect("clean check");
        assert!(!clean, "workspace should be clean after commit");

        fs::write(workspace.path().join("tracked.txt"), "v2\n").expect("mutate file");
        let dirty = EditLoop::check_workspace_dirty(
            workspace.path(),
            &[PathBuf::from("tracked.txt")],
        )
        .expect("dirty check");
        assert!(dirty, "workspace should be dirty after tracked file change");
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .expect("git should start");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
