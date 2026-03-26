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
        // EL-03 step 1: workspace-dirty warning.
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

        // EL-04: assemble → model → apply → validate → retry cycle.
        let root = self.working_dir.clone();
        let validation_suite = ValidationSuite::load_or_infer(&root);
        let runner = DefaultCommandRunner::new();

        let mut retry_context = String::new();
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
            let patch_applied = match ctx.drive_edit_turn(message).await {
                Ok(turn_result) => turn_result.patch_applied,
                Err(err) => {
                    ctx.emit_transcript_line(format!("[edit loop turn error: {err}]"));
                    retry_context = format!("[previous turn failed: {err}]");
                    continue;
                }
            };

            if cancel.is_cancelled() {
                return Ok(EditLoopOutcome::Cancelled);
            }

            if !patch_applied {
                ctx.emit_transcript_line("[edit loop: no patch applied, retrying]".to_string());
                retry_context =
                    "[previous turn produced no patch; propose and apply a concrete edit]"
                        .to_string();
                continue;
            }

            // Validate: run the project validation suite concurrently.
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

        let output = match command.output() {
            Ok(o) => o,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error)
                    .context("failed to execute git status for workspace-dirty check");
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

fn clamp_turns(turns: u8) -> u8 {
    turns.clamp(1, HARD_MAX_TURNS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::runtime::UiUpdate;
    use crate::state::ConversationManager;
    use crate::tools::ToolOperator;
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

    fn final_text_turn(message_id: &str, text: &str) -> Vec<String> {
        vec![
            format!(
                r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
            ),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
                .to_string(),
            format!(
                r#"event: content_block_delta
data: {{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{text}"}}}}"#
            ),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#
                .to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#
                .to_string(),
        ]
    }

    fn write_file_turn(message_id: &str, tool_id: &str, path: &str, content: &str) -> Vec<String> {
        let partial_json = serde_json::json!({
            "path": path,
            "content": content,
        })
        .to_string();
        let partial_json = serde_json::to_string(&partial_json).expect("partial json string");

        vec![
            format!(
                r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
            ),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_write_01","name":"write_file","input":{}}}"#
                .replace("toolu_write_01", tool_id),
            format!(
                r#"event: content_block_delta
data: {{"type":"content_block_delta","index":0,"delta":{{"type":"input_json_delta","partial_json":{partial_json}}}}}"#
            ),
            r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#
                .to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":8}}"#
                .to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#
                .to_string(),
        ]
    }

    #[tokio::test]
    async fn test_edit_loop_skips_validation_when_no_patch_is_applied() {
        let workspace = tempfile::tempdir().expect("tempdir");
        run_git(workspace.path(), &["init"]);

        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![final_text_turn(
            "msg-no-patch",
            "I need more context before editing.",
        )])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());
        let mut edit_loop = EditLoop::new("task-no-patch".to_string())
            .with_max_turns(1)
            .with_working_dir(workspace.path().to_path_buf());
        let cancel = CancellationToken::new();

        let outcome = edit_loop
            .run("edit src/lib.rs".to_string(), &mut ctx, &cancel)
            .await
            .expect("run should succeed");

        assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
        assert!(
            edit_loop.last_validation_result().is_none(),
            "validation must not run when the turn applied no patch"
        );
    }

    #[tokio::test]
    async fn test_edit_loop_validation_failure_retries_after_patch_and_stops_at_max_turns() {
        let workspace = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(workspace.path().join(".vex")).expect("create .vex");
        std::fs::create_dir_all(workspace.path().join("src")).expect("create src");
        #[cfg(not(windows))]
        std::fs::write(
            workspace.path().join(".vex/validate.toml"),
            r#"[[commands]]
label = "forced failure"
program = "sh"
args = ["-c", "printf 'still failing\n' >&2; exit 1"]
"#,
        )
        .expect("write validate config");
        #[cfg(windows)]
        std::fs::write(
            workspace.path().join(".vex/validate.toml"),
            r#"[[commands]]
label = "forced failure"
program = "cmd"
args = ["/C", "echo still failing 1>&2 && exit /b 1"]
"#,
        )
        .expect("write validate config");

        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![
            write_file_turn(
                "msg-write-1",
                "toolu_write_1",
                "src/lib.rs",
                "pub const RETRY_ROUND: usize = 1;\n",
            ),
            final_text_turn("msg-final-1", "Applied the first patch."),
            write_file_turn(
                "msg-write-2",
                "toolu_write_2",
                "src/lib.rs",
                "pub const RETRY_ROUND: usize = 2;\n",
            ),
            final_text_turn("msg-final-2", "Applied the retry patch."),
        ])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(workspace.path().to_path_buf()));
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());
        let transcript = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let transcript_reader = Arc::clone(&transcript);

        let reader = tokio::spawn(async move {
            while let Some(update) = rx.recv().await {
                match update {
                    UiUpdate::ToolApprovalRequest(request) => {
                        let _ = request.response_tx.send(true);
                    }
                    UiUpdate::TranscriptLine(line) => {
                        transcript_reader.lock().unwrap().push(line);
                    }
                    UiUpdate::StreamDelta(_)
                    | UiUpdate::StreamBlockStart { .. }
                    | UiUpdate::StreamBlockDelta { .. }
                    | UiUpdate::StreamBlockComplete { .. }
                    | UiUpdate::CommandSessionStarted { .. }
                    | UiUpdate::CommandSessionAttached { .. }
                    | UiUpdate::CommandSessionFinished { .. }
                    | UiUpdate::TurnComplete
                    | UiUpdate::EditLoopComplete { .. }
                    | UiUpdate::Error(_)
                    | UiUpdate::ContextCompacted { .. } => {}
                }
            }
        });

        let mut edit_loop = EditLoop::new("task-validation-retry".to_string())
            .with_max_turns(2)
            .with_working_dir(workspace.path().to_path_buf());
        let cancel = CancellationToken::new();

        let outcome = edit_loop
            .run("edit src/lib.rs".to_string(), &mut ctx, &cancel)
            .await
            .expect("run should succeed");

        drop(ctx);
        reader.await.expect("reader task");

        assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
        let transcript = transcript.lock().unwrap();
        assert!(
            transcript
                .iter()
                .any(|line| line.contains("[edit loop: validation failed, retrying]")),
            "validation failure must trigger retry guidance: {:?}",
            *transcript
        );
        assert!(
            transcript
                .iter()
                .any(|line| line.contains("[edit loop turn 2/2]")),
            "validation failure must advance the orchestrator into the next turn: {:?}",
            *transcript
        );
        assert!(
            workspace.path().join("src/lib.rs").is_file(),
            "write_file turn must mutate the workspace before validation retries"
        );
        let content = std::fs::read_to_string(workspace.path().join("src/lib.rs"))
            .expect("read retried file");
        assert!(
            content.contains("RETRY_ROUND"),
            "mutation turn must leave a concrete edit behind before validation retries: {content}"
        );
        let validation = edit_loop
            .last_validation_result()
            .expect("last validation result");
        assert!(
            !validation.passed,
            "forced validation failure must remain recorded in edit-loop state"
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
