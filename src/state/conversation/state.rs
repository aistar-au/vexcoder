use crate::api::ApiClient;
use crate::config::{CompactionConfig, HookConfig, HttpHookConfig, SearchConfig};
use crate::mcp::McpRegistry;
use crate::runtime::ConfiguredSandbox;
use crate::runtime::json_handoff::RuntimeSignal;
use crate::runtime::session_task::now_millis;
use crate::runtime::task_document::{PulseOutcome, TaskDocument, TaskDocumentCondenser, TaskInfo};
use crate::runtime::task_state::TaskStatus;
use crate::runtime::{ModelBackendKind, TaskMutationSummary};
use crate::tool_preview::ReadFileRollupCache;
use crate::tools::ToolOperator;
use crate::types::{ApiMessage, Content, StreamChunkMetadata};
use crate::usage::PulseTokens;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct UndoCheckpoint {
    pub tool_name: String,
    pub path: PathBuf,
    pub cleanup_path: Option<PathBuf>,
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
    ToolCallArgumentsUpdated {
        tool_call_id: String,
        tool_name: Option<String>,
        arguments: serde_json::Value,
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
    ContextCompacted {
        messages_before: usize,
        messages_after: usize,
        summary: String,
    },
    StreamError(String),
}

pub struct ToolApprovalRequest {
    pub tool_name: String,
    pub input_preview: String,
    pub response_tx: oneshot::Sender<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PulseToolPolicy {
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
    pub(super) task_doc: Option<TaskDocument>,
    pub(super) condenser: TaskDocumentCondenser,
    pub(super) current_round_entry_start: usize,
    pub(super) current_round_stream_block_count: usize,
    pub(super) tool_call_started_at: HashMap<String, DateTime<Utc>>,
    pub(super) last_turn_tokens: PulseTokens,
    pub(super) read_file_history_cache: ReadFileRollupCache,
    pub(super) undo_stack: Vec<UndoCheckpoint>,
    pub(super) max_undo_checkpoints: usize,
    pub(super) undo_enabled: bool,
    pub(super) compaction_config: CompactionConfig,
    pub(super) notes_path: Option<PathBuf>,
    pub(super) max_memory_tokens: usize,
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
            tool_call_started_at: HashMap::new(),
            last_turn_tokens: PulseTokens::default(),
            read_file_history_cache: ReadFileRollupCache::default(),
            undo_stack: Vec::new(),
            max_undo_checkpoints: 20,
            undo_enabled: true,
            compaction_config: CompactionConfig::default(),
            notes_path: None,
            max_memory_tokens: 0,
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

    pub fn with_compaction_config(mut self, config: CompactionConfig) -> Self {
        self.compaction_config = config;
        self
    }

    pub fn with_memory_notes_config(
        mut self,
        notes_path: Option<PathBuf>,
        max_memory_tokens: usize,
    ) -> Self {
        if max_memory_tokens == 0
            && let Some(path) = notes_path.as_deref()
        {
            tracing::warn!(
                path = %path.display(),
                "memory notes path configured with zero token budget; note injection is disabled"
            );
        }
        self.notes_path = notes_path;
        self.max_memory_tokens = max_memory_tokens;
        self
    }

    pub async fn shutdown_resources(&mut self) {
        if let Some(mcp_registry) = self.mcp_registry.take() {
            mcp_registry.shutdown().await;
        }
    }

    #[cfg(test)]
    /// Test helper. Call `with_memory_notes_config(...)` on the returned
    /// manager when the test needs to exercise memory-note injection refresh.
    pub fn new_mock(client: ApiClient, tool_operator_responses: HashMap<String, String>) -> Self {
        Self {
            client: Arc::new(client),
            tool_operator: ToolOperator::new(std::env::temp_dir()),
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
            tool_call_started_at: HashMap::new(),
            last_turn_tokens: PulseTokens::default(),
            read_file_history_cache: ReadFileRollupCache::default(),
            undo_stack: Vec::new(),
            max_undo_checkpoints: 20,
            undo_enabled: true,
            compaction_config: CompactionConfig::default(),
            notes_path: None,
            max_memory_tokens: 0,
            mock_tool_operator_responses: Some(Arc::new(Mutex::new(tool_operator_responses))),
        }
    }

    pub(super) fn refresh_notes_content(&self) {
        let (notes_content, notes_warning) = crate::session_notes::resolve_notes_for_injection(
            self.notes_path.as_deref(),
            self.max_memory_tokens,
        );
        if let Some(warning) = notes_warning {
            tracing::debug!(warning = %warning, "memory notes refresh skipped");
        }
        self.client.set_notes_content(notes_content);
    }

    pub fn push_user_message(&mut self, input: String, cache_hint: Option<String>) {
        self.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(input),
            cache_hint,
        });
    }

    pub(super) fn shared_prefix_cache_hint(
        &self,
        shared_prefix_context: Option<&str>,
    ) -> Option<String> {
        shared_prefix_context
            .map(str::trim)
            .filter(|context| !context.is_empty())
            .and_then(|context| self.client.shared_prefix_fingerprint(context))
    }

    pub fn messages_for_api(&self) -> Vec<ApiMessage> {
        self.api_messages.clone()
    }

    pub fn clear_messages(&mut self) {
        self.api_messages.clear();
        self.task_doc = None;
        self.current_round_entry_start = 0;
        self.current_round_stream_block_count = 0;
        self.tool_call_started_at.clear();
        self.last_turn_tokens = PulseTokens::default();
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

    pub fn take_last_turn_tokens(&mut self) -> PulseTokens {
        std::mem::take(&mut self.last_turn_tokens)
    }

    pub fn current_turn_has_successful_mutation(&self) -> bool {
        let Some(doc) = &self.task_doc else {
            return false;
        };
        use crate::runtime::task_document::PulseEntry;
        let entries: &[PulseEntry] = if let Some(active) = &doc.active_pulse {
            &active.entries
        } else if let Some(last) = doc.completed_turns.last() {
            &last.entries
        } else {
            return false;
        };
        let mut mutating_tool_ids = std::collections::BTreeSet::new();
        for entry in entries {
            if let PulseEntry::ToolCall { id, name, .. } = entry
                && is_turn_mutation_tool(name)
            {
                mutating_tool_ids.insert(id.as_str());
            }
        }
        if mutating_tool_ids.is_empty() {
            return false;
        }
        entries.iter().any(|entry| {
            matches!(
                entry,
                PulseEntry::ToolResult { tool_call_id, is_error, .. }
                    if !is_error && mutating_tool_ids.contains(tool_call_id.as_str())
            )
        })
    }

    pub fn task_doc(&self) -> Option<&TaskDocument> {
        self.task_doc.as_ref()
    }

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
        let info = TaskInfo {
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
        self.task_doc = Some(self.condenser.begin_task(info));
    }

    pub(super) fn begin_turn_doc(
        &mut self,
        input: String,
        tool_policy: crate::state::PulseToolPolicy,
    ) {
        if let Some(doc) = self.task_doc.as_mut() {
            let now = now_millis();
            self.condenser.begin_turn(doc, input, now, tool_policy);
        }
        self.current_round_stream_block_count = 0;
        self.tool_call_started_at.clear();
    }

    pub(super) fn finish_turn_doc(&mut self, outcome: PulseOutcome, tokens: PulseTokens) {
        if let Some(doc) = self.task_doc.as_mut() {
            let now = now_millis();
            self.condenser.finish_turn(doc, outcome, tokens, now);
        }
        self.tool_call_started_at.clear();
    }

    pub(super) fn apply_doc_delta(&mut self, signal: RuntimeSignal) -> TaskMutationSummary {
        if let Some(doc) = self.task_doc.as_mut() {
            self.condenser.apply_runtime_signal(doc, signal)
        } else {
            TaskMutationSummary::default()
        }
    }

    pub(super) fn advance_round_start(&mut self) {
        self.current_round_entry_start = self
            .task_doc
            .as_ref()
            .and_then(|d| d.active_pulse.as_ref())
            .map(|t| t.entries.len())
            .unwrap_or(0);
        self.current_round_stream_block_count = 0;
    }

    pub fn push_undo_checkpoint(&mut self, checkpoint: UndoCheckpoint) {
        if self.max_undo_checkpoints == 0 {
            return;
        }
        if self.undo_stack.len() >= self.max_undo_checkpoints {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(checkpoint);
    }

    pub fn pop_undo_checkpoint(&mut self) -> Option<UndoCheckpoint> {
        self.undo_stack.pop()
    }

    pub fn undo_stack_len(&self) -> usize {
        self.undo_stack.len()
    }

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
