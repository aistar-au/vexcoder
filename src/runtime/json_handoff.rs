use super::UiUpdate;
use crate::api::stream::MAX_TOOL_CALL_INDEX;
use crate::api::stream::chat_compat::{
    ChatCompatChoice, ChatCompatChunk, ChatCompatDelta, ChatCompatPayload, ChatCompatToolCallDelta,
};
use crate::api::stream::provider::{ProviderDelta, ProviderStreamEvent};
use crate::runtime::AssistantPhase;
use crate::runtime::delta_accumulator::{AccumulationError, DeltaAccumulator};
use crate::state::{StreamBlock, ToolStatus};
use crate::turn_evidence::{
    SummaryRecord, TurnEvidenceRecord, command_evidence_from_tool_result,
    note_changed_files_from_tool_call,
};
use crate::types::{ApiUsage, ContentBlock, StreamChunkMetadata};
use crate::usage::TurnTokens;
use chrono::{DateTime, SecondsFormat, Timelike, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

mod derived;

use self::derived::{DerivedTurnState, empty_json_object, turn_tokens_from_usage};
pub use self::derived::{runtime_approval_request_event, token_usage_from_turn_tokens};

const SYNTHETIC_FINAL_TEXT_BLOCK_START: usize = 1_000_000;
const THINKING_DATA_TAG: &str = "thinking_data";

fn legacy_external_thinking_tag() -> &'static str {
    concat!("re", "dacted_thinking")
}

