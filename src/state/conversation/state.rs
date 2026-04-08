use crate::api::ApiClient;
use crate::config::{HookConfig, HttpHookConfig, SearchConfig};
use crate::mcp::McpRegistry;
use crate::runtime::json_handoff::RuntimeEvent;
use crate::runtime::session_task::now_millis;
use crate::runtime::task_document::{TaskDocument, TaskDocumentCondenser, TaskMeta, TurnOutcome};
use crate::runtime::task_state::TaskStatus;
use crate::runtime::ConfiguredSandbox;
use crate::runtime::{ModelBackendKind, TaskMutationSummary};
use crate::tool_preview::ReadFileRollupCache;
use crate::tools::ToolOperator;
use crate::types::{ApiMessage, Content, StreamChunkMetadata};
use crate::usage::TurnTokens;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::oneshot;

/// A snapshot of a single file's content before a mutating tool call.
#[derive(Debug, Clone)]
pub struct UndoCheckpoint {
    /// The tool name that triggered this checkpoint (write_file, edit_file, etc.).
    pub tool_name: String,
    /// Absolute path of the affected file.
    pub path: PathBuf,
    /// Optional path that must be removed when the checkpoint is restored.
    pub cleanup_path: Option<PathBuf>,
    /// Previous file bytes (None if the file did not exist).
    pub previous_content: Option<Vec<u8>>,
}

use super::super::stream_block::StreamBlock;

pub enum ConversationStreamUpdate {
    Delta(String),
    BlockStart {
        index: usize,
        block: StreamBlock,
    },
    BlockDelta {
        index: usize,
        delta: String,
    },
    BlockComplete {
        index: usize,
    },
    ToolApprovalRequest(ToolApprovalRequest),
    TranscriptLine(String),
    ServerMetadata(Box<StreamChunkMetadata>),
    CommandSessionStarted {
        session_id: u64,
        command: String,
    },
    CommandSessionAttached {
        session_id: u64,
        pid: Option<u32>,
    },
    CommandSessionFinished {
        session_id: u64,
    },
    /// Emitted when conversation history is compacted (ADR-029 session persistence).
    ContextCompacted {
        messages_before: usize,
        messages_after: usize,
        summary: String,
    },
    /// A structured stream error (API error or SSE parse failure) that the
    /// runtime must surface to the UI.  ADR-021 Item 19.
    StreamError(String),
}

pub struct ToolApprovalRequest {
    pub tool_name: String,
    pub input_preview: String,
    pub response_tx: oneshot::Sender<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TurnToolPolicy {
    #[default]
    Default,
    TestsOnlyMutations,
}

pub struct ConversationManager {
    pub(super) client: Arc<ApiClient>,
    pub(super) tool_operator: ToolOperator,
    pub(super) mcp_registry: Option<Arc<McpRegistry>>,
    pub(super) sandbox: ConfiguredSandbox,
    pub(super) hooks: Vec<HookConfig>,
    pub(super) http_hooks: Vec<HttpHookConfig>,
    pub(super) search_config: SearchConfig,
    pub(super) api_messages: Vec<ApiMessage>,
    /// Canonical in-process task document. Owned by the condenser; replaces the
    /// former `current_turn_blocks` parallel live-turn model.
    pub(super) task_doc: Option<TaskDocument>,
    pub(super) condenser: TaskDocumentCondenser,
    /// Entry index in `active_turn.entries` where the current API round began.
    /// Reset at the start of every round so that per-round operations such as
    /// `promote_thinking_blocks_to_final_text` only touch the current round's
    /// entries rather than entries from earlier rounds of the same turn.
    pub(super) current_round_entry_start: usize,
    /// Number of stream `BlockStart` events emitted in the current API round,
    /// used by `upsert_turn_block` to pad with empty Thinking placeholders when
    /// the model sends a block at a non-zero index without prior blocks.
    pub(super) current_round_stream_block_count: usize,
    pub(super) last_turn_tokens: TurnTokens,
    pub(super) read_file_history_cache: ReadFileRollupCache,
    pub(super) undo_stack: Vec<UndoCheckpoint>,
    pub(super) max_undo_checkpoints: usize,
    pub(super) undo_enabled: bool,
    #[cfg(test)]
    pub(super) mock_tool_operator_responses: Option<Arc<Mutex<HashMap<String, String>>>>,
}

impl ConversationManager {
    pub fn new(client: ApiClient, operator: ToolOperator) -> Self {
        Self::new_with_hooks(client, operator, Vec::new())
    }

