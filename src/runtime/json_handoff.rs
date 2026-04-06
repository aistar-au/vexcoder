use super::UiUpdate;
use crate::state::StreamBlock;
use crate::turn_evidence::{
    command_evidence_from_tool_result, note_changed_files_from_tool_call, SummaryRecord,
    TurnEvidenceRecord,
};
use crate::types::ContentBlock;
use crate::usage::TurnTokens;
use chrono::{Timelike, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod derived;

use self::derived::{empty_json_object, turn_tokens_from_usage, DerivedTurnState};
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

#[derive(Debug, Clone)]
struct PendingToolCall {
    runtime_id: String,
    name: String,
    arguments: serde_json::Value,
}

pub struct RuntimeEnvelopeNormalizer {
    task_id: String,
    turn: u32,
    next_seq: u64,
    pending_tool_calls: IndexMap<String, PendingToolCall>,
    turn_changed_files: BTreeSet<String>,
    tool_id_counter: u64,
    open_final_text_block: Option<usize>,
    next_synthetic_block_index: usize,
}

impl RuntimeEnvelopeNormalizer {
    pub fn new(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            turn: 0,
            next_seq: 1,
            pending_tool_calls: IndexMap::new(),
            turn_changed_files: BTreeSet::new(),
            tool_id_counter: 0,
            open_final_text_block: None,
            next_synthetic_block_index: SYNTHETIC_FINAL_TEXT_BLOCK_START,
        }
    }

    pub fn start_turn(&mut self, turn: u32, input: Option<String>) -> RuntimeEnvelope {
        self.turn = turn;
        self.next_seq = 1;
        self.pending_tool_calls.clear();
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
                vec![self.record_tool_call(id.clone(), name.clone(), input.clone())]
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
        terminal: Option<TurnEndContext>,
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
                envelopes
            }
            UiUpdate::StreamBlockDelta { index, delta } => {
                vec![self.next_envelope(RuntimeEvent::TranscriptBlockDelta {
                    index: *index,
                    delta: delta.clone(),
                })]
            }
            UiUpdate::StreamBlockComplete { index } => {
                vec![self.next_envelope(RuntimeEvent::TranscriptBlockComplete { index: *index })]
            }
            UiUpdate::TurnComplete => self.complete_turn(terminal.unwrap_or_default()),
            UiUpdate::Error(message) => self.emit_error(
                "runtime_error".to_string(),
                message.clone(),
                false,
                terminal.unwrap_or_default(),
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
        terminal: TurnEndContext,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.close_open_final_text_block();
        envelopes.push(self.next_envelope(RuntimeEvent::Error {
            code,
            message,
            recoverable,
        }));

        if !recoverable {
            envelopes.push(self.next_envelope(RuntimeEvent::TurnEnd {
                status: "failed".to_string(),
                usage: terminal.usage,
                changed_files: self.resolve_changed_files(terminal.changed_files),
            }));
        }

        envelopes
    }

    pub fn emit_max_turns_reached(
        &mut self,
        max_turns: u32,
        terminal: TurnEndContext,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.close_open_final_text_block();
        envelopes.push(self.next_envelope(RuntimeEvent::MaxTurnsReached { max_turns }));
        envelopes.push(self.next_envelope(RuntimeEvent::TurnEnd {
            status: "failed".to_string(),
            usage: terminal.usage,
            changed_files: self.resolve_changed_files(terminal.changed_files),
        }));
        envelopes
    }

    pub fn emit_cancelled(&mut self, terminal: TurnEndContext) -> Vec<RuntimeEnvelope> {
        self.finish_turn_with_status("cancelled", terminal)
    }

    pub fn emit_event(&mut self, event: RuntimeEvent) -> RuntimeEnvelope {
        self.next_envelope(event)
    }

    fn complete_turn(&mut self, terminal: TurnEndContext) -> Vec<RuntimeEnvelope> {
        self.finish_turn_with_status("completed", terminal)
    }

    fn finish_turn_with_status(
        &mut self,
        status: &str,
        terminal: TurnEndContext,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.close_open_final_text_block();
        envelopes.push(self.next_envelope(RuntimeEvent::TurnEnd {
            status: status.to_string(),
            usage: terminal.usage,
            changed_files: self.resolve_changed_files(terminal.changed_files),
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

    fn generate_tool_call_id(&mut self) -> String {
        self.tool_id_counter = self.tool_id_counter.saturating_add(1);
        let now = Utc::now();
        let millis = now.timestamp_millis() as u128;
        let entropy = ((now.nanosecond() as u128 ^ self.tool_id_counter as u128) & 0xffff) as u16;
        format!("call_{millis}_{entropy:04x}")
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
                if let Some(state) = current_turn.as_mut() {
                    if let StreamBlock::FinalText { content } = block {
                        state.start_final_text_block(*index, content.clone());
                    }
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
                if let Some(state) = current_turn.as_mut() {
                    if let Some(name) = tool_name {
                        if let Some(evidence) = command_evidence_from_tool_result(name, *is_error) {
                            state.command_history.push(evidence);
                        }
                    }
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