fn transitional_internal_thinking_tag() -> &'static str {
    concat!("su", "ppressed_thinking")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEnvelope {
    pub version: u16,
    pub task_id: String,
    pub turn: u32,
    pub seq: u64,
    pub event_id: String,
    pub emitted_at: String,
    pub source: RuntimeEnvelopeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<String>,
    pub event: RuntimeEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEnvelopeSource {
    Model,
    Runtime,
    UserRequest,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    TurnStart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<String>,
    },
    TranscriptLine {
        line: String,
    },
    TranscriptBlockStart {
        index: usize,
        block: StreamBlock,
    },
    TranscriptBlockDelta {
        index: usize,
        delta: String,
    },
    TranscriptBlockComplete {
        index: usize,
    },
    /// Update the status of an in-flight tool call without a full ToolResult.
    /// Emitted by the streaming layer when a tool transitions from Pending to
    /// Executing so the sole-writer condenser can track all status mutations.
    ToolCallStatusUpdated {
        tool_call_id: String,
        status: ToolStatus,
    },
    /// Promote a Thinking block to Final phase and mark it as no longer
    /// streaming.  Emitted at the end of an API round so the sole-writer
    /// condenser is the only code path that mutates block phase.
    TranscriptBlockPhaseUpdated {
        index: usize,
        phase: AssistantPhase,
        streaming: bool,
    },
    ToolCallStarted {
        tool_call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
        status: ToolStatus,
        started_at: String,
    },
    ToolCallArgumentsDelta {
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        delta: String,
        status: ToolStatus,
        #[serde(default)]
        invalid_json: bool,
    },
    ToolCallCompleted {
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        status: ToolStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<String>,
        completed_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        output: String,
    },
    ToolCallFailed {
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        status: ToolStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<String>,
        completed_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        output: String,
    },
    ApprovalRequest {
        capability: String,
        scope: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
    },
    ApprovalResolved {
        capability: String,
        scope: String,
        approved: bool,
    },
    ValidationResult {
        passed: bool,
        outputs: Vec<ValidationOutputEnvelope>,
    },
    ServerMetadata {
        metadata: Box<StreamChunkMetadata>,
    },
    UsageUpdated {
        usage: TokenUsageEnvelope,
    },
    TurnEnd {
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsageEnvelope>,
        changed_files: Vec<String>,
    },
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
    MaxTurnsReached {
        max_turns: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeRequest {
    SubmitInput {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        input: String,
    },
    Interrupt {
        request_id: String,
        task_id: String,
    },
    ApproveCapability {
        request_id: String,
        task_id: String,
        capability: String,
        scope: String,
    },
    DenyCapability {
        request_id: String,
        task_id: String,
        capability: String,
    },
}

impl RuntimeRequest {
    pub fn request_id(&self) -> &str {
        match self {
            RuntimeRequest::SubmitInput { request_id, .. }
            | RuntimeRequest::Interrupt { request_id, .. }
            | RuntimeRequest::ApproveCapability { request_id, .. }
            | RuntimeRequest::DenyCapability { request_id, .. } => request_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationOutputEnvelope {
    pub label: String,
    pub exit_code: i32,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenUsageEnvelope {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub estimated: bool,
    #[serde(default)]
    pub cache_creation_input: u64,
    #[serde(default)]
    pub cache_read_input: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnEndContext {
    pub usage: Option<TokenUsageEnvelope>,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DerivedBatchRecords {
    pub turns: Vec<TurnEvidenceRecord>,
    pub summary: Option<SummaryRecord>,
    pub max_turns_reached: bool,
}

pub type ToolCallId = String;

/// Generates a `tx_`-prefixed tool call ID suitable for use as a
/// [`ToolCallId`] anywhere in the runtime pipeline.
///
/// `counter` must be an [`AtomicU32`] owned or shared by the caller and
/// monotonically incremented per call. `entropy` is a 16-bit time- or
/// task-derived salt that reduces collision risk across counter resets.
///
/// The resulting format is `tx_{counter}_{entropy:04x}` — a decimal monotonic
/// counter followed by a 4-hex-digit entropy field — matching the pattern
/// `^tx_[0-9]+_[0-9a-f]{4}$` enforced by `schemas/runtime_envelope_v1.json`.
///
/// IDs are generated once in `src/runtime/json_handoff.rs` and passed
/// pre-generated to [`super::delta_accumulator::DeltaAccumulator`] — the
/// accumulator never creates its own IDs.
pub fn generate_tool_call_id(counter: &AtomicU32, entropy: u16) -> ToolCallId {
    let count = counter.fetch_add(1, Ordering::SeqCst).saturating_add(1);
    format!("tx_{}_{entropy:04x}", count)
}

#[derive(Debug, Clone)]
struct PendingToolCall {
    runtime_id: ToolCallId,
    name: String,
    arguments: serde_json::Value,
    start_event_id: String,
    started_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct PendingToolCallContext {
    name: String,
    start_event_id: String,
}

#[derive(Debug, Clone, Default)]
struct PendingChatCompatToolCall {
    id: String,
    name: String,
    pending_arguments: String,
    started: bool,
    stopped: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ProviderContentBlockCompat {
    Text {
        text: String,
        #[serde(default)]
        citations: Option<Vec<serde_json::Value>>,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default = "empty_json_object")]
        input: serde_json::Value,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    ThinkingData {
        data: String,
    },
    ServerToolUse {
        id: String,
        name: String,
        #[serde(default = "empty_json_object")]
        input: serde_json::Value,
    },
    WebSearchToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
    },
}

#[derive(Debug, Clone)]
enum ProviderContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult,
    Thinking {
        thinking: String,
    },
    ThinkingData {
        data: String,
    },
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    WebSearchToolResult,
}

#[derive(Debug, Clone)]
enum ProviderBlockDelta {
    Text(String),
    ToolArguments(String),
}

pub struct RuntimeEnvelopeNormalizer {
    task_id: String,
    turn: u32,
    next_seq: u64,
    pending_tool_calls: IndexMap<String, PendingToolCall>,
    pending_tool_call_contexts: HashMap<ToolCallId, PendingToolCallContext>,
    streaming_tool_call_blocks: HashMap<usize, ToolCallId>,
    block_sources: HashMap<usize, RuntimeEnvelopeSource>,
    turn_changed_files: BTreeSet<String>,
    tool_id_counter: AtomicU32,
    open_final_text_block: Option<usize>,
    next_synthetic_block_index: usize,
    delta_accumulator: Option<Arc<DeltaAccumulator>>,
    legacy_stream_turn_started: bool,
    legacy_open_blocks: BTreeSet<usize>,
    legacy_turn_tokens: TurnTokens,
    chat_compat_message_started: bool,
    chat_compat_tools: Vec<PendingChatCompatToolCall>,
}

impl RuntimeEnvelopeNormalizer {
    pub fn new(task_id: impl Into<String>) -> Self {
        Self::new_inner(task_id.into(), None)
    }

    pub fn new_with_delta_accumulator(
        task_id: impl Into<String>,
        delta_accumulator: Arc<DeltaAccumulator>,
    ) -> Self {
        Self::new_inner(task_id.into(), Some(delta_accumulator))
    }

    pub fn delta_accumulator(&self) -> Option<Arc<DeltaAccumulator>> {
        self.delta_accumulator.clone()
    }

    fn new_inner(task_id: String, delta_accumulator: Option<Arc<DeltaAccumulator>>) -> Self {
        Self {
            task_id,
            turn: 0,
            next_seq: 1,
            pending_tool_calls: IndexMap::new(),
            pending_tool_call_contexts: HashMap::new(),
            streaming_tool_call_blocks: HashMap::new(),
            block_sources: HashMap::new(),
            turn_changed_files: BTreeSet::new(),
            tool_id_counter: AtomicU32::new(0),
            open_final_text_block: None,
            next_synthetic_block_index: SYNTHETIC_FINAL_TEXT_BLOCK_START,
            delta_accumulator,
            legacy_stream_turn_started: false,
            legacy_open_blocks: BTreeSet::new(),
            legacy_turn_tokens: TurnTokens::default(),
            chat_compat_message_started: false,
            chat_compat_tools: Vec::new(),
        }
    }

    pub fn start_turn(&mut self, turn: u32, input: Option<String>) -> RuntimeEnvelope {
        self.turn = turn;
        // next_seq is intentionally NOT reset here: the seq counter is
        // process-lifetime monotonic so that event_ids remain unique across
        // turn boundaries within the same RuntimeEventEmitter instance.
        self.pending_tool_calls.clear();
        self.pending_tool_call_contexts.clear();
        self.streaming_tool_call_blocks.clear();
        self.block_sources.clear();
        self.turn_changed_files.clear();
        self.open_final_text_block = None;
        self.next_synthetic_block_index = SYNTHETIC_FINAL_TEXT_BLOCK_START;
        self.legacy_stream_turn_started = false;
        self.legacy_open_blocks.clear();
        self.legacy_turn_tokens = TurnTokens::default();
        self.chat_compat_message_started = false;
        self.chat_compat_tools.clear();

        self.next_envelope(RuntimeEvent::TurnStart { input })
    }

    pub fn normalize_content_block(&mut self, block: &ContentBlock) -> Vec<RuntimeEnvelope> {
        match block {
            ContentBlock::ToolUse {
                id, name, input, ..
            }
            | ContentBlock::ServerToolUse { id, name, input } => {
                let envelope = self.record_tool_call(id.clone(), name.clone(), input.clone());
                vec![envelope]
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => vec![self.record_tool_result(tool_use_id, content.clone(), *is_error)],
            ContentBlock::Text { .. }
            | ContentBlock::Thinking { .. }
            | ContentBlock::ThinkingData { .. }
            | ContentBlock::WebSearchToolResult { .. } => Vec::new(),
        }
    }

    pub fn normalize_stream_block(&mut self, block: &StreamBlock) -> Vec<RuntimeEnvelope> {
        match block {
            StreamBlock::ToolCall {
                id, name, input, ..
            } => vec![self.record_tool_call(id.clone(), name.clone(), input.clone())],
            StreamBlock::ToolResult {
                tool_call_id,
                output,
                is_error,
            } => vec![self.record_tool_result(tool_call_id, output.clone(), *is_error)],
            StreamBlock::Thinking { .. } | StreamBlock::FinalText { .. } => Vec::new(),
        }
    }

    pub fn normalize_tool_call_array(
        &mut self,
        tool_call_value: &serde_json::Value,
    ) -> Vec<RuntimeEnvelope> {
        match tool_call_value {
            serde_json::Value::Array(items) => items
                .iter()
                .filter_map(|item| self.normalize_grammar_tool_call(item))
                .collect(),
            serde_json::Value::Object(_) => self
                .normalize_grammar_tool_call(tool_call_value)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn normalize_ui_update(
        &mut self,
        update: &UiUpdate,
        turn_end: Option<TurnEndContext>,
    ) -> Vec<RuntimeEnvelope> {
        match update {
            UiUpdate::TranscriptLine(line) => {
                let mut envelopes = self.close_open_final_text_block();
                envelopes
                    .push(self.next_envelope(RuntimeEvent::TranscriptLine { line: line.clone() }));
                envelopes
            }
            UiUpdate::StreamDelta(text) => {
                let (index, block_start) = self.ensure_open_final_text_block();
                let mut envelopes = block_start.into_iter().collect::<Vec<_>>();
                envelopes.push(self.next_envelope_with_source(
                    RuntimeEvent::TranscriptBlockDelta {
                        index,
                        delta: text.clone(),
                    },
                    None,
                    None,
                    self.block_sources.get(&index).cloned(),
                ));
                envelopes
            }
            UiUpdate::StreamBlockStart { index, block } => {
                let mut envelopes = self.close_open_final_text_block();
                let source = source_for_stream_block(block);
                self.block_sources.insert(*index, source.clone());
                envelopes.push(self.next_envelope_with_source(
                    RuntimeEvent::TranscriptBlockStart {
                        index: *index,
                        block: block.clone(),
                    },
                    None,
                    None,
                    Some(source),
                ));
                envelopes.extend(self.normalize_stream_block(block));
                if let StreamBlock::ToolCall { id, .. } = block
                    && let Some(pending) = self.pending_tool_calls.get(id)
                {
                    self.streaming_tool_call_blocks
                        .insert(*index, pending.runtime_id.clone());
                }
                envelopes
            }
            UiUpdate::StreamBlockDelta { index, delta } => {
                let mut envelopes = Vec::new();
                // TranscriptBlockDelta is the accepted protocol event and is
                // emitted first so consumers see the raw stream before the
                // derived ToolCallArgumentsDelta that follows for tool blocks.
                envelopes.push(self.next_envelope_with_source(
                    RuntimeEvent::TranscriptBlockDelta {
                        index: *index,
                        delta: delta.clone(),
                    },
                    None,
                    None,
                    self.block_sources.get(index).cloned(),
                ));
                if let Some(runtime_id) = self.streaming_tool_call_blocks.get(index).cloned() {
                    envelopes.push(self.record_tool_call_arguments_delta(&runtime_id, delta));
                }
                envelopes
            }
            UiUpdate::StreamBlockComplete { index } => {
                if let Some(runtime_id) = self.streaming_tool_call_blocks.remove(index) {
                    self.finish_tool_accumulator(&runtime_id);
                }
                let source = self.block_sources.remove(index);
                vec![self.next_envelope_with_source(
                    RuntimeEvent::TranscriptBlockComplete { index: *index },
                    None,
                    None,
                    source,
                )]
            }
            UiUpdate::TurnComplete => self.complete_turn(turn_end.unwrap_or_default()),
            UiUpdate::Error(message) => self.emit_error(
                "runtime_error".to_string(),
                message.clone(),
                false,
                turn_end.unwrap_or_default(),
            ),
            UiUpdate::ToolApprovalRequest(request) => {
                let mut envelopes = self.close_open_final_text_block();
                envelopes.push(self.next_envelope(runtime_approval_request_event(request)));
                envelopes
            }
            UiUpdate::ServerMetadata(metadata) => {
                vec![self.next_envelope(RuntimeEvent::ServerMetadata {
                    metadata: Box::new((**metadata).clone()),
                })]
            }
            UiUpdate::CommandSessionStarted { .. }
            | UiUpdate::CommandSessionAttached { .. }
            | UiUpdate::CommandSessionFinished { .. }
            | UiUpdate::ToolCallArgumentsUpdated { .. }
            | UiUpdate::EditLoopComplete { .. }
            | UiUpdate::ContextCompacted { .. } => Vec::new(),
        }
    }

    pub fn normalize_runtime_request(&mut self, request: &RuntimeRequest) -> Vec<RuntimeEnvelope> {
        approval_resolution_event(request)
            .map(|event| {
                vec![self.next_envelope_with_context(
                    event,
                    Some(request.request_id().to_string()),
                    None,
                )]
            })
            .unwrap_or_default()
    }

    pub(crate) fn finish_legacy_stream_turn(&mut self) -> Vec<RuntimeEnvelope> {
        if !self.legacy_stream_turn_started {
            return Vec::new();
        }

        let mut envelopes = Vec::new();
        let open_blocks = std::mem::take(&mut self.legacy_open_blocks);
        for index in open_blocks {
            envelopes
                .extend(self.normalize_ui_update(&UiUpdate::StreamBlockComplete { index }, None));
        }

        let usage = token_usage_from_turn_tokens(std::mem::take(&mut self.legacy_turn_tokens));
        envelopes.extend(self.normalize_ui_update(
            &UiUpdate::TurnComplete,
            Some(TurnEndContext {
                usage,
                changed_files: Vec::new(),
            }),
        ));

        self.legacy_stream_turn_started = false;
        self.chat_compat_message_started = false;
        self.chat_compat_tools.clear();

        envelopes
    }

    pub(crate) fn normalize_provider_stream_event(
        &mut self,
        event: ProviderStreamEvent,
    ) -> Vec<RuntimeEnvelope> {
        match event {
            ProviderStreamEvent::MessageStart { message } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = message_start_metadata(message.metadata) {
                    envelopes.extend(self.emit_provider_server_metadata(metadata));
                }
                if let Some(usage) = message.usage {
                    envelopes.extend(self.emit_provider_usage_update(usage));
                }
                envelopes
            }
            ProviderStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => decode_provider_content_block(content_block)
                .map(|content_block| {
                    self.normalize_provider_content_block_start(index, content_block)
                })
                .unwrap_or_default(),
            ProviderStreamEvent::ContentBlockDelta { index, delta } => provider_block_delta(&delta)
                .map(|delta| self.normalize_provider_block_delta(index, delta))
                .unwrap_or_default(),
            ProviderStreamEvent::ContentBlockStop { index } => {
                self.normalize_provider_content_block_stop(index)
            }
            ProviderStreamEvent::MessageDelta { delta, usage } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = delta.metadata {
                    envelopes.extend(self.emit_provider_server_metadata(metadata));
                }
                if let Some(usage) = usage {
                    envelopes.extend(self.emit_provider_usage_update(usage));
                }
                envelopes
            }
            ProviderStreamEvent::MessageStop
            | ProviderStreamEvent::Ping
            | ProviderStreamEvent::Unknown => Vec::new(),
            ProviderStreamEvent::Error { error } => vec![self.emit_event(RuntimeEvent::Error {
                code: error.error_type,
                message: error.message,
                recoverable: true,
            })],
        }
    }

    pub(crate) fn normalize_chat_compat_payload(
        &mut self,
        payload: ChatCompatPayload,
    ) -> Vec<RuntimeEnvelope> {
        match payload {
            ChatCompatPayload::Done => {
                let envelopes = self.close_chat_compat_tool_blocks();
                self.chat_compat_message_started = false;
                self.chat_compat_tools.clear();
                envelopes
            }
            ChatCompatPayload::Chunk(chunk) => self.normalize_chat_compat_chunk(*chunk),
        }
    }

    pub(crate) fn emit_legacy_stream_error(
        &mut self,
        code: String,
        message: String,
    ) -> RuntimeEnvelope {
        let _ = self.ensure_legacy_stream_turn_started();
        self.emit_event(RuntimeEvent::Error {
            code,
            message,
            recoverable: true,
        })
    }

    pub fn emit_error(
        &mut self,
        code: String,
        message: String,
        recoverable: bool,
        turn_end: TurnEndContext,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.close_open_final_text_block();
        envelopes.push(self.next_envelope(RuntimeEvent::Error {
            code,
            message,
            recoverable,
        }));

        if !recoverable {
            let changed_files = self.resolve_changed_files(turn_end.changed_files);
            self.finish_pending_tool_accumulations();
            self.pending_tool_calls.clear();
            self.pending_tool_call_contexts.clear();
            self.streaming_tool_call_blocks.clear();
            self.block_sources.clear();
            envelopes.push(self.next_envelope(RuntimeEvent::TurnEnd {
                status: "failed".to_string(),
                usage: turn_end.usage,
                changed_files,
            }));
        }

        envelopes
    }

    pub fn emit_max_turns_reached(
        &mut self,
        max_turns: u32,
        turn_end: TurnEndContext,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.close_open_final_text_block();
        let changed_files = self.resolve_changed_files(turn_end.changed_files);
        self.finish_pending_tool_accumulations();
        self.pending_tool_calls.clear();
        self.pending_tool_call_contexts.clear();
        self.streaming_tool_call_blocks.clear();
        self.block_sources.clear();
        envelopes.push(self.next_envelope(RuntimeEvent::MaxTurnsReached { max_turns }));
        envelopes.push(self.next_envelope(RuntimeEvent::TurnEnd {
            status: "failed".to_string(),
            usage: turn_end.usage,
            changed_files,
        }));
        envelopes
    }

    pub fn emit_cancelled(&mut self, turn_end: TurnEndContext) -> Vec<RuntimeEnvelope> {
        self.finish_turn_with_status("cancelled", turn_end)
    }

    pub fn emit_event(&mut self, event: RuntimeEvent) -> RuntimeEnvelope {
        self.next_envelope(event)
    }

    fn ensure_legacy_stream_turn_started(&mut self) -> Vec<RuntimeEnvelope> {
        if self.legacy_stream_turn_started {
            return Vec::new();
        }

        let envelope = self.start_turn(1, None);
        self.legacy_stream_turn_started = true;
        vec![envelope]
    }

    fn normalize_provider_content_block_start(
        &mut self,
        index: usize,
        content_block: ProviderContentBlock,
    ) -> Vec<RuntimeEnvelope> {
        let Some((block, initial_delta)) = provider_stream_block(&content_block) else {
            return Vec::new();
        };

        let mut envelopes = self.ensure_legacy_stream_turn_started();
        self.legacy_open_blocks.insert(index);
        envelopes
            .extend(self.normalize_ui_update(&UiUpdate::StreamBlockStart { index, block }, None));
        if let Some(delta) = initial_delta.filter(|delta| !delta.is_empty()) {
            envelopes.extend(
                self.normalize_ui_update(&UiUpdate::StreamBlockDelta { index, delta }, None),
            );
        }
        envelopes
    }

    fn normalize_provider_block_delta(
        &mut self,
        index: usize,
        block_delta: ProviderBlockDelta,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.ensure_legacy_stream_turn_started();
        if matches!(block_delta, ProviderBlockDelta::Text(_))
            && !self.legacy_open_blocks.contains(&index)
        {
            self.legacy_open_blocks.insert(index);
            envelopes.extend(self.normalize_ui_update(
                &UiUpdate::StreamBlockStart {
                    index,
                    block: StreamBlock::Thinking {
                        content: String::new(),
                        collapsed: false,
                    },
                },
                None,
            ));
        }

        let delta = match block_delta {
            ProviderBlockDelta::Text(delta) | ProviderBlockDelta::ToolArguments(delta) => delta,
        };
        envelopes
            .extend(self.normalize_ui_update(&UiUpdate::StreamBlockDelta { index, delta }, None));
        envelopes
    }

    fn normalize_provider_content_block_stop(&mut self, index: usize) -> Vec<RuntimeEnvelope> {
        if !self.legacy_open_blocks.remove(&index) {
            return Vec::new();
        }

        self.normalize_ui_update(&UiUpdate::StreamBlockComplete { index }, None)
    }

    fn emit_provider_server_metadata(
        &mut self,
        metadata: StreamChunkMetadata,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.ensure_legacy_stream_turn_started();
        envelopes
            .extend(self.normalize_ui_update(&UiUpdate::ServerMetadata(Box::new(metadata)), None));
        envelopes
    }

    fn emit_provider_usage_update(&mut self, usage: ApiUsage) -> Vec<RuntimeEnvelope> {
        accumulate_turn_tokens(&mut self.legacy_turn_tokens, &usage);

        let mut envelopes = self.ensure_legacy_stream_turn_started();
        if let Some(usage) = token_usage_from_api_usage(&usage) {
            envelopes.push(self.emit_event(RuntimeEvent::UsageUpdated { usage }));
        }
        envelopes
    }

    fn normalize_chat_compat_chunk(&mut self, mut chunk: ChatCompatChunk) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.emit_chat_compat_message_start(&chunk);

        if let Some(mut usage) = chunk.usage.take() {
            if usage.service_tier.is_none() {
                usage.service_tier = chunk.service_tier.clone();
            }
            if let Some(metadata) = chat_compat_metadata(&chunk, None, None) {
                envelopes.extend(self.emit_provider_server_metadata(metadata));
            }
            envelopes.extend(self.emit_provider_usage_update(usage));
        }

        if chunk.choices.is_empty() {
            return envelopes;
        }

        for choice in chunk.choices.clone() {
            let ChatCompatChoice {
                index: choice_index,
                delta,
                finish_reason,
                logprobs,
            } = choice;
            let ChatCompatDelta {
                role: _,
                content,
                refusal,
                tool_calls,
            } = delta;

            if let Some(content) = content {
                envelopes.extend(
                    self.normalize_provider_block_delta(0, ProviderBlockDelta::Text(content)),
                );
            }

            if let Some(tool_calls) = tool_calls {
                for tool_call in tool_calls {
                    envelopes.extend(self.apply_chat_compat_tool_delta(choice_index, tool_call));
                }
            }

            let has_logprobs = logprobs.is_some();
            let metadata = chat_compat_metadata(&chunk, choice_index, logprobs);
            let has_server_progress = metadata
                .as_ref()
                .is_some_and(|md| md.prompt_progress.is_some() || md.timings.is_some());

            if (refusal.is_some() || finish_reason.is_some() || has_logprobs || has_server_progress)
                && let Some(metadata) = metadata
            {
                envelopes.extend(self.emit_provider_server_metadata(metadata));
            }

            if finish_reason.is_some() {
                envelopes.extend(self.close_chat_compat_tool_blocks());
            }
        }

        envelopes
    }

    fn emit_chat_compat_message_start(&mut self, chunk: &ChatCompatChunk) -> Vec<RuntimeEnvelope> {
        if self.chat_compat_message_started {
            return Vec::new();
        }

        let role = chunk
            .choices
            .iter()
            .find_map(|choice| choice.delta.role.clone());
        let metadata = chat_compat_metadata(chunk, None, None);
        let has_message_fields =
            chunk.id.is_some() || chunk.model.is_some() || role.is_some() || metadata.is_some();
        if !has_message_fields {
            return Vec::new();
        }

        self.chat_compat_message_started = true;
        message_start_metadata(metadata)
            .map(|metadata| self.emit_provider_server_metadata(metadata))
            .unwrap_or_default()
    }

    fn apply_chat_compat_tool_delta(
        &mut self,
        choice_index: Option<usize>,
        tool_call: ChatCompatToolCallDelta,
    ) -> Vec<RuntimeEnvelope> {
        let raw_index = tool_call.index.unwrap_or(0).min(MAX_TOOL_CALL_INDEX);
        let block_index = raw_index.saturating_add(1);
        self.ensure_chat_compat_tool_state(block_index);

        let mut start_block = None;
        let mut partial_json = None;
        {
            let state = &mut self.chat_compat_tools[block_index];
            let _ = (choice_index, tool_call.call_type);

            if let Some(id) = tool_call.id
                && !id.is_empty()
            {
                state.id = id;
            }
            if let Some(function) = tool_call.function {
                if let Some(name) = function.name
                    && !name.is_empty()
                {
                    state.name = name;
                }
                if let Some(arguments) = function.arguments {
                    state.pending_arguments.push_str(&arguments);
                }
            }

            if !state.started && !state.name.is_empty() {
                let id = if state.id.is_empty() {
                    format!("toolu_chat_compat_{block_index}")
                } else {
                    state.id.clone()
                };
                start_block = Some(ProviderContentBlock::ToolUse {
                    id,
                    name: state.name.clone(),
                    input: empty_json_object(),
                });
                state.started = true;
            }

            if state.started && !state.pending_arguments.is_empty() {
                partial_json = Some(std::mem::take(&mut state.pending_arguments));
            }
        }

        let mut envelopes = Vec::new();
        if let Some(content_block) = start_block {
            envelopes
                .extend(self.normalize_provider_content_block_start(block_index, content_block));
        }
        if let Some(partial_json) = partial_json {
            envelopes.extend(self.normalize_provider_block_delta(
                block_index,
                ProviderBlockDelta::ToolArguments(partial_json),
            ));
        }
        envelopes
    }

    fn ensure_chat_compat_tool_state(&mut self, index: usize) {
        let required = index.saturating_add(1);
        if self.chat_compat_tools.len() < required {
            self.chat_compat_tools
                .resize_with(required, PendingChatCompatToolCall::default);
        }
    }

    fn close_chat_compat_tool_blocks(&mut self) -> Vec<RuntimeEnvelope> {
        let mut to_close = Vec::new();
        for (index, state) in self.chat_compat_tools.iter_mut().enumerate() {
            if index == 0 {
                continue;
            }
            if state.started && !state.stopped {
                state.stopped = true;
                to_close.push(index);
            }
        }

        let mut envelopes = Vec::new();
        for index in to_close {
            envelopes.extend(self.normalize_provider_content_block_stop(index));
        }
        envelopes
    }

    fn complete_turn(&mut self, turn_end: TurnEndContext) -> Vec<RuntimeEnvelope> {
        self.finish_turn_with_status("completed", turn_end)
    }

    fn finish_turn_with_status(
        &mut self,
        status: &str,
        turn_end: TurnEndContext,
    ) -> Vec<RuntimeEnvelope> {
        let changed_files = self.resolve_changed_files(turn_end.changed_files);
        self.finish_pending_tool_accumulations();
        let mut envelopes = self.close_open_final_text_block();
        self.pending_tool_calls.clear();
        self.pending_tool_call_contexts.clear();
        self.streaming_tool_call_blocks.clear();
        self.block_sources.clear();
        envelopes.push(self.next_envelope(RuntimeEvent::TurnEnd {
            status: status.to_string(),
            usage: turn_end.usage,
            changed_files,
        }));
        envelopes
    }

    fn ensure_open_final_text_block(&mut self) -> (usize, Option<RuntimeEnvelope>) {
        if let Some(index) = self.open_final_text_block {
            return (index, None);
        }

        let index = self.next_synthetic_block_index;
        self.next_synthetic_block_index = self.next_synthetic_block_index.saturating_add(1);
        self.open_final_text_block = Some(index);
        self.block_sources
            .insert(index, RuntimeEnvelopeSource::Model);

        (
            index,
            Some(self.next_envelope_with_source(
                RuntimeEvent::TranscriptBlockStart {
                    index,
                    block: StreamBlock::FinalText {
                        content: String::new(),
                    },
                },
                None,
                None,
                Some(RuntimeEnvelopeSource::Model),
            )),
        )
    }

    fn close_open_final_text_block(&mut self) -> Vec<RuntimeEnvelope> {
        let Some(index) = self.open_final_text_block.take() else {
            return Vec::new();
        };

        let source = self.block_sources.remove(&index);

        vec![self.next_envelope_with_source(
            RuntimeEvent::TranscriptBlockComplete { index },
            None,
            None,
            source,
        )]
    }

    fn normalize_grammar_tool_call(
        &mut self,
        tool_call_value: &serde_json::Value,
    ) -> Option<RuntimeEnvelope> {
        let name = tool_call_value.get("name")?.as_str()?.to_string();
        let arguments = tool_call_value
            .get("arguments")
            .cloned()
            .unwrap_or_else(empty_json_object);

        let runtime_id = self.generate_tool_call_id();
        let started_at = Utc::now();
        let envelope = self.next_envelope(RuntimeEvent::ToolCallStarted {
            tool_call_id: runtime_id.clone(),
            tool_name: name.clone(),
            arguments: arguments.clone(),
            status: ToolStatus::Pending,
            started_at: timestamp_string(started_at),
        });
        self.pending_tool_calls.insert(
            runtime_id.clone(),
            PendingToolCall {
                runtime_id: runtime_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
                start_event_id: envelope.event_id.clone(),
                started_at,
            },
        );
        self.pending_tool_call_contexts.insert(
            runtime_id.clone(),
            PendingToolCallContext {
                name: name.clone(),
                start_event_id: envelope.event_id.clone(),
            },
        );
        self.start_tool_accumulator(&runtime_id, &name);
        self.seed_tool_accumulator(&runtime_id, &arguments);

        Some(envelope)
    }

    fn record_tool_call(
        &mut self,
        source_id: String,
        name: String,
        arguments: serde_json::Value,
    ) -> RuntimeEnvelope {
        let runtime_id = self.generate_tool_call_id();
        let started_at = Utc::now();
        let envelope = self.next_envelope(RuntimeEvent::ToolCallStarted {
            tool_call_id: runtime_id.clone(),
            tool_name: name.clone(),
            arguments: arguments.clone(),
            status: ToolStatus::Pending,
            started_at: timestamp_string(started_at),
        });
        self.pending_tool_calls.insert(
            source_id,
            PendingToolCall {
                runtime_id: runtime_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
                start_event_id: envelope.event_id.clone(),
                started_at,
            },
        );
        self.pending_tool_call_contexts.insert(
            runtime_id.clone(),
            PendingToolCallContext {
                name: name.clone(),
                start_event_id: envelope.event_id.clone(),
            },
        );
        self.start_tool_accumulator(&runtime_id, &name);
        self.seed_tool_accumulator(&runtime_id, &arguments);

        envelope
    }

    fn record_tool_result(
        &mut self,
        source_id: &str,
        output: String,
        is_error: bool,
    ) -> RuntimeEnvelope {
        let completed_at = Utc::now();
        let completed_at_value = timestamp_string(completed_at);
        if let Some(pending) = self.pending_tool_calls.shift_remove(source_id) {
            self.pending_tool_call_contexts.remove(&pending.runtime_id);
            self.finish_tool_accumulator(&pending.runtime_id);
            if !is_error {
                note_changed_files_from_tool_call(
                    &mut self.turn_changed_files,
                    &pending.name,
                    &pending.arguments,
                );
            }
            let duration_ms = completed_at
                .signed_duration_since(pending.started_at)
                .num_milliseconds()
                .max(0) as u64;
            let event = if is_error {
                RuntimeEvent::ToolCallFailed {
                    tool_call_id: pending.runtime_id,
                    tool_name: Some(pending.name),
                    status: ToolStatus::Error,
                    started_at: Some(timestamp_string(pending.started_at)),
                    completed_at: completed_at_value,
                    duration_ms: Some(duration_ms),
                    output,
                }
            } else {
                RuntimeEvent::ToolCallCompleted {
                    tool_call_id: pending.runtime_id,
                    tool_name: Some(pending.name),
                    status: ToolStatus::Complete,
                    started_at: Some(timestamp_string(pending.started_at)),
                    completed_at: completed_at_value,
                    duration_ms: Some(duration_ms),
                    output,
                }
            };
            return self.next_envelope_with_context(event, None, Some(pending.start_event_id));
        }

        self.next_envelope(if is_error {
            RuntimeEvent::ToolCallFailed {
                tool_call_id: source_id.to_string(),
                tool_name: None,
                status: ToolStatus::Error,
                started_at: None,
                completed_at: completed_at_value,
                duration_ms: None,
                output,
            }
        } else {
            RuntimeEvent::ToolCallCompleted {
                tool_call_id: source_id.to_string(),
                tool_name: None,
                status: ToolStatus::Complete,
                started_at: None,
                completed_at: completed_at_value,
                duration_ms: None,
                output,
            }
        })
    }

    fn record_tool_call_arguments_delta(
        &mut self,
        tool_call_id: &ToolCallId,
        delta: &str,
    ) -> RuntimeEnvelope {
        let (tool_name, parent_event_id) = self
            .pending_tool_call_contexts
            .get(tool_call_id)
            .map(|pending| {
                (
                    Some(pending.name.clone()),
                    Some(pending.start_event_id.clone()),
                )
            })
            .unwrap_or((None, None));
        let invalid_json = matches!(
            self.accumulate_tool_delta(tool_call_id, delta),
            Some(AccumulationError::MalformedPartial)
        );

        self.next_envelope_with_context(
            RuntimeEvent::ToolCallArgumentsDelta {
                tool_call_id: tool_call_id.clone(),
                tool_name,
                delta: delta.to_string(),
                status: ToolStatus::Pending,
                invalid_json,
            },
            None,
            parent_event_id,
        )
    }

    fn resolve_changed_files(&self, changed_files: Vec<String>) -> Vec<String> {
        if changed_files.is_empty() {
            self.turn_changed_files.iter().cloned().collect()
        } else {
            changed_files
        }
    }

    fn start_tool_accumulator(&self, tool_call_id: &ToolCallId, name: &str) {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return;
        };

        if let Err(error) = delta_accumulator.start_tool(
            tool_call_id.clone(),
            self.task_id.clone(),
            name.to_string(),
        ) {
            tracing::debug!(%error, tool_call_id = %tool_call_id, "failed to start tool delta accumulation");
        }
    }

    fn seed_tool_accumulator(&self, tool_call_id: &ToolCallId, arguments: &serde_json::Value) {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return;
        };

        let Ok(serialized) = serde_json::to_string(arguments) else {
            return;
        };
        if serialized == "{}" {
            return;
        }

        if let Err(error) = delta_accumulator.accumulate(tool_call_id, &serialized) {
            tracing::debug!(%error, tool_call_id = %tool_call_id, "failed to seed tool delta accumulation");
        }
    }

    fn accumulate_tool_delta(
        &self,
        tool_call_id: &ToolCallId,
        delta: &str,
    ) -> Option<AccumulationError> {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return None;
        };

        if let Err(error) = delta_accumulator.accumulate(tool_call_id, delta) {
            tracing::debug!(%error, tool_call_id = %tool_call_id, "failed to accumulate tool delta");
            return Some(error);
        }

        None
    }

    fn finish_tool_accumulator(&self, tool_call_id: &ToolCallId) {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return;
        };

        delta_accumulator.finish(tool_call_id);
    }

    fn finish_pending_tool_accumulations(&self) {
        for pending in self.pending_tool_calls.values() {
            self.finish_tool_accumulator(&pending.runtime_id);
        }
    }

    fn generate_tool_call_id(&mut self) -> ToolCallId {
        let now = Utc::now();
        let entropy = ((now.nanosecond() as u128
            ^ self.tool_id_counter.load(Ordering::SeqCst) as u128)
            & 0xffff) as u16;
        generate_tool_call_id(&self.tool_id_counter, entropy)
    }

    fn next_envelope(&mut self, event: RuntimeEvent) -> RuntimeEnvelope {
        self.next_envelope_with_context(event, None, None)
    }

    fn next_envelope_with_context(
        &mut self,
        event: RuntimeEvent,
        request_id: Option<String>,
        parent_event_id: Option<String>,
    ) -> RuntimeEnvelope {
        self.next_envelope_with_source(event, request_id, parent_event_id, None)
    }

    fn next_envelope_with_source(
        &mut self,
        event: RuntimeEvent,
        request_id: Option<String>,
        parent_event_id: Option<String>,
        source: Option<RuntimeEnvelopeSource>,
    ) -> RuntimeEnvelope {
        let seq = self.next_seq;
        let envelope = RuntimeEnvelope {
            version: 1,
            task_id: self.task_id.clone(),
            turn: self.turn,
            seq,
            event_id: format!("evt:{}:{}:{seq}", self.task_id, self.turn),
            emitted_at: timestamp_string(Utc::now()),
            source: source.unwrap_or_else(|| source_for_event(&event)),
            request_id,
            parent_event_id,
            event,
        };
        self.next_seq = seq.saturating_add(1);
        envelope
    }
}

fn decode_provider_content_block(value: serde_json::Value) -> Option<ProviderContentBlock> {
    let mut value = value;
    if let Some(object) = value.as_object_mut()
        && object
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|tag| {
                tag == legacy_external_thinking_tag() || tag == transitional_internal_thinking_tag()
            })
    {
        object.insert(
            "type".to_string(),
            serde_json::Value::String(THINKING_DATA_TAG.to_string()),
        );
    }

    serde_json::from_value::<ProviderContentBlockCompat>(value)
        .ok()
        .map(Into::into)
}

impl From<ProviderContentBlockCompat> for ProviderContentBlock {
    fn from(value: ProviderContentBlockCompat) -> Self {
        match value {
            ProviderContentBlockCompat::Text { text, citations } => {
                let _ = citations;
                Self::Text { text }
            }
            ProviderContentBlockCompat::ToolUse {
                id,
                name,
                input,
                metadata,
            } => {
                let _ = metadata;
                Self::ToolUse { id, name, input }
            }
            ProviderContentBlockCompat::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let _ = (tool_use_id, content, is_error);
                Self::ToolResult
            }
            ProviderContentBlockCompat::Thinking {
                thinking,
                signature,
            } => {
                let _ = signature;
                Self::Thinking { thinking }
            }
            ProviderContentBlockCompat::ThinkingData { data } => Self::ThinkingData { data },
            ProviderContentBlockCompat::ServerToolUse { id, name, input } => {
                Self::ServerToolUse { id, name, input }
            }
            ProviderContentBlockCompat::WebSearchToolResult {
                tool_use_id,
                content,
            } => {
                let _ = (tool_use_id, content);
                Self::WebSearchToolResult
            }
        }
    }
}

fn provider_stream_block(
    content_block: &ProviderContentBlock,
) -> Option<(StreamBlock, Option<String>)> {
    match content_block {
        ProviderContentBlock::Text { text } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(text.clone()),
        )),
        ProviderContentBlock::Thinking { thinking } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(thinking.clone()),
        )),
        ProviderContentBlock::ThinkingData { data } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(data.clone()),
        )),
        ProviderContentBlock::ToolUse { id, name, input }
        | ProviderContentBlock::ServerToolUse { id, name, input } => Some((
            StreamBlock::ToolCall {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
                status: ToolStatus::Pending,
            },
            None,
        )),
        ProviderContentBlock::ToolResult | ProviderContentBlock::WebSearchToolResult => None,
    }
}

