use super::{RuntimeEvent, TokenUsageEnvelope};
use crate::runtime::task_state::CommandEvidence;
use crate::state::ToolApprovalRequest;
use crate::turn_evidence::TurnEvidenceRecord;
use crate::usage::TurnTokens;

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

pub(super) fn turn_tokens_from_usage(usage: &TokenUsageEnvelope) -> TurnTokens {
    TurnTokens {
        input: usage.input,
        output: usage.output,
        estimated: usage.estimated,
        ..Default::default()
    }
}

pub fn runtime_approval_request_event(request: &ToolApprovalRequest) -> RuntimeEvent {
    let capability = capability_name_for_tool(&request.tool_name).unwrap_or("unknown");
    RuntimeEvent::ApprovalRequest {
        capability: capability.to_string(),
        scope: "once".to_string(),
        tool_name: Some(request.tool_name.clone()),
    }
}

fn capability_name_for_tool(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "read_file" | "list_files" | "list_directory" | "list_dir" | "glob_files" | "search"
        | "search_files" | "search_content" | "find_files" | "git_status" | "git_diff"
        | "git_log" | "git_show" => Some("read-file"),
        "write_file" | "edit_file" | "rename_file" => Some("write-file"),
        "apply_patch" | "git_add" | "git_commit" => Some("apply-patch"),
        "run_command" => Some("run-command"),
        _ => None,
    }
}

pub(super) fn empty_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[derive(Default)]
pub(super) struct DerivedTurnState {
    pub(super) turn: usize,
    pub(super) input: String,
    pub(super) delta_response: String,
    pub(super) assistant_message: Option<String>,
    pub(super) changed_files: Vec<String>,
    pub(super) command_history: Vec<CommandEvidence>,
    pub(super) tokens: TurnTokens,
}

impl DerivedTurnState {
    pub(super) fn into_record(self, instructions_path: Option<String>) -> TurnEvidenceRecord {
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