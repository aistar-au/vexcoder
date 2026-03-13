use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio_util::sync::CancellationToken;

use crate::runtime::ModelBackendKind;
use crate::types::ModelProfile;

use super::command::DefaultCommandRunner;
use super::context::RuntimeContext;
use super::task_state::TaskId;
use super::validation::{ValidationResult, ValidationSuite};

const DEFAULT_MAX_TURNS: u8 = 6;
const HARD_MAX_TURNS: u8 = 12;

#[derive(Debug, Clone)]
pub struct EditLoop {
    pub task_id: TaskId,
    pub max_turns: u8,
    pub stop_on_clean_validate: bool,
    pub profile: ModelProfile,
    working_dir: PathBuf,
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
            profile: ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
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

    pub fn with_profile(mut self, profile: ModelProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_working_dir(mut self, working_dir: PathBuf) -> Self {
        self.working_dir = working_dir;
        self
    }

    pub fn profile_name(&self) -> &str {
        self.profile.name.as_str()
    }

    pub fn set_last_validation_result(&mut self, result: ValidationResult) {
        self.last_validation_result = Some(result);
    }

    pub async fn run(
        &mut self,
        instruction: String,
        ctx: &mut RuntimeContext,
        cancel: &CancellationToken,
    ) -> Result<EditLoopOutcome> {
        // EL-03 step 1: workspace-dirty warning.
        if Self::check_workspace_dirty(&self.working_dir, &[])? {
            ctx.emit_transcript_line(
                    "[edit loop warning: workspace has uncommitted changes; proceeding without mutating git state]"
                        .to_string(),
                );
        }

        // EL-04: assemble → model → apply → validate → retry cycle.
        let root = self.working_dir.clone();
        let validation_suite = ValidationSuite::load_or_infer(&root);
        let runner = DefaultCommandRunner::new();

        let mut retry_context = String::new();
        let mut patch_applied: bool;

        for turn in 0..self.max_turns {
            if cancel.is_cancelled() {
                return Ok(EditLoopOutcome::Cancelled);
            }

            // Yield between turns so the TUI, tests, and other tasks can
            // observe intermediate state (e.g. system-prompt injection).
            tokio::task::yield_now().await;

            // Assemble: instruction + validation retry context (if any).
            let message = if retry_context.is_empty() {
                instruction.clone()
            } else {
                format!("{instruction}\n\n{retry_context}")
            };

            ctx.emit_transcript_line(format!("[edit loop turn {}/{}]", turn + 1, self.max_turns));

            // Model: drive a full tool-loop turn (read/edit/write/command).
            match ctx.drive_edit_turn(message).await {
                Ok(_response) => {
                    patch_applied = true;
                }
                Err(err) => {
                    ctx.emit_transcript_line(format!("[edit loop turn error: {err}]"));
                    retry_context = format!("[previous turn failed: {err}]");
                    continue;
                }
            }

            if cancel.is_cancelled() {
                return Ok(EditLoopOutcome::Cancelled);
            }

            // Validate: run the project validation suite concurrently.
            ctx.emit_transcript_line("[edit loop: running validation]".to_string());
            let validation_result = validation_suite.run_in_dir(&runner, Some(&root)).await?;
            self.set_last_validation_result(validation_result.clone());

            if validation_result.passed {
                ctx.emit_transcript_line("[edit loop: validation passed]".to_string());
                if self.stop_on_clean_validate {
                    return Ok(EditLoopOutcome::Success {
                        patch_applied,
                        validate_passed: true,
                    });
                }
            } else {
                // Retry: format failure output as context for the next turn.
                retry_context = validation_suite.format_for_retry(&validation_result);
                ctx.emit_transcript_line("[edit loop: validation failed, retrying]".to_string());
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

    fn make_runtime_context() -> (RuntimeContext, mpsc::UnboundedReceiver<UiUpdate>) {
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let (tx, rx) = mpsc::unbounded_channel::<UiUpdate>();
        (
            RuntimeContext::new(conversation, tx, CancellationToken::new()),
            rx,
        )
    }

    #[tokio::test]
    async fn test_edit_loop_terminates_at_max_turns() {
        let mut edit_loop = EditLoop::new("task-001".to_string()).with_max_turns(1);
        let (mut ctx, _rx) = make_runtime_context();
        let cancel = CancellationToken::new();

        let outcome = edit_loop
            .run(
                "edit src/runtime/edit_loop.rs".to_string(),
                &mut ctx,
                &cancel,
            )
            .await
            .expect("run should succeed");

        assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
    }

    #[tokio::test]
    async fn test_edit_loop_returns_cancelled_when_token_is_pre_cancelled() {
        let mut edit_loop = EditLoop::new("task-002".to_string());
        let (mut ctx, _rx) = make_runtime_context();
        let cancel = CancellationToken::new();
        cancel.cancel();

        let outcome = edit_loop
            .run(
                "edit src/runtime/edit_loop.rs".to_string(),
                &mut ctx,
                &cancel,
            )
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
                "user.name=vex-test",
                "-c",
                "user.email=vex-test@example.com",
                "commit",
                "-m",
                "init",
            ],
        );

        let clean =
            EditLoop::check_workspace_dirty(workspace.path(), &[PathBuf::from("tracked.txt")])
                .expect("clean check");
        assert!(!clean, "workspace should be clean after commit");

        fs::write(workspace.path().join("tracked.txt"), "v2\n").expect("mutate file");
        let dirty =
            EditLoop::check_workspace_dirty(workspace.path(), &[PathBuf::from("tracked.txt")])
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

    #[test]
    fn test_edit_loop_emits_dirty_workspace_warning() {
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::write(workspace.path().join("target.rs"), "fn main() {}\n").expect("seed file");
        run_git(workspace.path(), &["init"]);
        run_git(workspace.path(), &["add", "target.rs"]);
        run_git(
            workspace.path(),
            &[
                "-c",
                "user.name=vex-test",
                "-c",
                "user.email=vex-test@example.com",
                "commit",
                "-m",
                "init",
            ],
        );

        let clean =
            EditLoop::check_workspace_dirty(workspace.path(), &[PathBuf::from("target.rs")])
                .expect("clean check");
        assert!(
            !clean,
            "workspace should report clean immediately after commit"
        );

        fs::write(
            workspace.path().join("target.rs"),
            "fn main() { /* dirty */ }\n",
        )
        .expect("mutate");
        let dirty =
            EditLoop::check_workspace_dirty(workspace.path(), &[PathBuf::from("target.rs")])
                .expect("dirty check");
        assert!(
            dirty,
            "workspace should report dirty after tracked file change"
        );
    }

    #[tokio::test]
    async fn test_edit_loop_cancel_mid_validation() {
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancel_clone.cancel();
        });

        let mut edit_loop = EditLoop::new("task-cancel-mid".to_string()).with_max_turns(4);
        let (mut ctx, _rx) = make_runtime_context();
        let outcome = edit_loop
            .run("edit src/lib.rs".to_string(), &mut ctx, &cancel)
            .await
            .expect("run should not error");

        assert!(
            matches!(outcome, EditLoopOutcome::Cancelled),
            "loop must return Cancelled when token fires mid-run"
        );
    }

    #[tokio::test]
    async fn test_edit_loop_run_emits_dirty_workspace_warning_to_transcript() {
        let _env_lock = crate::test_support::ENV_LOCK.lock().await;
        let original_dir = std::env::current_dir().expect("current_dir");
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::write(workspace.path().join("tracked.txt"), "v1\n").expect("seed file");
        run_git(workspace.path(), &["init"]);
        run_git(workspace.path(), &["add", "tracked.txt"]);
        run_git(
            workspace.path(),
            &[
                "-c",
                "user.name=vex-test",
                "-c",
                "user.email=vex-test@example.com",
                "commit",
                "-m",
                "init",
            ],
        );
        fs::write(workspace.path().join("tracked.txt"), "v2\n").expect("mutate file");
        std::env::set_current_dir(workspace.path()).expect("set_current_dir");

        let mut edit_loop = EditLoop::new("task-dirty-warning".to_string()).with_max_turns(1);
        let (mut ctx, mut rx) = make_runtime_context();
        let cancel = CancellationToken::new();
        let outcome = edit_loop
            .run("edit tracked.txt".to_string(), &mut ctx, &cancel)
            .await
            .expect("run should succeed");
        std::env::set_current_dir(original_dir).expect("restore current_dir");

        let warning = rx.recv().await.expect("expected transcript update");
        match warning {
            UiUpdate::TranscriptLine(line) => {
                assert!(
                    line.contains("workspace has uncommitted changes"),
                    "expected workspace-dirty warning, got: {line}"
                );
            }
            _ => panic!("expected transcript warning update"),
        }
        assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
    }
}
