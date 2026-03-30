use super::super::stream_block::StreamBlock;
use crate::api::ApiClient;
use crate::config::{HookConfig, HttpHookConfig};
use crate::mcp::McpRegistry;
use crate::runtime::ConfiguredSandbox;
use crate::tool_preview::ReadFileSnapshotCache;
use crate::tools::ToolOperator;
use crate::types::{ApiMessage, Content};
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
    pub(super) api_messages: Vec<ApiMessage>,
    pub(super) current_turn_blocks: Vec<StreamBlock>,
    pub(super) current_turn_applied_mutation: bool,
    pub(super) last_turn_tokens: TurnTokens,
    pub(super) read_file_history_cache: ReadFileSnapshotCache,
    pub(super) undo_stack: Vec<UndoCheckpoint>,
    pub(super) max_undo_checkpoints: usize,
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
            api_messages: Vec::new(),
            current_turn_blocks: Vec::new(),
            current_turn_applied_mutation: false,
            last_turn_tokens: TurnTokens::default(),
            read_file_history_cache: ReadFileSnapshotCache::default(),
            undo_stack: Vec::new(),
            max_undo_checkpoints: 20,
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

    pub fn with_mcp_registry(mut self, mcp_registry: Option<Arc<McpRegistry>>) -> Self {
        self.mcp_registry = mcp_registry;
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
            api_messages: Vec::new(),
            current_turn_blocks: Vec::new(),
            current_turn_applied_mutation: false,
            last_turn_tokens: TurnTokens::default(),
            read_file_history_cache: ReadFileSnapshotCache::default(),
            undo_stack: Vec::new(),
            max_undo_checkpoints: 20,
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
        self.current_turn_blocks.clear();
        self.current_turn_applied_mutation = false;
        self.last_turn_tokens = TurnTokens::default();
        self.read_file_history_cache = ReadFileSnapshotCache::default();
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
        if self.current_turn_applied_mutation {
            return true;
        }

        let mut mutating_tool_call_ids = std::collections::BTreeSet::new();
        for block in &self.current_turn_blocks {
            if let StreamBlock::ToolCall { id, name, .. } = block {
                if is_turn_mutation_tool(name) {
                    mutating_tool_call_ids.insert(id.as_str());
                }
            }
        }

        if mutating_tool_call_ids.is_empty() {
            return false;
        }

        self.current_turn_blocks.iter().any(|block| {
            matches!(
                block,
                StreamBlock::ToolResult {
                    tool_call_id,
                    is_error,
                    ..
                } if !*is_error && mutating_tool_call_ids.contains(tool_call_id.as_str())
            )
        })
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
