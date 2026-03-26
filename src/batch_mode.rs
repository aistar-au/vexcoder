use anyhow::Result;
use std::collections::{BTreeSet, HashMap, VecDeque};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::mcp::McpRegistry;
use crate::runtime::{
    context::RuntimeContext,
    frontend::{FrontendAdapter, UserInputEvent},
    mode::RuntimeMode,
    project_instructions::{load_project_instructions, LoadResult},
    r#loop::Runtime,
    resolve_configured_sandbox,
    task_state::{CommandEvidence, TaskId, TaskStatus},
    TaskState, UiUpdate,
};
use crate::session_notes::{build_api_client_with_notes, clear_notes_file};
use crate::state::{ConversationManager, StreamBlock, ToolApprovalRequest};
use crate::tools::ToolOperator;
use crate::turn_evidence::{
    command_evidence_from_tool_result, note_changed_files_from_tool_call, SummaryRecord,
    TurnEvidenceRecord,
};
use crate::usage::TurnTokens;
use std::path::PathBuf;

// ── Output format ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jsonl,
    Text,
}

// ── Auto-approve scope ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoApproveScope {
    /// Grant each capability for the current turn only.
    Once,
    /// Grant each capability for the entire batch run.
    Task,
}

// ── Batch run options ──────────────────────────────────────────────────────────

pub struct BatchRunOpts {
    pub max_turns: Option<usize>,
    pub auto_approve: Option<AutoApproveScope>,
    pub format: OutputFormat,
    pub resume_state: Option<TaskState>,
}

impl Default for BatchRunOpts {
    fn default() -> Self {
        Self {
            max_turns: None,
            auto_approve: None,
            format: OutputFormat::Jsonl,
            resume_state: None,
        }
    }
}

// ── Batch result ───────────────────────────────────────────────────────────────

pub struct BatchResult {
    pub status: TaskStatus,
    pub output_lines: Vec<String>,
    pub turn_count: usize,
    pub task_id: TaskId,
}

// ── BatchMode (RuntimeMode impl) ───────────────────────────────────────────────

pub struct BatchMode {
    task_id: TaskId,
    status: TaskStatus,
    turn_in_progress: bool,
    done: bool,
    max_turns: Option<usize>,
    auto_approve: Option<AutoApproveScope>,
    format: OutputFormat,
    notes_path: Option<PathBuf>,
    instructions_path: Option<String>,
    current_response: String,
    current_turn: usize,
    current_turn_input: String,
    current_turn_changed_files: BTreeSet<String>,
    task_changed_files: BTreeSet<String>,
    current_turn_command_history: Vec<CommandEvidence>,
    current_turn_tokens: TurnTokens,
    pending_tool_calls: HashMap<String, PendingToolCall>,
    output_lines: Vec<String>,
}

#[derive(Clone)]
struct PendingToolCall {
    name: String,
    input: serde_json::Value,
}

