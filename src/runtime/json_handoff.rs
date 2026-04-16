use super::UiUpdate;
use crate::runtime::AssistantPhase;
use crate::runtime::delta_accumulator::DeltaAccumulator;
use crate::state::{StreamBlock, ToolStatus};
use crate::turn_evidence::{
    SummaryRecord, TurnEvidenceRecord, command_evidence_from_tool_result,
    note_changed_files_from_tool_call,
};
use crate::types::ContentBlock;
use crate::usage::TurnTokens;
use chrono::{Timelike, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

mod derived;

use self::derived::{DerivedTurnState, empty_json_object, turn_tokens_from_usage};
pub use self::derived::{runtime_approval_request_event, token_usage_from_turn_tokens};

const SYNTHETIC_FINAL_TEXT_BLOCK_START: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEnvelope {
    pub version: u16,
    pub task_id: String,
    pub turn: u32,
    pub seq: u64,
    pub event: RuntimeEvent,
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
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        is_error: bool,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        input: String,
    },
    Interrupt {
        task_id: String,
    },
    ApproveCapability {
        task_id: String,
        capability: String,
        scope: String,
    },
    DenyCapability {
        task_id: String,
        capability: String,
    },
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
}

pub struct RuntimeEnvelopeNormalizer {
    task_id: String,
    turn: u32,
    next_seq: u64,
    pending_tool_calls: IndexMap<String, PendingToolCall>,
    streaming_tool_call_blocks: HashMap<usize, ToolCallId>,
    turn_changed_files: BTreeSet<String>,
    tool_id_counter: AtomicU32,
    open_final_text_block: Option<usize>,
    next_synthetic_block_index: usize,
    delta_accumulator: Option<Arc<DeltaAccumulator>>,
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
            streaming_tool_call_blocks: HashMap::new(),
            turn_changed_files: BTreeSet::new(),
            tool_id_counter: AtomicU32::new(0),
            open_final_text_block: None,
            next_synthetic_block_index: SYNTHETIC_FINAL_TEXT_BLOCK_START,
            delta_accumulator,
        }
    }

    pub fn start_turn(&mut self, turn: u32, input: Option<String>) -> RuntimeEnvelope {
        self.turn = turn;
        self.next_seq = 1;
        self.pending_tool_calls.clear();
        self.streaming_tool_call_blocks.clear();
        self.turn_changed_files.clear();
        self.open_final_text_block = None;
        self.next_synthetic_block_index = SYNTHETIC_FINAL_TEXT_BLOCK_START;

        self.next_envelope(RuntimeEvent::TurnStart { input })
    }

    pub fn normalize_content_block(&mut self, block: &ContentBlock) -> Vec<RuntimeEnvelope> {
        match block {
            ContentBlock::ToolUse {
                id, name, input, ..
            }
            | ContentBlock::ServerToolUse { id, name, input } => {
                let envelope = self.record_tool_call(id.clone(), name.clone(), input.clone());
                self.seed_tool_accumulator_for_source(id, input);
                vec![envelope]
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => vec![self.record_tool_result(tool_use_id, content.clone(), *is_error)],
            ContentBlock::Text { .. }
            | ContentBlock::Thinking { .. }
            | ContentBlock::RedactedThinking { .. }
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
                envelopes.push(self.next_envelope(RuntimeEvent::TranscriptBlockDelta {
                    index,
                    delta: text.clone(),
                }));
                envelopes
            }
            UiUpdate::StreamBlockStart { index, block } => {
                let mut envelopes = self.close_open_final_text_block();
                envelopes.push(self.next_envelope(RuntimeEvent::TranscriptBlockStart {
                    index: *index,
                    block: block.clone(),
                }));
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
                if let Some(runtime_id) = self.streaming_tool_call_blocks.get(index).cloned() {
                    self.accumulate_tool_delta(&runtime_id, delta);
                }
                vec![self.next_envelope(RuntimeEvent::TranscriptBlockDelta {
                    index: *index,
                    delta: delta.clone(),
                })]
            }
            UiUpdate::StreamBlockComplete { index } => {
                if let Some(runtime_id) = self.streaming_tool_call_blocks.remove(index) {
                    self.finish_tool_accumulator(&runtime_id);
                }
                vec![self.next_envelope(RuntimeEvent::TranscriptBlockComplete { index: *index })]
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
            UiUpdate::ServerMetadata(_) => Vec::new(),
            UiUpdate::CommandSessionStarted { .. }
            | UiUpdate::CommandSessionAttached { .. }
            | UiUpdate::CommandSessionFinished { .. }
            | UiUpdate::EditLoopComplete { .. }
            | UiUpdate::ContextCompacted { .. } => Vec::new(),
        }
    }

    pub fn normalize_runtime_request(&mut self, request: &RuntimeRequest) -> Vec<RuntimeEnvelope> {
        approval_resolution_event(request)
            .map(|event| vec![self.next_envelope(event)])
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
            self.streaming_tool_call_blocks.clear();
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
        self.streaming_tool_call_blocks.clear();
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
        self.pending_tool_calls.clear();
        self.streaming_tool_call_blocks.clear();
        let mut envelopes = self.close_open_final_text_block();
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

        (
            index,
            Some(self.next_envelope(RuntimeEvent::TranscriptBlockStart {
                index,
                block: StreamBlock::FinalText {
                    content: String::new(),
                },
            })),
        )
    }

    fn close_open_final_text_block(&mut self) -> Vec<RuntimeEnvelope> {
        let Some(index) = self.open_final_text_block.take() else {
            return Vec::new();
        };

        vec![self.next_envelope(RuntimeEvent::TranscriptBlockComplete { index })]
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
        self.pending_tool_calls.insert(
            runtime_id.clone(),
            PendingToolCall {
                runtime_id: runtime_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            },
        );
        self.start_tool_accumulator(&runtime_id, &name);
        self.seed_tool_accumulator(&runtime_id, &arguments);

        Some(self.next_envelope(RuntimeEvent::ToolCall {
            id: runtime_id,
            name,
            arguments,
        }))
    }

    fn record_tool_call(
        &mut self,
        source_id: String,
        name: String,
        arguments: serde_json::Value,
    ) -> RuntimeEnvelope {
        let runtime_id = self.generate_tool_call_id();
        self.pending_tool_calls.insert(
            source_id,
            PendingToolCall {
                runtime_id: runtime_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            },
        );
        self.start_tool_accumulator(&runtime_id, &name);

        self.next_envelope(RuntimeEvent::ToolCall {
            id: runtime_id,
            name,
            arguments,
        })
    }

    fn record_tool_result(
        &mut self,
        source_id: &str,
        output: String,
        is_error: bool,
    ) -> RuntimeEnvelope {
        if let Some(pending) = self.pending_tool_calls.shift_remove(source_id) {
            self.finish_tool_accumulator(&pending.runtime_id);
            if !is_error {
                note_changed_files_from_tool_call(
                    &mut self.turn_changed_files,
                    &pending.name,
                    &pending.arguments,
                );
            }
            return self.next_envelope(RuntimeEvent::ToolResult {
                tool_call_id: pending.runtime_id,
                tool_name: Some(pending.name),
                is_error,
                output,
            });
        }

        self.next_envelope(RuntimeEvent::ToolResult {
            tool_call_id: source_id.to_string(),
            tool_name: None,
            is_error,
            output,
        })
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

    fn seed_tool_accumulator_for_source(&self, source_id: &str, arguments: &serde_json::Value) {
        let Some(pending) = self.pending_tool_calls.get(source_id) else {
            return;
        };

        self.seed_tool_accumulator(&pending.runtime_id, arguments);
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

    fn accumulate_tool_delta(&self, tool_call_id: &ToolCallId, delta: &str) {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return;
        };

        if let Err(error) = delta_accumulator.accumulate(tool_call_id, delta) {
            tracing::debug!(%error, tool_call_id = %tool_call_id, "failed to accumulate tool delta");
        }
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
        let envelope = RuntimeEnvelope {
            version: 1,
            task_id: self.task_id.clone(),
            turn: self.turn,
            seq: self.next_seq,
            event,
        };
        self.next_seq = self.next_seq.saturating_add(1);
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
            RuntimeEvent::ToolResult {
                tool_name,
                is_error,
                ..
            } => {
                if let Some(state) = current_turn.as_mut()
                    && let Some(name) = tool_name
                    && let Some(evidence) = command_evidence_from_tool_result(name, *is_error)
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
            | RuntimeEvent::ToolCall { .. }
            | RuntimeEvent::ToolCallStatusUpdated { .. }
            | RuntimeEvent::TranscriptBlockPhaseUpdated { .. }
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

#[cfg(test)]
mod tests;
