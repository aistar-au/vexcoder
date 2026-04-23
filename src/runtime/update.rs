use super::edit_loop::EditLoopOutcome;
use super::validation::ValidationResult;
use crate::state::{StreamBlock, ToolApprovalRequest};
use crate::types::StreamChunkMetadata;

pub enum UiUpdate {
    TranscriptLine(String),
    StreamDelta(String),
    StreamBlockStart {
        index: usize,
        block: StreamBlock,
    },
    StreamBlockDelta {
        index: usize,
        delta: String,
    },
    ToolCallArgumentsUpdated {
        tool_call_id: String,
        tool_name: Option<String>,
        arguments: serde_json::Value,
    },
    StreamBlockComplete {
        index: usize,
    },
    ToolApprovalRequest(ToolApprovalRequest),
    ServerMetadata(Box<StreamChunkMetadata>),
    CommandSessionStarted {
        session_id: u64,
        command: String,
    },
    CommandSessionAttached {
        session_id: u64,
        pid: Option<u32>,
    },
    EditLoopComplete {
        outcome: EditLoopOutcome,
        last_validation_result: Option<ValidationResult>,
    },
    CommandSessionFinished {
        session_id: u64,
    },
    TurnComplete,
    Error(String),

    ContextCompacted {
        messages_before: usize,
        messages_after: usize,
        summary: String,
    },
}
