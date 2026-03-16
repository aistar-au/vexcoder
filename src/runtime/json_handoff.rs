use super::UiUpdate;
use crate::state::{StreamBlock, ToolApprovalRequest};
use crate::turn_evidence::{
    command_evidence_from_tool_result, note_changed_files_from_tool_call, SummaryRecord,
    TurnEvidenceRecord,
};
use crate::types::ContentBlock;
use crate::usage::TurnTokens;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

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
    AssistantDelta {
        text: String,
    },
    AssistantMessage {
        content: String,
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
    assistant_text: String,
    pending_tool_calls: HashMap<String, PendingToolCall>,
    turn_changed_files: BTreeSet<String>,
    tool_id_counter: u64,
}

impl RuntimeEnvelopeNormalizer {
    pub fn new(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            turn: 0,
            next_seq: 1,
            assistant_text: String::new(),
            pending_tool_calls: HashMap::new(),
            turn_changed_files: BTreeSet::new(),
            tool_id_counter: 0,
        }
    }

    pub fn start_turn(&mut self, turn: u32, input: Option<String>) -> RuntimeEnvelope {
        self.turn = turn;
        self.next_seq = 1;
        self.assistant_text.clear();
        self.pending_tool_calls.clear();
        self.turn_changed_files.clear();

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
            UiUpdate::TranscriptLine(_) => Vec::new(),
            UiUpdate::StreamDelta(text) => {
                self.assistant_text.push_str(text);
                vec![self.next_envelope(RuntimeEvent::AssistantDelta { text: text.clone() })]
            }
            UiUpdate::TurnComplete => self.complete_turn(terminal.unwrap_or_default()),
            UiUpdate::Error(message) => self.emit_error(
                "runtime_error".to_string(),
                message.clone(),
                false,
                terminal.unwrap_or_default(),
            ),
            UiUpdate::ToolApprovalRequest(request) => {
                vec![self.next_envelope(runtime_approval_request_event(request))]
            }
            UiUpdate::StreamBlockStart { .. }
            | UiUpdate::StreamBlockDelta { .. }
            | UiUpdate::StreamBlockComplete { .. }
            | UiUpdate::CommandSessionStarted { .. }
            | UiUpdate::CommandSessionAttached { .. }
            | UiUpdate::CommandSessionFinished { .. }
            | UiUpdate::EditLoopComplete { .. } => Vec::new(),
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
        let mut envelopes = vec![self.next_envelope(RuntimeEvent::Error {
            code,
            message,
            recoverable,
        })];

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
        vec![
            self.next_envelope(RuntimeEvent::MaxTurnsReached { max_turns }),
            self.next_envelope(RuntimeEvent::TurnEnd {
                status: "failed".to_string(),
                usage: terminal.usage,
                changed_files: self.resolve_changed_files(terminal.changed_files),
            }),
        ]
    }

    fn complete_turn(&mut self, terminal: TurnEndContext) -> Vec<RuntimeEnvelope> {
        vec![
            self.next_envelope(RuntimeEvent::AssistantMessage {
                content: self.assistant_text.clone(),
            }),
            self.next_envelope(RuntimeEvent::TurnEnd {
                status: "completed".to_string(),
                usage: terminal.usage,
                changed_files: self.resolve_changed_files(terminal.changed_files),
            }),
        ]
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
        if let Some(pending) = self.pending_tool_calls.remove(source_id) {
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
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let millis = now.as_millis();
        let entropy = ((now.as_nanos() ^ self.tool_id_counter as u128) & 0xffff) as u16;
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
            RuntimeEvent::AssistantDelta { text } => {
                if let Some(state) = current_turn.as_mut() {
                    state.delta_response.push_str(text);
                }
            }
            RuntimeEvent::AssistantMessage { content } => {
                if let Some(state) = current_turn.as_mut() {
                    state.assistant_message = Some(content.clone());
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
            RuntimeEvent::ToolCall { .. }
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
        })
    });

    DerivedBatchRecords {
        turns,
        summary,
        max_turns_reached,
    }
}

pub fn token_usage_from_turn_tokens(tokens: TurnTokens) -> Option<TokenUsageEnvelope> {
    if tokens.is_zero() {
        None
    } else {
        Some(TokenUsageEnvelope {
            input: tokens.input,
            output: tokens.output,
            estimated: tokens.estimated,
        })
    }
}

fn turn_tokens_from_usage(usage: &TokenUsageEnvelope) -> TurnTokens {
    TurnTokens {
        input: usage.input,
        output: usage.output,
        estimated: usage.estimated,
    }
}

fn runtime_approval_request_event(request: &ToolApprovalRequest) -> RuntimeEvent {
    let capability = capability_name_for_tool(&request.tool_name).unwrap_or("unknown");
    RuntimeEvent::ApprovalRequest {
        capability: capability.to_string(),
        scope: "once".to_string(),
        tool_name: Some(request.tool_name.clone()),
    }
}

