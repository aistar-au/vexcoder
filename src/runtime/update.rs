use crate::state::{StreamBlock, ToolApprovalRequest};

pub enum UiUpdate {
    TranscriptLine(String),
    StreamDelta(String),
    StreamBlockStart { index: usize, block: StreamBlock },
    StreamBlockDelta { index: usize, delta: String },
    StreamBlockComplete { index: usize },
    ToolApprovalRequest(ToolApprovalRequest),
    TurnComplete,
    Error(String),
}