fn provider_block_delta(delta: &ProviderDelta) -> Option<ProviderBlockDelta> {
    if let Some(text) = delta.text.as_deref()
        && !text.is_empty()
    {
        return Some(ProviderBlockDelta::Text(text.to_string()));
    }
    if let Some(thinking) = delta.thinking.as_deref()
        && !thinking.is_empty()
    {
        return Some(ProviderBlockDelta::Text(thinking.to_string()));
    }
    if let Some(partial_json) = delta.partial_json.as_deref()
        && !partial_json.is_empty()
    {
        return Some(ProviderBlockDelta::ToolArguments(partial_json.to_string()));
    }

    None
}

fn message_start_metadata(metadata: Option<StreamChunkMetadata>) -> Option<StreamChunkMetadata> {
    let mut metadata = metadata?;
    metadata.prompt_progress = None;
    metadata.timings = None;

    (metadata.object.is_some()
        || metadata.created.is_some()
        || metadata.system_fingerprint.is_some()
        || metadata.service_tier.is_some()
        || metadata.choice_index.is_some()
        || metadata.logprobs.is_some())
    .then_some(metadata)
}

fn chat_compat_metadata(
    chunk: &ChatCompatChunk,
    choice_index: Option<usize>,
    logprobs: Option<serde_json::Value>,
) -> Option<StreamChunkMetadata> {
    let metadata = StreamChunkMetadata {
        object: chunk.object.clone(),
        created: chunk.created,
        system_fingerprint: chunk.system_fingerprint.clone(),
        service_tier: chunk.service_tier.clone(),
        choice_index,
        logprobs,
        prompt_progress: chunk.prompt_progress.clone(),
        timings: chunk.timings.clone(),
    };
    (metadata.object.is_some()
        || metadata.created.is_some()
        || metadata.system_fingerprint.is_some()
        || metadata.service_tier.is_some()
        || metadata.choice_index.is_some()
        || metadata.logprobs.is_some()
        || metadata.prompt_progress.is_some()
        || metadata.timings.is_some())
    .then_some(metadata)
}

