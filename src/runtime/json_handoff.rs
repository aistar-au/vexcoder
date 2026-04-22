use super::UiUpdate;
use crate::runtime::AssistantPhase;
use crate::runtime::delta_accumulator::DeltaAccumulator;
use crate::state::{StreamBlock, ToolStatus};
use crate::turn_evidence::{SummaryRecord, TurnEvidenceRecord, command_evidence_from_tool_result};
use crate::types::{ContentBlock, StreamChunkMetadata};
use crate::usage::TurnTokens;
use chrono::{DateTime, SecondsFormat, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

mod derived;
mod protocol_ingress;
mod tool_calls;

use self::derived::{DerivedTurnState, turn_tokens_from_usage};
pub use self::derived::{runtime_approval_request_event, token_usage_from_turn_tokens};
use self::protocol_ingress::ProtocolIngressState;
use self::tool_calls::{PendingToolCall, PendingToolCallContext};

const SYNTHETIC_FINAL_TEXT_BLOCK_START: usize = 1_000_000;

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
    
    
    ToolCallStatusUpdated {
        tool_call_id: String,
        status: ToolStatus,
    },
    
    
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments: Option<serde_json::Value>,
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


pub fn generate_tool_call_id(counter: &AtomicU32, entropy: u16) -> ToolCallId {
    let count = counter.fetch_add(1, Ordering::SeqCst).saturating_add(1);
    format!("tx_{}_{entropy:04x}", count)
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
    protocol_ingress: ProtocolIngressState,
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
            protocol_ingress: ProtocolIngressState::default(),
        }
    }

    pub fn start_turn(&mut self, turn: u32, input: Option<String>) -> RuntimeEnvelope {
        self.turn = turn;
        
        
        self.pending_tool_calls.clear();
        self.pending_tool_call_contexts.clear();
        self.streaming_tool_call_blocks.clear();
        self.block_sources.clear();
        self.turn_changed_files.clear();
        self.open_final_text_block = None;
        self.next_synthetic_block_index = SYNTHETIC_FINAL_TEXT_BLOCK_START;
        self.protocol_ingress = ProtocolIngressState::default();

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