fn capability_name_for_tool(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "read_file" | "list_files" | "list_directory" | "search" | "search_files"
        | "search_content" | "find_files" | "git_status" | "git_diff" | "git_log" | "git_show" => {
            Some("read-file")
        }
        "write_file" | "edit_file" | "rename_file" => Some("write-file"),
        "apply_patch" | "git_add" | "git_commit" => Some("apply-patch"),
        "run_command" => Some("run-command"),
        _ => None,
    }
}

fn empty_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[derive(Default)]
struct DerivedTurnState {
    turn: usize,
    input: String,
    delta_response: String,
    assistant_message: Option<String>,
    changed_files: Vec<String>,
    command_history: Vec<super::CommandEvidence>,
    tokens: TurnTokens,
}

impl DerivedTurnState {
    fn into_record(self, instructions_path: Option<String>) -> TurnEvidenceRecord {
        TurnEvidenceRecord {
            turn: self.turn,
            input: self.input,
            response: self.assistant_message.unwrap_or(self.delta_response),
            instructions_path,
            changed_files: self.changed_files,
            command_history: self.command_history,
            tokens: self.tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::sync::oneshot;

    #[test]
    fn test_pi_09_anchor_runtime_envelope_serde_shape() {
        let envelope = RuntimeEnvelope {
            version: 1,
            task_id: "task-1741700000000".to_string(),
            turn: 1,
            seq: 3,
            event: RuntimeEvent::ToolCall {
                id: "call_1741700123456_9a2f".to_string(),
                name: "read_file".to_string(),
                arguments: json!({
                    "path": "src/app.rs"
                }),
            },
        };

        let value = serde_json::to_value(&envelope).expect("runtime envelope must serialize");
        assert_eq!(value["version"], 1);
        assert_eq!(value["task_id"], "task-1741700000000");
        assert_eq!(value["turn"], 1);
        assert_eq!(value["seq"], 3);
        assert_eq!(value["event"]["type"], "tool_call");
        assert_eq!(value["event"]["id"], "call_1741700123456_9a2f");
        assert_eq!(value["event"]["name"], "read_file");
        assert_eq!(value["event"]["arguments"]["path"], "src/app.rs");
    }

    #[test]
    fn test_pi_11_schema_assets_parse_as_json() {
        let envelope_schema: serde_json::Value =
            serde_json::from_str(include_str!("../../schemas/runtime_envelope_v1.json"))
                .expect("runtime envelope schema must parse");
        let request_schema: serde_json::Value =
            serde_json::from_str(include_str!("../../schemas/runtime_request_v1.json"))
                .expect("runtime request schema must parse");

        assert_eq!(
            envelope_schema["$id"],
            "https://vexcoder.io/schemas/runtime_envelope_v1.json"
        );
        assert_eq!(
            request_schema["$id"],
            "https://vexcoder.io/schemas/runtime_request_v1.json"
        );
        assert_eq!(envelope_schema["properties"]["version"]["const"], 1);
        assert_eq!(request_schema["$defs"]["scope"]["enum"][0], "once");
        assert_eq!(request_schema["$defs"]["scope"]["enum"][1], "session");
    }

    #[test]
    fn test_pi_11_tool_call_grammar_keeps_mcp_namespace_rule() {
        let grammar = include_str!("../../grammars/tool_call.gbnf");

        assert!(grammar.contains("tool_call ::= \"{\""));
        assert!(grammar.contains("mcp_tool ::= \"\\\"mcp."));
        assert!(grammar.contains("\\\"read_file\\\""));
        assert!(grammar.contains("\\\"apply_patch\\\""));
        assert!(grammar.contains("\"*\" | \"?\""));
    }

    #[test]
    fn test_pi_10_normalization_discards_provider_ids_and_tracks_results() {
        let mut normalizer = RuntimeEnvelopeNormalizer::new("task-1");
        let start = normalizer.start_turn(1, Some("ship it".to_string()));
        assert_eq!(start.seq, 1);

        let tool_call = normalizer
            .normalize_content_block(&ContentBlock::ToolUse {
                id: "provider-call-1".to_string(),
                name: "write_file".to_string(),
                input: json!({"path":"src/main.rs"}),
                metadata: None,
            })
            .pop()
            .expect("tool call envelope");

        let runtime_call_id = match &tool_call.event {
            RuntimeEvent::ToolCall {
                id,
                name,
                arguments,
            } => {
                assert_ne!(id, "provider-call-1");
                assert_runtime_tool_id(id);
                assert_eq!(name, "write_file");
                assert_eq!(arguments["path"], "src/main.rs");
                id.clone()
            }
            other => panic!("expected tool_call event, got {other:?}"),
        };

        let tool_result = normalizer
            .normalize_content_block(&ContentBlock::ToolResult {
                tool_use_id: "provider-call-1".to_string(),
                content: "ok".to_string(),
                is_error: false,
            })
            .pop()
            .expect("tool result envelope");

        match tool_result.event {
            RuntimeEvent::ToolResult {
                tool_call_id,
                tool_name,
                is_error,
                output,
            } => {
                assert_eq!(tool_call_id, runtime_call_id);
                assert_eq!(tool_name.as_deref(), Some("write_file"));
                assert!(!is_error);
                assert_eq!(output, "ok");
            }
            other => panic!("expected tool_result event, got {other:?}"),
        }

        let grammar_calls = normalizer.normalize_tool_call_array(&json!([
            {"name":"read_file","arguments":{"path":"src/lib.rs"}},
            {"name":"apply_patch","arguments":{"path":"src/lib.rs"}}
        ]));
        assert_eq!(grammar_calls.len(), 2);
        assert_eq!(grammar_calls[0].seq, 4);
        assert_eq!(grammar_calls[1].seq, 5);
    }

    #[test]
    fn test_pi_10_normalization_projects_ui_updates_and_approval_events() {
        let mut normalizer = RuntimeEnvelopeNormalizer::new("task-2");
        let _ = normalizer.start_turn(1, Some("review".to_string()));

        let delta = normalizer
            .normalize_ui_update(&UiUpdate::StreamDelta("hello".to_string()), None)
            .pop()
            .expect("delta envelope");
        assert_eq!(delta.seq, 2);
        assert!(matches!(
            delta.event,
            RuntimeEvent::AssistantDelta { ref text } if text == "hello"
        ));

        let (response_tx, _response_rx) = oneshot::channel();
        let approval = normalizer
            .normalize_ui_update(
                &UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                    tool_name: "apply_patch".to_string(),
                    input_preview: "{}".to_string(),
                    response_tx,
                }),
                None,
            )
            .pop()
            .expect("approval envelope");

        assert!(matches!(
            approval.event,
            RuntimeEvent::ApprovalRequest {
                ref capability,
                ref scope,
                tool_name: Some(ref tool_name)
            } if capability == "apply-patch" && scope == "once" && tool_name == "apply_patch"
        ));

        let resolved = normalizer
            .normalize_runtime_request(&RuntimeRequest::ApproveCapability {
                task_id: "task-2".to_string(),
                capability: "apply-patch".to_string(),
                scope: "session".to_string(),
            })
            .pop()
            .expect("approval resolved envelope");
        assert!(matches!(
            resolved.event,
            RuntimeEvent::ApprovalResolved {
                ref capability,
                ref scope,
                approved: true
            } if capability == "apply-patch" && scope == "session"
        ));

        let terminal = normalizer.normalize_ui_update(
            &UiUpdate::TurnComplete,
            Some(TurnEndContext {
                usage: Some(TokenUsageEnvelope {
                    input: 4,
                    output: 2,
                    estimated: false,
                }),
                changed_files: vec!["src/main.rs".to_string()],
            }),
        );
        assert_eq!(terminal.len(), 2);
        assert!(matches!(
            terminal[0].event,
            RuntimeEvent::AssistantMessage { ref content } if content == "hello"
        ));
        assert!(matches!(
            terminal[1].event,
            RuntimeEvent::TurnEnd {
                ref status,
                usage: Some(TokenUsageEnvelope { input: 4, output: 2, estimated: false }),
                ref changed_files,
            } if status == "completed" && changed_files == &vec!["src/main.rs".to_string()]
        ));
    }