fn token_usage_from_api_usage(usage: &ApiUsage) -> Option<TokenUsageEnvelope> {
    let usage = TokenUsageEnvelope {
        input: usage.input_tokens.unwrap_or(0),
        output: usage.output_tokens.unwrap_or(0),
        estimated: false,
        cache_creation_input: usage.cache_creation_input_tokens.unwrap_or(0),
        cache_read_input: usage.cache_read_input_tokens.unwrap_or(0),
    };

    (!usage.input.eq(&0)
        || !usage.output.eq(&0)
        || !usage.cache_creation_input.eq(&0)
        || !usage.cache_read_input.eq(&0))
    .then_some(usage)
}

fn accumulate_turn_tokens(turn_tokens: &mut TurnTokens, usage: &ApiUsage) {
    if let Some(input) = usage.input_tokens {
        turn_tokens.input = turn_tokens.input.saturating_add(input);
    }
    if let Some(output) = usage.output_tokens {
        turn_tokens.output = turn_tokens.output.saturating_add(output);
    }
    if let Some(cache_creation) = usage.cache_creation_input_tokens {
        turn_tokens.cache_creation_input_tokens = turn_tokens
            .cache_creation_input_tokens
            .saturating_add(cache_creation);
    }
    if let Some(cache_read) = usage.cache_read_input_tokens {
        turn_tokens.cache_read_input_tokens = turn_tokens
            .cache_read_input_tokens
            .saturating_add(cache_read);
    }
}

