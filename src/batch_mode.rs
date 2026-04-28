use crate::runtime::tokio::sync::mpsc;
use anyhow::Result;
use std::collections::{BTreeSet, HashMap, VecDeque};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::mcp::McpRegistry;
use crate::pulse_evidence::{
    SummaryRecord, TurnEvidenceRecord, command_evidence_from_tool_result,
    note_changed_files_from_tool_call,
};
use crate::runtime::{
    TaskState, UiUpdate,
    context::RuntimeContext,
    frontend::{FrontendAdapter, InputOccurrence},
    r#loop::Runtime,
    mode::RuntimeMode,
    project_instructions::{LoadResult, load_project_instructions},
    resolve_configured_sandbox,
    task_state::{CommandEvidence, TaskId, TaskStatus},
};
use crate::session_notes::{build_api_client_with_notes, clear_notes_file};
use crate::state::{ConversationManager, StreamBlock, ToolApprovalRequest};
use crate::tools::ToolOperator;
use crate::usage::PulseTokens;
use opentelemetry::KeyValue;
use std::path::PathBuf;
use tracing::Instrument;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jsonl,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoApproveScope {
    Once,

    Task,
}

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

pub struct BatchResult {
    pub status: TaskStatus,
    pub output_lines: Vec<String>,
    pub turn_count: usize,
    pub task_id: TaskId,
}

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
    current_turn_text_block_indices: BTreeSet<usize>,
    task_changed_files: BTreeSet<String>,
    current_turn_command_history: Vec<CommandEvidence>,
    current_turn_tokens: PulseTokens,
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
            current_turn_text_block_indices: BTreeSet::new(),
            task_changed_files: BTreeSet::new(),
            current_turn_command_history: Vec::new(),
            current_turn_tokens: PulseTokens::default(),
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

    fn approval_decision(&self) -> bool {
        self.auto_approve.is_some()
    }

    fn reset_current_turn_state(&mut self) {
        self.current_response.clear();
        self.current_turn_input.clear();
        self.current_turn_changed_files.clear();
        self.current_turn_text_block_indices.clear();
        self.current_turn_command_history.clear();
        self.current_turn_tokens = PulseTokens::default();
        self.pending_tool_calls.clear();
    }

    fn mark_max_turns_reached(&mut self) {
        self.status = TaskStatus::MaxTurnsReached;
        self.append_summary();
        self.done = true;
        self.turn_in_progress = false;
    }

    fn finish_turn(&mut self, tokens: PulseTokens) {
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
                    pulse: self.current_turn,
                    input: input_text,
                    response,
                    instructions_path: self.instructions_path.clone(),
                    changed_files,
                    command_history,
                    tokens: self.current_turn_tokens,
                };
                let line = serde_json::to_string(&record)
                    .expect("batch JSONL pulse record serialization must succeed");
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
        self.current_turn_tokens = PulseTokens::default();
        self.current_turn_text_block_indices.clear();
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
        self.finish_turn(PulseTokens::default());
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
        self.finish_turn(PulseTokens::default());
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

    fn sanitize_buffered_response_for_tool_call(&mut self) {
        // No-op: raw provider markup is no longer rewritten in the batch consumer.
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
        ctx.start_pulse(input);
    }

    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::TranscriptLine(_) => {}
            UiUpdate::StreamDelta(text) => {
                self.current_response.push_str(&text);
            }
            UiUpdate::PulseComplete => {
                self.finish_turn(ctx.session_tokens_rollup().last_pulse());
                if !self.done {
                    self.status = TaskStatus::Completed;
                    self.append_summary();
                    self.done = true;
                }
            }
            UiUpdate::Error(msg) => {
                self.current_response.push_str(&msg);
                self.finish_turn(PulseTokens::default());
                if !self.done {
                    self.status = TaskStatus::Failed;
                    self.append_summary();
                    self.done = true;
                }
            }
            UiUpdate::StreamBlockStart { index, block } => match block {
                StreamBlock::Thinking { content, .. } | StreamBlock::FinalText { content } => {
                    self.current_turn_text_block_indices.insert(index);
                    if !content.is_empty() {
                        self.current_response.push_str(&content);
                    }
                }
                StreamBlock::ToolCall {
                    id, name, input, ..
                } => {
                    self.sanitize_buffered_response_for_tool_call();
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
            },
            UiUpdate::StreamBlockDelta { index, delta } => {
                if self.current_turn_text_block_indices.contains(&index) {
                    self.current_response.push_str(&delta);
                }
            }
            UiUpdate::ToolCallArgumentsUpdated { .. } => {}
            UiUpdate::StreamBlockComplete { index } => {
                self.current_turn_text_block_indices.remove(&index);
            }
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest { response_tx, .. }) => {
                let approved = self.approval_decision();
                let _ = response_tx.send(approved);
            }
            UiUpdate::ServerMetadata(_) => {}
            UiUpdate::CommandSessionStarted { .. } => {}
            UiUpdate::CommandSessionAttached { .. } => {}
            UiUpdate::CommandSessionFinished { .. } => {}
            UiUpdate::EditLoopComplete { .. } => {}
            UiUpdate::ContextCompacted { .. } => {}
        }
    }

    fn is_pulse_in_progress(&self) -> bool {
        self.turn_in_progress
    }
}