impl BatchMode {
    pub fn new(
        task_id: TaskId,
        opts: BatchRunOpts,
        notes_path: Option<PathBuf>,
        instructions_path: Option<String>,
    ) -> Self {
        Self {
            task_id,
            status: TaskStatus::Ready,
            turn_in_progress: false,
            done: false,
            max_turns: opts.max_turns,
            auto_approve: opts.auto_approve,
            format: opts.format,
            notes_path,
            instructions_path,
            current_response: String::new(),
            current_turn: 0,
            current_turn_input: String::new(),
            current_turn_changed_files: BTreeSet::new(),
            task_changed_files: BTreeSet::new(),
            current_turn_command_history: Vec::new(),
            current_turn_tokens: TurnTokens::default(),
            pending_tool_calls: HashMap::new(),
            output_lines: Vec::new(),
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn status(&self) -> &TaskStatus {
        &self.status
    }

    pub fn output_lines(&self) -> &[String] {
        &self.output_lines
    }

    // The approval response channel is currently binary. AutoApproveScope keeps
    // the public API distinction between Once and Task, but scope-sensitive
    // enforcement remains deferred until the handler consumes scoped grants.
    fn approval_decision(&self) -> bool {
        self.auto_approve.is_some()
    }

    fn reset_current_turn_state(&mut self) {
        self.current_response.clear();
        self.current_turn_input.clear();
        self.current_turn_changed_files.clear();
        self.current_turn_command_history.clear();
        self.current_turn_tokens = TurnTokens::default();
        self.pending_tool_calls.clear();
    }

    fn mark_max_turns_reached(&mut self) {
        self.status = TaskStatus::MaxTurnsReached;
        self.append_summary();
        self.done = true;
        self.turn_in_progress = false;
    }

    fn finish_turn(&mut self, tokens: TurnTokens) {
        self.current_turn += 1;
        self.current_turn_tokens = tokens;

        let response = std::mem::take(&mut self.current_response);
        let input_text = std::mem::take(&mut self.current_turn_input);
        let changed_files = self
            .current_turn_changed_files
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        self.task_changed_files
            .extend(changed_files.iter().cloned());
        let command_history = std::mem::take(&mut self.current_turn_command_history);
        match self.format {
            OutputFormat::Jsonl => {
                let record = TurnEvidenceRecord {
                    turn: self.current_turn,
                    input: input_text,
                    response,
                    instructions_path: self.instructions_path.clone(),
                    changed_files,
                    command_history,
                    tokens: self.current_turn_tokens,
                };
                let line = serde_json::to_string(&record)
                    .expect("batch JSONL turn record serialization must succeed");
                self.output_lines.push(line);
            }
            OutputFormat::Text => {
                if self.current_turn > 1 {
                    self.output_lines.push(String::new());
                }
                self.output_lines.push(response);
            }
        }

        self.turn_in_progress = false;
        self.current_turn_tokens = TurnTokens::default();
        self.pending_tool_calls.clear();
    }

    fn append_summary(&mut self) {
        if self.format == OutputFormat::Jsonl {
            let status_str = format!("{:?}", self.status);
            let record = SummaryRecord {
                summary: true,
                status: status_str,
                task_id: self.task_id.clone(),
                total_turns: self.current_turn,
                instructions_path: self.instructions_path.clone(),
                changed_files: self.task_changed_files.iter().cloned().collect(),
                session_tasks: Vec::new(),
            };
            let line = serde_json::to_string(&record)
                .expect("batch JSONL summary record serialization must succeed");
            self.output_lines.push(line);
        }
    }

    fn complete_without_model(&mut self, input: &str, message: String) {
        self.reset_current_turn_state();
        self.status = TaskStatus::Running;
        self.turn_in_progress = true;
        self.current_turn_input = input.to_string();
        self.current_response = message;
        self.finish_turn(TurnTokens::default());
        if !self.done {
            self.status = TaskStatus::Completed;
            self.append_summary();
            self.done = true;
        }
    }

    fn fail_without_model(&mut self, input: &str, message: String) {
        self.reset_current_turn_state();
        self.status = TaskStatus::Running;
        self.turn_in_progress = true;
        self.current_turn_input = input.to_string();
        self.current_response = message;
        self.finish_turn(TurnTokens::default());
        if !self.done {
            self.status = TaskStatus::Failed;
            self.append_summary();
            self.done = true;
        }
    }

    fn try_handle_batch_memory_clear(&mut self, input: &str) -> bool {
        if input.trim() != "/memory clear" {
            return false;
        }

        if self.auto_approve.is_none() {
            self.fail_without_model(
                input,
                "[memory] /memory clear requires --auto-approve in batch mode".to_string(),
            );
            return true;
        }

        match clear_notes_file(self.notes_path.as_deref()) {
            Ok(()) => self.complete_without_model(input, "[memory: cleared]".to_string()),
            Err(e) => self.fail_without_model(input, format!("[memory] error clearing: {e}")),
        }
        true
    }

    fn note_changed_files_from_tool_call(&mut self, name: &str, input: &serde_json::Value) {
        note_changed_files_from_tool_call(&mut self.current_turn_changed_files, name, input);
    }

    fn note_command_history_from_tool_result(&mut self, name: &str, is_error: bool) {
        if let Some(evidence) = command_evidence_from_tool_result(name, is_error) {
            self.current_turn_command_history.push(evidence);
        }
    }
}

fn resolve_batch_project_instructions(config: &Config) -> (Option<String>, Option<String>) {
    match load_project_instructions(&config.working_dir, config.max_project_instructions_tokens) {
        LoadResult::Loaded(project_instructions) => {
            let display = project_instructions.path.to_string_lossy().into_owned();
            (Some(project_instructions.content), Some(display))
        }
        LoadResult::OverBudget {
            path,
            estimated_tokens,
        } => {
            eprintln!(
                "[project instructions] {} skipped: estimated {} tokens exceeds budget of {}",
                path.display(),
                estimated_tokens,
                config.max_project_instructions_tokens,
            );
            (None, None)
        }
        LoadResult::NotFound => (None, None),
    }
}

impl RuntimeMode for BatchMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        if self.done {
            return;
        }
        let max_reached = self
            .max_turns
            .map(|max| self.current_turn >= max)
            .unwrap_or(false);
        if max_reached {
            self.mark_max_turns_reached();
            return;
        }
        if self.try_handle_batch_memory_clear(&input) {
            return;
        }
        self.reset_current_turn_state();
        self.status = TaskStatus::Running;
        self.turn_in_progress = true;
        self.current_turn_input = input.clone();
        ctx.start_turn(input);
    }

    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::TranscriptLine(_) => {}
            UiUpdate::StreamDelta(text) => {
                self.current_response.push_str(&text);
            }
            UiUpdate::TurnComplete => {
                self.finish_turn(ctx.session_tokens_snapshot().last_turn());
                if !self.done {
                    self.status = TaskStatus::Completed;
                    self.append_summary();
                    self.done = true;
                }
            }
            UiUpdate::Error(msg) => {
                self.current_response.push_str(&msg);
                self.finish_turn(TurnTokens::default());
                if !self.done {
                    self.status = TaskStatus::Failed;
                    self.append_summary();
                    self.done = true;
                }
            }
            UiUpdate::StreamBlockStart { block, .. } => match block {
                StreamBlock::ToolCall {
                    id, name, input, ..
                } => {
                    self.pending_tool_calls
                        .insert(id, PendingToolCall { name, input });
                }
                StreamBlock::ToolResult {
                    tool_call_id,
                    is_error,
                    ..
                } => {
                    if let Some(pending) = self.pending_tool_calls.remove(&tool_call_id) {
                        if !is_error {
                            self.note_changed_files_from_tool_call(&pending.name, &pending.input);
                        }
                        self.note_command_history_from_tool_result(&pending.name, is_error);
                    }
                }
                StreamBlock::Thinking { .. } | StreamBlock::FinalText { .. } => {}
            },
            UiUpdate::StreamBlockDelta { .. } | UiUpdate::StreamBlockComplete { .. } => {}
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest { response_tx, .. }) => {
                let approved = self.approval_decision();
                let _ = response_tx.send(approved);
            }
            UiUpdate::CommandSessionStarted { .. } => {}
            UiUpdate::CommandSessionAttached { .. } => {}
            UiUpdate::CommandSessionFinished { .. } => {}
            UiUpdate::EditLoopComplete { .. } => {}
            UiUpdate::ContextCompacted { .. } => {}
        }
    }

    fn is_turn_in_progress(&self) -> bool {
        self.turn_in_progress
    }
}

