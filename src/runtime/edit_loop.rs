use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio_util::sync::CancellationToken;

use crate::runtime::ConfiguredSandbox;
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
    sandbox: ConfiguredSandbox,
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
            working_dir: std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir()),
            sandbox: ConfiguredSandbox::default(),
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

    pub fn with_sandbox(mut self, sandbox: ConfiguredSandbox) -> Self {
        self.sandbox = sandbox;
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
        match Self::check_workspace_dirty(&self.working_dir, &[]) {
            Ok(true) => {
                ctx.emit_transcript_line(
                    "[edit loop warning: workspace has uncommitted changes; proceeding without mutating git state]"
                        .to_string(),
                );
            }
            Ok(false) => {}
            Err(error) => {
                ctx.emit_transcript_line(format!(
                    "[edit loop warning: skipped workspace-dirty check: {error}]"
                ));
            }
        }

        let root = self.working_dir.clone();
        let validation_suite = ValidationSuite::load_or_infer(&root);
        let runner = DefaultCommandRunner::new();

        let mut retry_context = String::new();
        for pulse in 0..self.max_turns {
            if cancel.is_cancelled() {
                return Ok(EditLoopOutcome::Cancelled);
            }

            tokio::task::yield_now().await;

            let message = if retry_context.is_empty() {
                instruction.clone()
            } else {
                format!("{instruction}\n\n{retry_context}")
            };

            ctx.emit_transcript_line(format!("[edit loop pulse {}/{}]", pulse + 1, self.max_turns));

            let patch_applied = match ctx.drive_edit_turn(message).await {
                Ok(turn_result) => turn_result.patch_applied,
                Err(err) => {
                    ctx.emit_transcript_line(format!("[edit loop pulse error: {err}]"));
                    retry_context = format!("[previous pulse failed: {err}]");
                    continue;
                }
            };

            if cancel.is_cancelled() {
                return Ok(EditLoopOutcome::Cancelled);
            }

            if !patch_applied {
                ctx.emit_transcript_line("[edit loop: no patch applied, retrying]".to_string());
                retry_context =
                    "[previous pulse produced no patch; propose and apply a concrete edit]"
                        .to_string();
                continue;
            }

            ctx.emit_transcript_line("[edit loop: running validation]".to_string());
            let validation_result = validation_suite
                .run_in_dir_with_sandbox(&runner, &self.sandbox, Some(&root))
                .await?;
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

        let output = match command.output() {
            Ok(o) => o,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error)
                    .context("failed to call git status for workspace-dirty check");
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
            if stderr.contains("not a git repository") {
                return Ok(false);
            }
            anyhow::bail!(
                "git status failed for workspace-dirty check: {}",
                stderr.trim()
            );
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

fn clamp_turns(pulses: u8) -> u8 {
    pulses.clamp(1, HARD_MAX_TURNS)
}

#[cfg(test)]
mod tests;