pub struct BatchFrontend {
    pending: VecDeque<InputOccurrence>,
}

impl BatchFrontend {
    pub fn new(task: String) -> Self {
        let mut pending = VecDeque::new();
        pending.push_back(InputOccurrence::Text(task));
        Self { pending }
    }
}

impl FrontendAdapter<BatchMode> for BatchFrontend {
    fn poll_user_input(&mut self, mode: &BatchMode) -> Option<InputOccurrence> {
        if mode.is_done() {
            return None;
        }
        if mode.is_pulse_in_progress() {
            return None;
        }
        self.pending.pop_front()
    }

    fn render(&mut self, _mode: &BatchMode) {}

    fn should_quit(&self) -> bool {
        false
    }
}

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

    pub fn set_done(&mut self) {
        self.done = true;
    }
}

impl FrontendAdapter<BatchMode> for BatchFrontendQuit {
    fn poll_user_input(&mut self, mode: &BatchMode) -> Option<InputOccurrence> {
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
    crate::state::warm_codebase_index_with_config(&config.working_dir, &config.search);
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
        .with_search_config(config.search.clone())
        .with_sandbox(sandbox)
        .with_mcp_registry(mcp_registry)
        .with_compaction_config(config.compaction.clone());

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
    let ts = chrono::Utc::now().timestamp_millis();
    format!("batch-{}", ts)
}

pub async fn run_batch(task: String, opts: BatchRunOpts, config: &Config) -> Result<BatchResult> {
    let task_id = opts
        .resume_state
        .as_ref()
        .map(|state| state.id.clone())
        .unwrap_or_else(uuid_task_id);
    let batch_span = tracing::info_span!(
        "batch_run",
        task_id = %task_id,
        trace_id = tracing::field::Empty,
        otel_span_id = tracing::field::Empty,
        max_turns = opts.max_turns.unwrap_or_default(),
        output_format = ?opts.format,
    );
    crate::observability::attach_standard_baggage(
        &batch_span,
        "cli",
        "batch",
        None,
        Some(&task_id),
    );
    crate::observability::set_span_attributes(
        &batch_span,
        [
            KeyValue::new("task.id", task_id.clone()),
            KeyValue::new("vex.batch.output_format", format!("{:?}", opts.format)),
        ],
    );

    async move {
        let (sandbox, sandbox_warning) = resolve_configured_sandbox(&config.sandbox)?;
        let (instructions_text, instructions_path) = resolve_batch_project_instructions(config);
        let (client, notes_warning) = build_api_client_with_notes(config)?;
        let mcp_registry = McpRegistry::connect_all_blocking(&config.mcp_servers)?;
        crate::state::warm_codebase_index_with_config(&config.working_dir, &config.search);
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
        let conversation =
            ConversationManager::new_with_hooks(client, operator, config.hooks.clone())
                .with_search_config(config.search.clone())
                .with_sandbox(sandbox)
                .with_mcp_registry(mcp_registry)
                .with_compaction_config(config.compaction.clone());

        let (update_tx, mut update_rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
        let mut mode = BatchMode::new(
            task_id.clone(),
            opts,
            config.notes_path.clone(),
            instructions_path,
        );

        ctx.populate_local_server_info().await;

        mode.on_user_input(task, &mut ctx);

        if mode.is_done() {
            ctx.shutdown_resources().await;
            return Ok(BatchResult {
                status: mode.status,
                output_lines: mode.output_lines,
                turn_count: mode.current_turn,
                task_id,
            });
        }

        let spinner = if console::Term::stderr().is_term() {
            let pb = indicatif::ProgressBar::new_spinner();
            pb.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.magenta} {msg}")
                    .expect("valid template"),
            );
            pb.set_message("working…");
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            Some(pb)
        } else {
            None
        };

        while let Some(update) = update_rx.recv().await {
            mode.on_model_update(update, &mut ctx);
            if let Some(ref pb) = spinner {
                pb.set_message(format!("pulse {} · working…", mode.current_turn));
            }
            if mode.is_done() {
                break;
            }
        }

        if let Some(pb) = spinner {
            pb.finish_and_clear();
        }

        ctx.shutdown_resources().await;

        Ok(BatchResult {
            status: mode.status,
            output_lines: mode.output_lines,
            turn_count: mode.current_turn,
            task_id,
        })
    }
    .instrument(batch_span)
    .await
}

#[cfg(test)]
pub async fn run_batch_mode(task: &str, _max_turns: usize) -> Result<BatchResult> {
    use crate::api::{ApiClient, mock_client::MockApiClient};
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
    use crate::api::{ApiClient, mock_client::MockApiClient};
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
    use crate::api::{ApiClient, mock_client::MockApiClient};
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
mod tests;