pub fn approval_resolution_event(request: &RuntimeRequest) -> Option<RuntimeEvent> {
    match request {
        RuntimeRequest::ApproveCapability {
            capability, scope, ..
        } => Some(RuntimeEvent::ApprovalResolved {
            capability: capability.clone(),
            scope: scope.clone(),
            approved: true,
        }),
        RuntimeRequest::DenyCapability { capability, .. } => Some(RuntimeEvent::ApprovalResolved {
            capability: capability.clone(),
            scope: "once".to_string(),
            approved: false,
        }),
        RuntimeRequest::SubmitInput { .. } | RuntimeRequest::Interrupt { .. } => None,
    }
}

pub fn derive_batch_records(
    envelopes: &[RuntimeEnvelope],
    instructions_path: Option<String>,
) -> DerivedBatchRecords {
    let mut turns = Vec::new();
    let mut current_turn: Option<DerivedTurnState> = None;
    let mut task_id = None;
    let mut last_status = None;
    let mut all_changed_files = BTreeSet::new();
    let mut max_turns_reached = false;

    for envelope in envelopes {
        task_id.get_or_insert_with(|| envelope.task_id.clone());
        match &envelope.event {
            RuntimeEvent::TurnStart { input } => {
                if let Some(state) = current_turn.take() {
                    turns.push(state.into_record(instructions_path.clone()));
                }
                current_turn = Some(DerivedTurnState {
                    turn: envelope.turn as usize,
                    input: input.clone().unwrap_or_default(),
                    ..DerivedTurnState::default()
                });
            }
            RuntimeEvent::TranscriptBlockStart { index, block } => {
                if let Some(state) = current_turn.as_mut()
                    && let StreamBlock::FinalText { content } = block
                {
                    state.start_final_text_block(*index, content.clone());
                }
            }
            RuntimeEvent::TranscriptBlockDelta { index, delta } => {
                if let Some(state) = current_turn.as_mut() {
                    state.append_final_text_delta(*index, delta);
                }
            }
            RuntimeEvent::TranscriptBlockComplete { index } => {
                if let Some(state) = current_turn.as_mut() {
                    state.complete_final_text_block(*index);
                }
            }
            RuntimeEvent::ToolCallCompleted {
                tool_name, status, ..
            } => {
                if let Some(state) = current_turn.as_mut()
                    && let Some(name) = tool_name
                    && let Some(evidence) =
                        command_evidence_from_tool_result(name, *status == ToolStatus::Error)
                {
                    state.command_history.push(evidence);
                }
            }
            RuntimeEvent::ToolCallFailed {
                tool_name, status, ..
            } => {
                if let Some(state) = current_turn.as_mut()
                    && let Some(name) = tool_name
                    && let Some(evidence) =
                        command_evidence_from_tool_result(name, *status == ToolStatus::Error)
                {
                    state.command_history.push(evidence);
                }
            }
            RuntimeEvent::TurnEnd {
                status,
                usage,
                changed_files,
            } => {
                if let Some(mut state) = current_turn.take() {
                    state.changed_files = changed_files.clone();
                    state.tokens = usage
                        .as_ref()
                        .map_or_else(TurnTokens::default, turn_tokens_from_usage);
                    for path in &state.changed_files {
                        all_changed_files.insert(path.clone());
                    }
                    last_status = Some(status.clone());
                    turns.push(state.into_record(instructions_path.clone()));
                }
            }
            RuntimeEvent::MaxTurnsReached { .. } => {
                max_turns_reached = true;
            }
            RuntimeEvent::TranscriptLine { .. }
            | RuntimeEvent::ToolCallStarted { .. }
            | RuntimeEvent::ToolCallArgumentsDelta { .. }
            | RuntimeEvent::ToolCallStatusUpdated { .. }
            | RuntimeEvent::TranscriptBlockPhaseUpdated { .. }
            | RuntimeEvent::ServerMetadata { .. }
            | RuntimeEvent::UsageUpdated { .. }
            | RuntimeEvent::ApprovalRequest { .. }
            | RuntimeEvent::ApprovalResolved { .. }
            | RuntimeEvent::ValidationResult { .. }
            | RuntimeEvent::Error { .. } => {}
        }
    }

    if let Some(state) = current_turn.take() {
        for path in &state.changed_files {
            all_changed_files.insert(path.clone());
        }
        turns.push(state.into_record(instructions_path.clone()));
    }

    let summary = task_id.and_then(|task_id| {
        last_status.map(|status| SummaryRecord {
            summary: true,
            status,
            task_id,
            total_turns: turns.len(),
            instructions_path,
            changed_files: all_changed_files.into_iter().collect(),
            session_tasks: Vec::new(),
        })
    });

    DerivedBatchRecords {
        turns,
        summary,
        max_turns_reached,
    }
}