    pub fn new_with_hooks(
        client: ApiClient,
        operator: ToolOperator,
        hooks: Vec<HookConfig>,
    ) -> Self {
        Self::new_with_hooks_full(client, operator, hooks, Vec::new())
    }

    pub fn new_with_hooks_full(
        client: ApiClient,
        operator: ToolOperator,
        hooks: Vec<HookConfig>,
        http_hooks: Vec<HttpHookConfig>,
    ) -> Self {
        Self {
            client: Arc::new(client),
            tool_operator: operator,
            mcp_registry: None,
            sandbox: ConfiguredSandbox::default(),
            hooks,
            http_hooks,
            search_config: SearchConfig::default(),
            api_messages: Vec::new(),
            task_doc: None,
            condenser: TaskDocumentCondenser::new(),
            current_round_entry_start: 0,
            current_round_stream_block_count: 0,
            last_turn_tokens: TurnTokens::default(),
            read_file_history_cache: ReadFileRollupCache::default(),
            undo_stack: Vec::new(),
            max_undo_checkpoints: 20,
            undo_enabled: true,
            #[cfg(test)]
            mock_tool_operator_responses: None,
        }
    }

    pub fn new_with_hooks_and_mcp(
        client: ApiClient,
        operator: ToolOperator,
        hooks: Vec<HookConfig>,
        mcp_registry: Option<Arc<McpRegistry>>,
    ) -> Self {
        Self::new_with_hooks(client, operator, hooks).with_mcp_registry(mcp_registry)
    }

    pub fn new_with_hooks_and_sandbox(
        client: ApiClient,
        operator: ToolOperator,
        hooks: Vec<HookConfig>,
        sandbox: ConfiguredSandbox,
    ) -> Self {
        Self::new_with_hooks(client, operator, hooks).with_sandbox(sandbox)
    }

    pub fn with_sandbox(mut self, sandbox: ConfiguredSandbox) -> Self {
        self.sandbox = sandbox;
        self
    }

    pub fn with_max_undo_checkpoints(mut self, max: usize) -> Self {
        self.max_undo_checkpoints = max;
        self
    }

    pub fn with_undo_enabled(mut self, enabled: bool) -> Self {
        self.undo_enabled = enabled;
        self
    }

    pub fn is_undo_enabled(&self) -> bool {
        self.undo_enabled
    }

    pub fn with_mcp_registry(mut self, mcp_registry: Option<Arc<McpRegistry>>) -> Self {
        self.mcp_registry = mcp_registry;
        self
    }

    pub fn with_search_config(mut self, search_config: SearchConfig) -> Self {
        self.search_config = search_config;
        self
    }

    pub async fn shutdown_resources(&mut self) {
        if let Some(mcp_registry) = self.mcp_registry.take() {
            mcp_registry.shutdown().await;
        }
    }

    #[cfg(test)]
    pub fn new_mock(client: ApiClient, tool_operator_responses: HashMap<String, String>) -> Self {
        Self {
            client: Arc::new(client),
            tool_operator: ToolOperator::new(std::env::temp_dir()), // Cross-platform temp dir
            mcp_registry: None,
            sandbox: ConfiguredSandbox::default(),
            hooks: Vec::new(),
            http_hooks: Vec::new(),
            search_config: SearchConfig::default(),
            api_messages: Vec::new(),
            task_doc: None,
            condenser: TaskDocumentCondenser::new(),
            current_round_entry_start: 0,
            current_round_stream_block_count: 0,
            last_turn_tokens: TurnTokens::default(),
            read_file_history_cache: ReadFileRollupCache::default(),
            undo_stack: Vec::new(),
            max_undo_checkpoints: 20,
            undo_enabled: true,
            mock_tool_operator_responses: Some(Arc::new(Mutex::new(tool_operator_responses))),
        }
    }

