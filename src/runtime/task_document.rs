mod condenser;
mod model;
mod task_state_bridge;

pub use condenser::{TaskDocumentCondenser, TaskMutationSummary};
pub use model::{
    ActiveTurnDocument, ApprovalDocument, AssistantBlockEntry, AssistantPhase,
    CommandSessionDocument, NoticeSeverity, PulseEntry, PulseOutcome, TaskDocument, TaskErrorState,
    TaskInfo, TurnDocument,
};

#[cfg(test)]
mod tests;