fn source_for_stream_block(block: &StreamBlock) -> RuntimeEnvelopeSource {
    match block {
        StreamBlock::ToolResult { .. } => RuntimeEnvelopeSource::Runtime,
        StreamBlock::Thinking { .. }
        | StreamBlock::ToolCall { .. }
        | StreamBlock::FinalText { .. } => RuntimeEnvelopeSource::Model,
    }
}

fn source_for_event(event: &RuntimeEvent) -> RuntimeEnvelopeSource {
    match event {
        RuntimeEvent::TurnStart { .. } | RuntimeEvent::ApprovalResolved { .. } => {
            RuntimeEnvelopeSource::UserRequest
        }
        RuntimeEvent::TranscriptLine { .. } => RuntimeEnvelopeSource::Runtime,
        RuntimeEvent::TranscriptBlockStart { block, .. } => source_for_stream_block(block),
        RuntimeEvent::TranscriptBlockComplete { .. }
        | RuntimeEvent::ToolCallStarted { .. }
        | RuntimeEvent::ToolCallArgumentsDelta { .. }
        | RuntimeEvent::ServerMetadata { .. }
        | RuntimeEvent::UsageUpdated { .. } => RuntimeEnvelopeSource::Model,
        // TranscriptBlockDelta callers always supply the block's recorded
        // source via next_envelope_with_source; this fallback only fires if a
        // delta arrives for an unregistered block index, in which case Runtime
        // is a safer default than assuming Model.
        RuntimeEvent::TranscriptBlockDelta { .. } => RuntimeEnvelopeSource::Runtime,
        RuntimeEvent::ToolCallStatusUpdated { .. }
        | RuntimeEvent::TranscriptBlockPhaseUpdated { .. }
        | RuntimeEvent::ToolCallCompleted { .. }
        | RuntimeEvent::ToolCallFailed { .. }
        | RuntimeEvent::ApprovalRequest { .. }
        | RuntimeEvent::ValidationResult { .. }
        | RuntimeEvent::TurnEnd { .. }
        | RuntimeEvent::Error { .. }
        | RuntimeEvent::MaxTurnsReached { .. } => RuntimeEnvelopeSource::Runtime,
    }
}

fn timestamp_string(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests;