// ── BatchFrontend (FrontendAdapter impl) ───────────────────────────────────────

pub struct BatchFrontend {
    pending: VecDeque<UserInputEvent>,
}

impl BatchFrontend {
    pub fn new(task: String) -> Self {
        let mut pending = VecDeque::new();
        pending.push_back(UserInputEvent::Text(task));
        Self { pending }
    }
}

impl FrontendAdapter<BatchMode> for BatchFrontend {
    fn poll_user_input(&mut self, mode: &BatchMode) -> Option<UserInputEvent> {
        if mode.is_done() {
            return None;
        }
        if mode.is_turn_in_progress() {
            return None;
        }
        self.pending.pop_front()
    }

    fn render(&mut self, _mode: &BatchMode) {}

    fn should_quit(&self) -> bool {
        false
    }
}

/// Wraps `BatchFrontend` and quits once the mode signals done via a shared
/// flag. In practice `run_batch` drives the update loop directly, so this
/// wrapper is provided for callers that want to use `Runtime::run`.
pub struct BatchFrontendQuit {
    inner: BatchFrontend,
    done: bool,
}

impl BatchFrontendQuit {
    pub fn new(task: String) -> Self {
        Self {
            inner: BatchFrontend::new(task),
            done: false,
        }
    }

    /// Signal that the mode has finished so `should_quit` returns `true`.
    pub fn set_done(&mut self) {
        self.done = true;
    }
}

impl FrontendAdapter<BatchMode> for BatchFrontendQuit {
    fn poll_user_input(&mut self, mode: &BatchMode) -> Option<UserInputEvent> {
        if mode.is_done() {
            self.done = true;
        }
        self.inner.poll_user_input(mode)
    }

    fn render(&mut self, mode: &BatchMode) {
        if mode.is_done() {
            self.done = true;
        }
    }

    fn should_quit(&self) -> bool {
        self.done
    }
}

// ── Public batch execution entry points ───────────────────────────────────────

/// Build a `Runtime<BatchMode>` from config for callers that want to drive the
/// loop themselves via `Runtime::run`. Most callers should use `run_batch`.
pub fn build_batch_runtime(
    config: &Config,
    _task: String,
    opts: BatchRunOpts,
) -> Result<(Runtime<BatchMode>, RuntimeContext, TaskId)> {
    let task_id = opts
        .resume_state
        .as_ref()
        .map(|state| state.id.clone())
        .unwrap_or_else(uuid_task_id);
    let (sandbox, sandbox_warning) = resolve_configured_sandbox(&config.sandbox)?;
    let (instructions_text, instructions_path) = resolve_batch_project_instructions(config);
    let (client, notes_warning) = build_api_client_with_notes(config)?;
    let mcp_registry = McpRegistry::connect_all_blocking(&config.mcp_servers)?;
    let client = client
        .with_project_instructions(instructions_text)
        .with_extra_tool_definitions(
            mcp_registry
                .as_ref()
                .map(|registry| registry.tool_definitions())
                .unwrap_or_default(),
        );
    if let Some(warning) = notes_warning {
        eprintln!("{warning}");
    }
    if let Some(warning) = sandbox_warning {
        eprintln!("{warning}");
    }
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new_with_hooks(client, operator, config.hooks.clone())
        .with_sandbox(sandbox)
        .with_mcp_registry(mcp_registry);

    let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let mode = BatchMode::new(
        task_id.clone(),
        opts,
        config.notes_path.clone(),
        instructions_path,
    );
    let runtime = Runtime::new(mode, update_rx);

    Ok((runtime, ctx, task_id))
}

