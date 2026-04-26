use super::{RuntimeSignal, TokenUsageEnvelope};
use crate::pulse_evidence::TurnEvidenceRecord;
use crate::runtime::task_state::CommandEvidence;
use crate::state::ToolApprovalRequest;
use crate::usage::PulseTokens;
use indexmap::IndexMap;

pub fn token_usage_from_turn_tokens(tokens: PulseTokens) -> Option<TokenUsageEnvelope> {
    if tokens.is_zero() {
        None
    } else {
        Some(TokenUsageEnvelope {
            input: tokens.input,
            output: tokens.output,
            estimated: tokens.estimated,
            cache_creation_input: tokens.cache_creation_input_tokens,
            cache_read_input: tokens.cache_read_input_tokens,
        })
    }
}

pub(super) fn turn_tokens_from_usage(usage: &TokenUsageEnvelope) -> PulseTokens {
    PulseTokens {
        input: usage.input,
        output: usage.output,
        estimated: usage.estimated,
        cache_creation_input_tokens: usage.cache_creation_input,
        cache_read_input_tokens: usage.cache_read_input,
    }
}

pub fn approval_request_signal(request: &ToolApprovalRequest) -> RuntimeSignal {
    let capability = capability_name_for_tool(&request.tool_name).unwrap_or("unknown");
    RuntimeSignal::ApprovalRequest {
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
    pub(super) pulse: usize,
    pub(super) input: String,
    pub(super) transcript_response: String,
    pub(super) pending_final_text_blocks: IndexMap<usize, String>,
    pub(super) changed_files: Vec<String>,
    pub(super) command_history: Vec<CommandEvidence>,
    pub(super) tokens: PulseTokens,
}

impl DerivedTurnState {
    pub(super) fn start_final_text_block(&mut self, index: usize, content: String) {
        self.pending_final_text_blocks.insert(index, content);
    }

    pub(super) fn append_final_text_delta(&mut self, index: usize, delta: &str) {
        if let Some(content) = self.pending_final_text_blocks.get_mut(&index) {
            content.push_str(delta);
        }
    }

    pub(super) fn complete_final_text_block(&mut self, index: usize) {
        if let Some((_, content)) = self.pending_final_text_blocks.shift_remove_entry(&index) {
            self.transcript_response.push_str(&content);
        }
    }

    pub(super) fn flush_open_final_text_blocks(&mut self) {
        let pending = std::mem::take(&mut self.pending_final_text_blocks);
        for (_, content) in pending {
            self.transcript_response.push_str(&content);
        }
    }

    pub(super) fn into_record(mut self, instructions_path: Option<String>) -> TurnEvidenceRecord {
        self.flush_open_final_text_blocks();
        let response = self.transcript_response;

        TurnEvidenceRecord {
            pulse: self.pulse,
            input: self.input,
            response,
            instructions_path,
            changed_files: self.changed_files,
            command_history: self.command_history,
            tokens: self.tokens,
        }
    }
}
