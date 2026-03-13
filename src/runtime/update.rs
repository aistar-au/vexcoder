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
}
