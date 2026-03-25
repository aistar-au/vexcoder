use super::edit_loop::EditLoopOutcome;
use super::validation::ValidationResult;
use crate::state::{StreamBlock, ToolApprovalRequest};

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
    StreamBlockComplete {
        index: usize,
    },
    ToolApprovalRequest(ToolApprovalRequest),
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
    /// Conversation history was compacted (ADR-029 session persistence).
    ContextCompacted {
        messages_before: usize,
        messages_after: usize,
        summary: String,
    },
}
