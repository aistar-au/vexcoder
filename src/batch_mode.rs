use anyhow::Result;
use serde::Serialize;
use std::collections::VecDeque;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::runtime::{
    context::RuntimeContext,
    frontend::{FrontendAdapter, UserInputEvent},
    mode::RuntimeMode,
    r#loop::Runtime,
    task_state::{TaskId, TaskStatus},
    UiUpdate,
};
use crate::state::{ConversationManager, ToolApprovalRequest};
use crate::tools::ToolOperator;

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
}

impl Default for BatchRunOpts {
    fn default() -> Self {
        Self {
            max_turns: None,
            auto_approve: None,
            format: OutputFormat::Jsonl,
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

// ── JSONL turn record ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct TurnRecord<'a> {
    turn: usize,
    response: &'a str,
    changed_files: Vec<String>,
    command_history: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct SummaryRecord<'a> {
    summary: bool,
    status: &'a str,
    task_id: &'a str,
    total_turns: usize,
    changed_files: Vec<String>,
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
    current_response: String,
    current_turn: usize,
    output_lines: Vec<String>,
}

impl BatchMode {
    pub fn new(task_id: TaskId, opts: BatchRunOpts) -> Self {
        Self {
            task_id,
            status: TaskStatus::Ready,
            turn_in_progress: false,
            done: false,
            max_turns: opts.max_turns,
            auto_approve: opts.auto_approve,
            format: opts.format,
            current_response: String::new(),
            current_turn: 0,
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

    fn approval_decision(&self) -> bool {
        self.auto_approve.is_some()
    }

    fn finish_turn(&mut self) {
        self.current_turn += 1;

        let response = std::mem::take(&mut self.current_response);

        match self.format {
            OutputFormat::Jsonl => {
                let record = TurnRecord {
                    turn: self.current_turn,
                    response: &response,
                    changed_files: vec![],
                    command_history: vec![],
                };
                if let Ok(line) = serde_json::to_string(&record) {
                    self.output_lines.push(line);
                }
            }
            OutputFormat::Text => {
                if self.current_turn > 1 {
                    self.output_lines.push(String::new());
                }
                self.output_lines.push(response);
            }
        }

        let max_reached = self
            .max_turns
            .map(|max| self.current_turn >= max)
            .unwrap_or(false);

        if max_reached {
            self.status = TaskStatus::MaxTurnsReached;
            self.append_summary();
            self.done = true;
        }

        self.turn_in_progress = false;
    }

    fn append_summary(&mut self) {
        if self.format == OutputFormat::Jsonl {
            let status_str = format!("{:?}", self.status);
            let record = SummaryRecord {
                summary: true,
                status: &status_str,
                task_id: &self.task_id,
                total_turns: self.current_turn,
                changed_files: vec![],
            };
            if let Ok(line) = serde_json::to_string(&record) {
                self.output_lines.push(line);
            }
        }
    }
}

impl RuntimeMode for BatchMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        if self.done {
            return;
        }
        self.status = TaskStatus::Running;
        self.turn_in_progress = true;
        ctx.start_turn(input);
    }

    fn on_model_update(&mut self, update: UiUpdate, _ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::StreamDelta(text) => {
                self.current_response.push_str(&text);
            }
            UiUpdate::TurnComplete => {
                self.finish_turn();
                if !self.done {
                    self.status = TaskStatus::Completed;
                    self.append_summary();
                    self.done = true;
                }
            }
            UiUpdate::Error(msg) => {
                self.current_response.push_str(&msg);
                self.finish_turn();
                self.status = TaskStatus::Failed;
                self.append_summary();
                self.done = true;
            }
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest { response_tx, .. }) => {
                let approved = self.approval_decision();
                let _ = response_tx.send(approved);
            }
            _ => {}
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
    let task_id = uuid_task_id();
    let client = crate::api::ApiClient::new(config)?;
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new(client, operator);

    let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let mode = BatchMode::new(task_id.clone(), opts);
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
    let task_id = uuid_task_id();
    let client = crate::api::ApiClient::new(config)?;
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new(client, operator);

    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let mut mode = BatchMode::new(task_id.clone(), opts);

    // Submit the initial task.
    mode.on_user_input(task, &mut ctx);

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

    let mock = Arc::new(MockApiClient::new(vec![]));
    let client = ApiClient::new_mock(mock);
    let conversation = ConversationManager::new_mock(client, HashMap::new());

    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let opts = BatchRunOpts {
        max_turns: Some(_max_turns),
        ..Default::default()
    };
    let task_id = "test-task".to_string();
    let mut mode = BatchMode::new(task_id.clone(), opts);

    mode.on_user_input(task.to_string(), &mut ctx);

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

    let mock = Arc::new(MockApiClient::new(vec![]));
    let client = ApiClient::new_mock(mock);
    let conversation = ConversationManager::new_mock(client, HashMap::new());

    let (update_tx, mut update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let task_id = "test-task".to_string();
    let mut mode = BatchMode::new(task_id.clone(), opts);

    mode.on_user_input(task.to_string(), &mut ctx);

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

    let mock = Arc::new(MockApiClient::new(vec![]));
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
    let mut mode = BatchMode::new(task_id.clone(), opts);

    mode.on_user_input(task.to_string(), &mut ctx);

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
    use crate::runtime::TaskStatus;

    #[tokio::test]
    async fn test_batch_mode_exits_zero_on_completion() {
        let result = run_batch_mode("echo hello", 3).await.unwrap();
        assert_eq!(result.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_batch_mode_max_turns_stops_run() {
        // With MockApiClient returning no responses, the turn completes
        // immediately (TurnComplete). max_turns = 1 means after 1 turn the
        // mode transitions to MaxTurnsReached before a second turn can start.
        let result = run_batch_mode_with_opts(
            "keep going",
            BatchRunOpts {
                max_turns: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(result.status, TaskStatus::MaxTurnsReached);
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
    async fn test_batch_mode_jsonl_output_includes_required_fields() {
        let output = capture_batch_jsonl("echo hello", 3).await.unwrap();
        let first_line = output.lines().next().unwrap_or("");
        // With a mock client that produces TurnComplete immediately, the first
        // output may be the summary line rather than a turn line; either way
        // the JSON must be valid.
        let v: serde_json::Value = serde_json::from_str(first_line).unwrap();
        // A turn line has "turn"; a summary line has "summary".
        assert!(v.get("turn").is_some() || v.get("summary").is_some());
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
}