fn uuid_task_id() -> TaskId {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("batch-{}", ts)
}

/// Drive a batch run to completion by polling the update channel directly.
/// This is the primary entry point for `vex exec`.
pub async fn run_batch(task: String, opts: BatchRunOpts, config: &Config) -> Result<BatchResult> {
    let task_id = opts
        .resume_state
        .as_ref()
        .map(|state| state.id.clone())
        .unwrap_or_else(uuid_task_id);
    let (sandbox, sandbox_warning) = resolve_configured_sandbox(&config.sandbox)?;
    let (instructions_text, instructions_path) = resolve_batch_project_instructions(config);
    let (client, notes_warning) = build_api_client_with_notes(config)?;
    let mcp_registry = McpRegistry::connect_all_blocking(&config.mcp_servers)?;
    let client = client
        .with_project_instructions(instructions_text)
        .with_extra_tool_definitions(
            mcp_registry
                .as_ref()
                .map(|registry| registry.tool_definitions())
                .unwrap_or_default(),
        );
    if let Some(warning) = notes_warning {
        eprintln!("{warning}");
    }
    if let Some(warning) = sandbox_warning {
        eprintln!("{warning}");
    }
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new_with_hooks(client, operator, config.hooks.clone())
        .with_sandbox(sandbox)
        .with_mcp_registry(mcp_registry);

    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let mut mode = BatchMode::new(
        task_id.clone(),
        opts,
        config.notes_path.clone(),
        instructions_path,
    );

    // Submit the initial task.
    mode.on_user_input(task, &mut ctx);

    if mode.is_done() {
        return Ok(BatchResult {
            status: mode.status,
            output_lines: mode.output_lines,
            turn_count: mode.current_turn,
            task_id,
        });
    }

    // Drain updates until the mode reports done.
    while let Some(update) = update_rx.recv().await {
        mode.on_model_update(update, &mut ctx);
        if mode.is_done() {
            break;
        }
    }

    Ok(BatchResult {
        status: mode.status,
        output_lines: mode.output_lines,
        turn_count: mode.current_turn,
        task_id,
    })
}

// ── Test helpers ───────────────────────────────────────────────────────────────

#[cfg(test)]
pub async fn run_batch_mode(task: &str, _max_turns: usize) -> Result<BatchResult> {
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::state::ConversationManager;
    use std::collections::HashMap;
    use std::sync::Arc;

    let mock = Arc::new(MockApiClient::new(vec![vec![]]));
    let client = ApiClient::new_mock(mock);
    let conversation = ConversationManager::new_mock(client, HashMap::new());

    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let opts = BatchRunOpts {
        max_turns: Some(_max_turns),
        ..Default::default()
    };
    let task_id = "test-task".to_string();
    let mut mode = BatchMode::new(task_id.clone(), opts, None, None);

    mode.on_user_input(task.to_string(), &mut ctx);

    if mode.is_done() {
        return Ok(BatchResult {
            status: mode.status,
            output_lines: mode.output_lines,
            turn_count: mode.current_turn,
            task_id,
        });
    }

    while let Some(update) = update_rx.recv().await {
        mode.on_model_update(update, &mut ctx);
        if mode.is_done() {
            break;
        }
    }

    Ok(BatchResult {
        status: mode.status,
        output_lines: mode.output_lines,
        turn_count: mode.current_turn,
        task_id,
    })
}

#[cfg(test)]
pub async fn run_batch_mode_with_opts(task: &str, opts: BatchRunOpts) -> Result<BatchResult> {
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::state::ConversationManager;
    use std::collections::HashMap;
    use std::sync::Arc;

    let mock = Arc::new(MockApiClient::new(vec![vec![]]));
    let client = ApiClient::new_mock(mock);
    let conversation = ConversationManager::new_mock(client, HashMap::new());

    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let task_id = "test-task".to_string();
    let mut mode = BatchMode::new(task_id.clone(), opts, None, None);

    mode.on_user_input(task.to_string(), &mut ctx);

    if mode.is_done() {
        return Ok(BatchResult {
            status: mode.status,
            output_lines: mode.output_lines,
            turn_count: mode.current_turn,
            task_id,
        });
    }

    while let Some(update) = update_rx.recv().await {
        mode.on_model_update(update, &mut ctx);
        if mode.is_done() {
            break;
        }
    }

    Ok(BatchResult {
        status: mode.status,
        output_lines: mode.output_lines,
        turn_count: mode.current_turn,
        task_id,
    })
}