    #[test]
    fn test_pi_12_runtime_handoff_round_trips_and_batch_derivation_hold() {
        let mut normalizer = RuntimeEnvelopeNormalizer::new("batch-1741700000000");
        let mut envelopes = Vec::new();

        envelopes.push(normalizer.start_turn(1, Some("inspect src/main.rs".to_string())));
        envelopes.extend(
            normalizer.normalize_ui_update(&UiUpdate::StreamDelta("hello ".to_string()), None),
        );
        envelopes.extend(
            normalizer.normalize_ui_update(&UiUpdate::StreamDelta("world".to_string()), None),
        );
        envelopes.extend(normalizer.normalize_stream_block(&StreamBlock::ToolCall {
            id: "provider-1".to_string(),
            name: "git_commit".to_string(),
            input: json!({"message":"test"}),
            status: crate::state::ToolStatus::Pending,
        }));
        envelopes.extend(normalizer.normalize_stream_block(&StreamBlock::ToolResult {
            tool_call_id: "provider-1".to_string(),
            output: "done".to_string(),
            is_error: false,
        }));
        envelopes.extend(normalizer.normalize_ui_update(
            &UiUpdate::TurnComplete,
            Some(TurnEndContext {
                usage: Some(TokenUsageEnvelope {
                    input: 10,
                    output: 5,
                    estimated: false,
                }),
                changed_files: vec![],
            }),
        ));

        envelopes.push(normalizer.start_turn(2, Some("second".to_string())));
        envelopes.push(RuntimeEnvelope {
            version: 1,
            task_id: "batch-1741700000000".to_string(),
            turn: 2,
            seq: 2,
            event: RuntimeEvent::AssistantDelta {
                text: "fallback".to_string(),
            },
        });
        envelopes.push(RuntimeEnvelope {
            version: 1,
            task_id: "batch-1741700000000".to_string(),
            turn: 2,
            seq: 3,
            event: RuntimeEvent::TurnEnd {
                status: "completed".to_string(),
                usage: None,
                changed_files: vec!["src/second.rs".to_string()],
            },
        });

        for envelope in &envelopes {
            let json = serde_json::to_string(envelope).expect("serialize envelope");
            let parsed: RuntimeEnvelope = serde_json::from_str(&json).expect("parse envelope");
            assert_eq!(&parsed, envelope);
        }

        let requests = vec![
            RuntimeRequest::SubmitInput {
                task_id: None,
                input: "go".to_string(),
            },
            RuntimeRequest::Interrupt {
                task_id: "batch-1741700000000".to_string(),
            },
            RuntimeRequest::ApproveCapability {
                task_id: "batch-1741700000000".to_string(),
                capability: "apply-patch".to_string(),
                scope: "session".to_string(),
            },
            RuntimeRequest::DenyCapability {
                task_id: "batch-1741700000000".to_string(),
                capability: "run-command".to_string(),
            },
        ];
        for request in requests {
            let json = serde_json::to_string(&request).expect("serialize request");
            let parsed: RuntimeRequest = serde_json::from_str(&json).expect("parse request");
            assert_eq!(parsed, request);
        }

        let turn_start_seqs = envelopes
            .iter()
            .filter_map(|envelope| match envelope.event {
                RuntimeEvent::TurnStart { .. } => Some((envelope.turn, envelope.seq)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(turn_start_seqs, vec![(1, 1), (2, 1)]);

        let derived = derive_batch_records(&envelopes, Some("AGENTS.md".to_string()));
        assert_eq!(derived.turns.len(), 2);
        assert_eq!(derived.turns[0].response, "hello world");
        assert_eq!(derived.turns[0].changed_files, Vec::<String>::new());
        assert_eq!(derived.turns[0].command_history.len(), 1);
        assert_eq!(derived.turns[0].tokens.input, 10);
        assert_eq!(derived.turns[0].tokens.output, 5);
        assert_eq!(derived.turns[1].response, "fallback");
        assert_eq!(
            derived.turns[1].changed_files,
            vec!["src/second.rs".to_string()]
        );
        assert_eq!(
            derived.summary.expect("summary").status,
            "completed".to_string()
        );
    }

    #[test]
    fn test_pi_12_error_and_max_turn_sequences_follow_contract() {
        let mut normalizer = RuntimeEnvelopeNormalizer::new("task-errors");
        let _ = normalizer.start_turn(1, Some("check".to_string()));

        let recoverable = normalizer.emit_error(
            "warning".to_string(),
            "retry".to_string(),
            true,
            TurnEndContext::default(),
        );
        assert_eq!(recoverable.len(), 1);
        assert!(matches!(
            recoverable[0].event,
            RuntimeEvent::Error {
                ref code,
                ref message,
                recoverable: true
            } if code == "warning" && message == "retry"
        ));

        let terminal = normalizer.emit_error(
            "fatal".to_string(),
            "boom".to_string(),
            false,
            TurnEndContext::default(),
        );
        assert_eq!(terminal.len(), 2);
        assert!(matches!(
            terminal[0].event,
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        assert!(matches!(
            terminal[1].event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "failed"
        ));

        let max_turns = normalizer.emit_max_turns_reached(3, TurnEndContext::default());
        assert!(matches!(
            max_turns[0].event,
            RuntimeEvent::MaxTurnsReached { max_turns: 3 }
        ));
        assert!(matches!(
            max_turns[1].event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "failed"
        ));
    }

    fn assert_runtime_tool_id(id: &str) {
        let parts: Vec<_> = id.split('_').collect();
        assert_eq!(parts.len(), 3, "runtime tool id must have three segments");
        assert_eq!(parts[0], "call");
        assert!(parts[1].chars().all(|ch| ch.is_ascii_digit()));
        assert_eq!(parts[2].len(), 4);
        assert!(parts[2].chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}