    pub fn push_user_message(&mut self, input: String) {
        self.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(input),
        });
    }

    pub fn messages_for_api(&self) -> Vec<ApiMessage> {
        self.api_messages.clone()
    }

    pub fn clear_messages(&mut self) {
        self.api_messages.clear();
        self.task_doc = None;
        self.current_round_entry_start = 0;
        self.current_round_stream_block_count = 0;
        self.last_turn_tokens = TurnTokens::default();
        self.read_file_history_cache = ReadFileRollupCache::default();
    }

    pub fn model_name(&self) -> String {
        self.client.model_name()
    }

    pub fn set_model_name(&self, name: impl Into<String>) {
        self.client.set_model_name(name);
    }

    pub fn client(&self) -> Arc<ApiClient> {
        Arc::clone(&self.client)
    }

    pub fn take_last_turn_tokens(&mut self) -> TurnTokens {
        std::mem::take(&mut self.last_turn_tokens)
    }

    pub fn current_turn_has_successful_mutation(&self) -> bool {
        let Some(doc) = &self.task_doc else {
            return false;
        };
        // Prefer the active turn (mid-send guards); fall back to the most
        // recently completed turn so callers that check AFTER send_message
        // returns (e.g. edit-loop context) still see the correct value.
        use crate::runtime::task_document::TurnEntry;
        let entries: &[TurnEntry] = if let Some(active) = &doc.active_turn {
            &active.entries
        } else if let Some(last) = doc.completed_turns.last() {
            &last.entries
        } else {
            return false;
        };
        let mut mutating_tool_ids = std::collections::BTreeSet::new();
        for entry in entries {
            if let TurnEntry::ToolCall { id, name, .. } = entry {
                if is_turn_mutation_tool(name) {
                    mutating_tool_ids.insert(id.as_str());
                }
            }
        }
        if mutating_tool_ids.is_empty() {
            return false;
        }
        entries.iter().any(|entry| {
            matches!(
                entry,
                TurnEntry::ToolResult { tool_call_id, is_error, .. }
                    if !is_error && mutating_tool_ids.contains(tool_call_id.as_str())
            )
        })
    }

    /// Expose the canonical task document for read-only callers (local API,
    /// batch mode, snapshot adapters).
    pub fn task_doc(&self) -> Option<&TaskDocument> {
        self.task_doc.as_ref()
    }

    /// Ensure a `TaskDocument` exists. Called once at the start of every
    /// `send_message_with_policy` invocation. Creates a minimal document when
    /// no meta has been supplied by the caller.
    pub(super) fn ensure_task_doc(&mut self) {
        if self.task_doc.is_some() {
            return;
        }
        let model_backend = if self.client.is_local_endpoint() {
            ModelBackendKind::LocalRuntime
        } else {
            ModelBackendKind::ApiServer
        };
        let now = now_millis();
        let meta = TaskMeta {
            id: uuid::Uuid::new_v4().to_string(),
            status: TaskStatus::Ready,
            parent_task_id: None,
            agent_id: None,
            worktree_path: None,
            branch_name: None,
            instructions_path: None,
            model_name: self.client.model_name(),
            model_backend,
            model_url: String::new(),
            started_at_ms: Some(now),
            updated_at_ms: now,
            last_heartbeat_ms: None,
            active_grants: std::collections::HashMap::new(),
            next_step_id: 0,
        };
        self.task_doc = Some(self.condenser.begin_task(meta));
    }

    /// Begin a new turn inside the document. The caller must have called
    /// `ensure_task_doc` first.
    pub(super) fn begin_turn_doc(
        &mut self,
        input: String,
        tool_policy: crate::state::TurnToolPolicy,
    ) {
        if let Some(doc) = self.task_doc.as_mut() {
            let now = now_millis();
            self.condenser.begin_turn(doc, input, now, tool_policy);
        }
        self.current_round_stream_block_count = 0;
    }

    /// Commit the active turn and mark it completed.
    pub(super) fn finish_turn_doc(&mut self, outcome: TurnOutcome, tokens: TurnTokens) {
        if let Some(doc) = self.task_doc.as_mut() {
            let now = now_millis();
            self.condenser.finish_turn(doc, outcome, tokens, now);
        }
    }

    /// Apply one `RuntimeEvent` to the document.
    pub(super) fn apply_doc_event(&mut self, event: RuntimeEvent) -> TaskMutationSummary {
        if let Some(doc) = self.task_doc.as_mut() {
            self.condenser.apply_runtime_event(doc, event)
        } else {
            TaskMutationSummary::default()
        }
    }

    /// Record the current number of active-turn entries as the start point for
    /// the new API round. Called at the top of every round in the send loop so
    /// that per-round helpers only see blocks from the current round.
    pub(super) fn advance_round_start(&mut self) {
        self.current_round_entry_start = self
            .task_doc
            .as_ref()
            .and_then(|d| d.active_turn.as_ref())
            .map(|t| t.entries.len())
            .unwrap_or(0);
        self.current_round_stream_block_count = 0;
    }

    /// Number of entries in the active turn at or after `current_round_entry_start`.
    pub(super) fn current_round_entry_count(&self) -> usize {
        let total = self
            .task_doc
            .as_ref()
            .and_then(|d| d.active_turn.as_ref())
            .map(|t| t.entries.len())
            .unwrap_or(0);
        total.saturating_sub(self.current_round_entry_start)
    }

    /// Push a checkpoint onto the undo stack, evicting the oldest if at capacity.
    pub fn push_undo_checkpoint(&mut self, checkpoint: UndoCheckpoint) {
        if self.max_undo_checkpoints == 0 {
            return;
        }
        if self.undo_stack.len() >= self.max_undo_checkpoints {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(checkpoint);
    }

    /// Pop the most recent checkpoint from the undo stack.
    pub fn pop_undo_checkpoint(&mut self) -> Option<UndoCheckpoint> {
        self.undo_stack.pop()
    }

    /// Number of checkpoints currently buffered.
    pub fn undo_stack_len(&self) -> usize {
        self.undo_stack.len()
    }

    /// Capture a pre-mutation snapshot for undo. Returns `None` for non-mutating
    /// tools or when the target path cannot be resolved from the tool input.
    pub fn capture_undo_snapshot(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> Option<UndoCheckpoint> {
        let (path_str, cleanup_path_str) = match tool_name {
            "write_file" | "apply_patch" | "edit_file" => (
                ["path", "file_path", "file"]
                    .iter()
                    .find_map(|k| input.get(*k).and_then(|v| v.as_str())),
                None,
            ),
            "rename_file" => (
                ["old_path", "from", "source_path"]
                    .iter()
                    .find_map(|k| input.get(*k).and_then(|v| v.as_str())),
                ["new_path", "to", "dest_path"]
                    .iter()
                    .find_map(|k| input.get(*k).and_then(|v| v.as_str())),
            ),
            _ => return None,
        };
        let path_str = path_str?;
        let abs = self.tool_operator.working_dir().join(path_str);
        let cleanup_path =
            cleanup_path_str.map(|value| self.tool_operator.working_dir().join(value));
        let previous = std::fs::read(&abs).ok();
        Some(UndoCheckpoint {
            tool_name: tool_name.to_string(),
            path: abs,
            cleanup_path,
            previous_content: previous,
        })
    }
}

fn is_turn_mutation_tool(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "apply_patch" | "edit_file" | "rename_file"
    )
}
