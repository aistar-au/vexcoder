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
    EditLoopComplete {
        outcome: EditLoopOutcome,
        last_validation_result: Option<ValidationResult>,
    },
    PassthroughCommandFinished,
    TurnComplete,
    Error(String),
}