#[cfg(test)]
pub async fn capture_batch_jsonl(task: &str, max_turns: usize) -> Result<String> {
    let result = run_batch_mode(task, max_turns).await?;
    Ok(result.output_lines.join("\n"))
}

#[cfg(test)]
pub async fn capture_batch_text(task: &str, max_turns: usize) -> Result<String> {
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::state::ConversationManager;
    use std::collections::HashMap;
    use std::sync::Arc;

    let mock = Arc::new(MockApiClient::new(vec![vec![]]));
    let client = ApiClient::new_mock(mock);
    let conversation = ConversationManager::new_mock(client, HashMap::new());

    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let opts = BatchRunOpts {
        max_turns: Some(max_turns),
        format: OutputFormat::Text,
        ..Default::default()
    };
    let task_id = "test-task".to_string();
    let mut mode = BatchMode::new(task_id.clone(), opts, None, None);

    mode.on_user_input(task.to_string(), &mut ctx);

    if mode.is_done() {
        return Ok(mode.output_lines.join("\n"));
    }

    while let Some(update) = update_rx.recv().await {
        mode.on_model_update(update, &mut ctx);
        if mode.is_done() {
            break;
        }
    }

    Ok(mode.output_lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::runtime::{EditLoopOutcome, TaskStatus};

    #[tokio::test]
    async fn test_batch_mode_exits_zero_on_completion() {
        let result = run_batch_mode("echo hello", 3).await.unwrap();
        assert_eq!(result.status, TaskStatus::Completed);
    }

    #[test]
    fn test_batch_mode_ignores_transcript_and_edit_loop_updates() {
        let opts = BatchRunOpts::default();
        let mut mode = BatchMode::new("task-batch".to_string(), opts, None, None);
        let client = crate::api::ApiClient::new_mock(std::sync::Arc::new(
            crate::api::mock_client::MockApiClient::new(vec![]),
        ));
        let conversation =
            crate::state::ConversationManager::new_mock(client, std::collections::HashMap::new());
        let (update_tx, _update_rx) = tokio::sync::mpsc::unbounded_channel::<UiUpdate>();
        let mut ctx = RuntimeContext::new(
            conversation,
            update_tx,
            tokio_util::sync::CancellationToken::new(),
        );

        mode.on_model_update(UiUpdate::TranscriptLine("warning".to_string()), &mut ctx);
        mode.on_model_update(
            UiUpdate::EditLoopComplete {
                outcome: EditLoopOutcome::Cancelled,
                last_validation_result: None,
            },
            &mut ctx,
        );

        assert!(mode.current_response.is_empty());
        assert!(mode.output_lines.is_empty());
        assert_eq!(mode.status, TaskStatus::Ready);
    }

    #[tokio::test]
    async fn test_batch_mode_completes_on_final_allowed_turn() {
        let result = run_batch_mode_with_opts(
            "keep going",
            BatchRunOpts {
                max_turns: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(result.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_batch_mode_interactive_approval_denied_by_default() {
        let result = run_batch_mode_with_opts(
            "run: ls",
            BatchRunOpts {
                auto_approve: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            result.status,
            TaskStatus::Completed | TaskStatus::Failed
        ));
    }

    #[tokio::test]
    async fn test_batch_mode_auto_approve_once_grants_single_turn() {
        let result = run_batch_mode_with_opts(
            "run: echo approved",
            BatchRunOpts {
                auto_approve: Some(AutoApproveScope::Once),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(result.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_batch_mode_memory_clear_requires_auto_approve() {
        let result = run_batch_mode_with_opts(
            "/memory clear",
            BatchRunOpts {
                format: OutputFormat::Text,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(result.status, TaskStatus::Failed);
        assert!(
            result
                .output_lines
                .iter()
                .any(|line| line.contains("requires --auto-approve")),
            "expected batch-mode memory clear guidance in output"
        );
    }

    #[tokio::test]
    async fn test_batch_mode_memory_clear_with_auto_approve_clears_notes() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        std::fs::write(&notes_path, "batch note\n").unwrap();

        let config = Config {
            model_token: None,
            model_name: "mock-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            model_url_skip_tls_check: false,
            working_dir: temp.path().to_path_buf(),
            model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
            model_protocol: crate::runtime::ModelProtocol::MessagesV1,
            tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                crate::runtime::ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            notes_path: Some(notes_path.clone()),
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };

        let result = run_batch(
            "/memory clear".to_string(),
            BatchRunOpts {
                auto_approve: Some(AutoApproveScope::Task),
                format: OutputFormat::Text,
                ..Default::default()
            },
            &config,
        )
        .await
        .unwrap();

        assert_eq!(result.status, TaskStatus::Completed);
        assert!(
            result
                .output_lines
                .iter()
                .any(|line| line.contains("[memory: cleared]")),
            "expected batch-mode memory clear confirmation in output"
        );
        assert_eq!(
            std::fs::read_to_string(&notes_path).unwrap(),
            "",
            "expected batch-mode memory clear to truncate the notes file"
        );
    }

    #[tokio::test]
    async fn test_batch_mode_memory_clear_with_max_turns_emits_single_summary() {
        let result = run_batch_mode_with_opts(
            "/memory clear",
            BatchRunOpts {
                max_turns: Some(1),
                format: OutputFormat::Jsonl,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let summary_count = result
            .output_lines
            .iter()
            .filter(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .and_then(|value| value.get("summary").and_then(|flag| flag.as_bool()))
                    == Some(true)
            })
            .count();

        assert_eq!(summary_count, 1, "expected exactly one summary record");
    }

    #[tokio::test]
    async fn test_batch_mode_jsonl_output_includes_required_fields() {
        let output = capture_batch_jsonl("echo hello", 3).await.unwrap();
        let records = output
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let first_turn = records
            .iter()
            .find(|value| {
                !value
                    .get("summary")
                    .and_then(|flag| flag.as_bool())
                    .unwrap_or(false)
            })
            .expect("expected a turn record before the final summary");
        assert_eq!(first_turn["turn"], 1);
        assert_eq!(first_turn["input"], "echo hello");
        assert!(first_turn.get("response").is_some());
        assert!(first_turn["changed_files"].is_array());
        assert!(first_turn["command_history"].is_array());

        let summary = records
            .iter()
            .find(|value| value.get("summary").and_then(|flag| flag.as_bool()) == Some(true))
            .expect("expected a final summary record");
        assert_eq!(summary["status"], "Completed");
    }

    #[tokio::test]
    async fn test_batch_mode_jsonl_output_captures_streamed_tool_evidence() {
        let mut ctx = setup_batch_ctx();
        let opts = BatchRunOpts {
            format: OutputFormat::Jsonl,
            ..Default::default()
        };
        let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);

        mode.on_user_input("task".to_string(), &mut ctx);
        mode.on_model_update(
            UiUpdate::StreamBlockStart {
                index: 0,
                block: StreamBlock::ToolCall {
                    id: "tool-1".to_string(),
                    name: "write_file".to_string(),
                    input: serde_json::json!({
                        "path": "src/main.rs",
                        "content": "fn main() {}\n"
                    }),
                    status: crate::state::ToolStatus::Executing,
                },
            },
            &mut ctx,
        );
        mode.on_model_update(
            UiUpdate::StreamBlockStart {
                index: 1,
                block: StreamBlock::ToolResult {
                    tool_call_id: "tool-1".to_string(),
                    output: "wrote file".to_string(),
                    is_error: false,
                },
            },
            &mut ctx,
        );
        mode.on_model_update(
            UiUpdate::StreamBlockStart {
                index: 1,
                block: StreamBlock::ToolCall {
                    id: "tool-2".to_string(),
                    name: "git_commit".to_string(),
                    input: serde_json::json!({
                        "message": "record evidence"
                    }),
                    status: crate::state::ToolStatus::Executing,
                },
            },
            &mut ctx,
        );
        mode.on_model_update(
            UiUpdate::StreamBlockStart {
                index: 2,
                block: StreamBlock::ToolResult {
                    tool_call_id: "tool-2".to_string(),
                    output: "Committed".to_string(),
                    is_error: false,
                },
            },
            &mut ctx,
        );
        mode.on_model_update(UiUpdate::StreamDelta("done".to_string()), &mut ctx);
        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

        let turn_line = mode.output_lines.first().expect("expected turn line");
        let turn_json: serde_json::Value = serde_json::from_str(turn_line).unwrap();
        let changed_files = turn_json["changed_files"]
            .as_array()
            .expect("changed_files must be present");
        assert!(changed_files
            .iter()
            .filter_map(|value| value.as_str())
            .any(|path| path == "src/main.rs"));
        let command_history = turn_json["command_history"]
            .as_array()
            .expect("command_history must be present");
        assert_eq!(command_history.len(), 1);
        assert_eq!(command_history[0]["program"], "git commit");
        assert_eq!(command_history[0]["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_batch_mode_ignores_failed_tool_changed_file_evidence() {
        let mut ctx = setup_batch_ctx();
        let opts = BatchRunOpts {
            format: OutputFormat::Jsonl,
            ..Default::default()
        };
        let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);

        mode.on_user_input("task".to_string(), &mut ctx);
        mode.on_model_update(
            UiUpdate::StreamBlockStart {
                index: 0,
                block: StreamBlock::ToolCall {
                    id: "tool-1".to_string(),
                    name: "write_file".to_string(),
                    input: serde_json::json!({
                        "path": "src/main.rs",
                        "content": "fn main() {}\n"
                    }),
                    status: crate::state::ToolStatus::Executing,
                },
            },
            &mut ctx,
        );
        mode.on_model_update(
            UiUpdate::StreamBlockStart {
                index: 1,
                block: StreamBlock::ToolResult {
                    tool_call_id: "tool-1".to_string(),
                    output: "permission denied".to_string(),
                    is_error: true,
                },
            },
            &mut ctx,
        );
        mode.on_model_update(UiUpdate::StreamDelta("done".to_string()), &mut ctx);
        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

        let turn_line = mode.output_lines.first().expect("expected turn line");
        let turn_json: serde_json::Value = serde_json::from_str(turn_line).unwrap();
        let changed_files = turn_json["changed_files"]
            .as_array()
            .expect("changed_files must be present");
        assert!(
            !changed_files
                .iter()
                .filter_map(|value| value.as_str())
                .any(|path| path == "src/main.rs"),
            "failed tool calls must not be recorded as changed files"
        );
    }

    #[tokio::test]
    async fn test_batch_mode_marks_second_turn_attempt_as_max_turns_reached() {
        let mut ctx = setup_batch_ctx();
        let mut mode = BatchMode::new(
            "test-task".to_string(),
            BatchRunOpts {
                max_turns: Some(1),
                format: OutputFormat::Jsonl,
                ..Default::default()
            },
            None,
            None,
        );
        mode.current_turn = 1;

        mode.on_user_input("second turn".to_string(), &mut ctx);

        assert_eq!(mode.status, TaskStatus::MaxTurnsReached);
        assert!(mode.is_done());
    }

    #[tokio::test]
    async fn test_batch_mode_text_format_outputs_plain_response() {
        let output = capture_batch_text("echo hello", 3).await.unwrap();
        // Text format must not begin with a JSON envelope character.
        let trimmed = output.trim_start();
        // Empty output is acceptable for a mock that produces no text delta.
        if !trimmed.is_empty() {
            assert!(!trimmed.starts_with('{'));
        }
    }

    #[tokio::test]
    async fn test_build_batch_runtime_injects_memory_notes_into_system_prompt() {
        let _env_lock = crate::test_support::ENV_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        std::fs::write(&notes_path, "batch note\n").unwrap();

        let config = Config {
            model_token: None,
            model_name: "mock-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            model_url_skip_tls_check: false,
            working_dir: temp.path().to_path_buf(),
            model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
            model_protocol: crate::runtime::ModelProtocol::MessagesV1,
            tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                crate::runtime::ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            notes_path: Some(notes_path),
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };

        let (_runtime, ctx, _task_id) =
            build_batch_runtime(&config, "hello".to_string(), BatchRunOpts::default()).unwrap();
        let system_prompt = ctx.test_system_prompt().await;
        assert!(
            system_prompt.contains("<memory>\nbatch note\n</memory>"),
            "expected batch runtime client to include memory notes in system prompt"
        );
    }

    #[tokio::test]
    async fn test_build_batch_runtime_injects_project_instructions_into_system_prompt() {
        let _env_lock = crate::test_support::ENV_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("AGENTS.md"), "# batch instructions\n").unwrap();

        let config = Config {
            model_token: None,
            model_name: "mock-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            model_url_skip_tls_check: false,
            working_dir: temp.path().to_path_buf(),
            model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
            model_protocol: crate::runtime::ModelProtocol::MessagesV1,
            tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                crate::runtime::ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };

        let (_runtime, ctx, _task_id) =
            build_batch_runtime(&config, "hello".to_string(), BatchRunOpts::default()).unwrap();
        let system_prompt = ctx.test_system_prompt().await;
        assert!(system_prompt.contains("[project instructions: start]"));
        assert!(system_prompt.contains("# batch instructions"));
    }

    #[test]
    fn test_build_batch_runtime_uses_resumed_task_id() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            model_token: None,
            model_name: "mock-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            model_url_skip_tls_check: false,
            working_dir: temp.path().to_path_buf(),
            model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
            model_protocol: crate::runtime::ModelProtocol::MessagesV1,
            tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                crate::runtime::ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };
        let resume_state = TaskState::new("task-batch-resume".to_string());

        let (_runtime, _ctx, task_id) = build_batch_runtime(
            &config,
            "hello".to_string(),
            BatchRunOpts {
                resume_state: Some(resume_state),
                ..Default::default()
            },
        )
        .expect("build_batch_runtime should succeed");

        assert_eq!(task_id, "task-batch-resume");
    }

    #[test]
    fn test_batch_mode_jsonl_output_includes_instructions_path() {
        let opts = BatchRunOpts {
            format: OutputFormat::Jsonl,
            ..Default::default()
        };
        let mut mode = BatchMode::new(
            "test-task".to_string(),
            opts,
            None,
            Some("AGENTS.md".to_string()),
        );

        mode.status = TaskStatus::Running;
        mode.turn_in_progress = true;
        mode.current_response = "done".to_string();
        mode.finish_turn(TurnTokens {
            input: 8,
            output: 4,
            estimated: false,
            ..Default::default()
        });
        mode.status = TaskStatus::Completed;
        mode.append_summary();

        let turn_json: serde_json::Value =
            serde_json::from_str(mode.output_lines.first().expect("expected turn line")).unwrap();
        assert_eq!(turn_json["instructions_path"], "AGENTS.md");
        assert_eq!(turn_json["tokens"]["input"], 8);

        let summary_json: serde_json::Value =
            serde_json::from_str(mode.output_lines.last().expect("expected summary line")).unwrap();
        assert_eq!(summary_json["instructions_path"], "AGENTS.md");
    }

    #[test]
    fn test_batch_mode_summary_keeps_changed_files_from_prior_turns() {
        let opts = BatchRunOpts {
            format: OutputFormat::Jsonl,
            ..Default::default()
        };
        let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);

        mode.status = TaskStatus::Running;
        mode.turn_in_progress = true;
        mode.current_turn_changed_files
            .insert("src/first.rs".to_string());
        mode.finish_turn(TurnTokens::default());

        mode.reset_current_turn_state();
        mode.status = TaskStatus::Running;
        mode.turn_in_progress = true;
        mode.current_turn_changed_files
            .insert("src/second.rs".to_string());
        mode.finish_turn(TurnTokens::default());

        mode.status = TaskStatus::Completed;
        mode.append_summary();

        let summary_json: serde_json::Value =
            serde_json::from_str(mode.output_lines.last().expect("expected summary line")).unwrap();
        let changed_files = summary_json["changed_files"]
            .as_array()
            .expect("summary changed_files must be present");
        let changed_files = changed_files
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        assert!(changed_files.contains(&"src/first.rs"));
        assert!(changed_files.contains(&"src/second.rs"));
    }

    fn setup_batch_ctx() -> RuntimeContext {
        use crate::api::{mock_client::MockApiClient, ApiClient};
        use crate::state::ConversationManager;
        use std::collections::HashMap;
        use std::sync::Arc;

        let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        RuntimeContext::new(conversation, tx, CancellationToken::new())
    }

    #[tokio::test]
    async fn test_batch_mode_zero_max_turns_stops_before_first_turn() {
        // max_turns = Some(0): current_turn (0) >= max (0) fires immediately
        // on the first on_user_input call, before start_turn is reached.
        let result = run_batch_mode_with_opts(
            "keep going",
            BatchRunOpts {
                max_turns: Some(0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            result.status,
            TaskStatus::MaxTurnsReached,
            "max_turns=0 must produce MaxTurnsReached, not Completed"
        );
    }

    #[test]
    fn test_batch_mode_jsonl_output_includes_input_field() {
        let opts = BatchRunOpts {
            format: OutputFormat::Jsonl,
            ..Default::default()
        };
        let mut mode = BatchMode::new("test-task".to_string(), opts, None, None);
        mode.status = TaskStatus::Running;
        mode.turn_in_progress = true;
        mode.current_turn_input = "what is the answer".to_string();
        mode.current_response = "forty-two".to_string();
        mode.finish_turn(TurnTokens {
            input: 5,
            output: 7,
            estimated: true,
            ..Default::default()
        });

        let turn_json: serde_json::Value =
            serde_json::from_str(mode.output_lines.first().expect("expected turn line")).unwrap();
        assert_eq!(
            turn_json
                .get("input")
                .expect("input field must be present in JSONL"),
            "what is the answer",
            "input field must record the prompt text submitted for this turn"
        );
        assert_eq!(turn_json["tokens"]["output"], 7);
        assert_eq!(turn_json["tokens"]["estimated"], true);
    }

    #[tokio::test]
    async fn test_batch_mode_memory_clear_jsonl_records_input() {
        let result = run_batch_mode_with_opts(
            "/memory clear",
            BatchRunOpts {
                format: OutputFormat::Jsonl,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let turn_json: serde_json::Value =
            serde_json::from_str(result.output_lines.first().expect("expected turn line")).unwrap();
        assert_eq!(
            turn_json
                .get("input")
                .expect("input field must be present in JSONL"),
            "/memory clear",
            "batch-mode local turns must preserve the submitted input text"
        );
    }
}
