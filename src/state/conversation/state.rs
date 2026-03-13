use super::super::stream_block::StreamBlock;
use crate::api::ApiClient;
use crate::config::HookConfig;
use crate::tool_preview::ReadFileSnapshotCache;
use crate::tools::ToolOperator;
use crate::types::{ApiMessage, Content};
use crate::usage::TurnTokens;
use std::sync::Arc;
#[cfg(test)]
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::oneshot;

pub enum ConversationStreamUpdate {
    Delta(String),
    BlockStart { index: usize, block: StreamBlock },
    BlockDelta { index: usize, delta: String },
    BlockComplete { index: usize },
    ToolApprovalRequest(ToolApprovalRequest),
    TranscriptLine(String),
    CommandSessionStarted { session_id: u64, command: String },
    CommandSessionAttached { session_id: u64, pid: Option<u32> },
    CommandSessionFinished { session_id: u64 },
}

pub struct ToolApprovalRequest {
    pub tool_name: String,
    pub input_preview: String,
    pub response_tx: oneshot::Sender<bool>,
}

#[cfg(test)]
impl ToolApprovalRequest {
    pub fn test_stub() -> Self {
        let (response_tx, _response_rx) = oneshot::channel::<bool>();
        Self {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }
    }
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
    pub(super) hooks: Vec<HookConfig>,
    pub(super) api_messages: Vec<ApiMessage>,
    pub(super) current_turn_blocks: Vec<StreamBlock>,
    pub(super) last_turn_tokens: TurnTokens,
    pub(super) read_file_history_cache: ReadFileSnapshotCache,
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
        Self {
            client: Arc::new(client),
            tool_operator: operator,
            hooks,
            api_messages: Vec::new(),
            current_turn_blocks: Vec::new(),
            last_turn_tokens: TurnTokens::default(),
            read_file_history_cache: ReadFileSnapshotCache::default(),
            #[cfg(test)]
            mock_tool_operator_responses: None,
        }
    }

    #[cfg(test)]
    pub fn new_mock(client: ApiClient, tool_operator_responses: HashMap<String, String>) -> Self {
        Self {
            client: Arc::new(client),
            tool_operator: ToolOperator::new(std::path::PathBuf::from("/tmp")), // Dummy operator
            hooks: Vec::new(),
            api_messages: Vec::new(),
            current_turn_blocks: Vec::new(),
            last_turn_tokens: TurnTokens::default(),
            read_file_history_cache: ReadFileSnapshotCache::default(),
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
}
