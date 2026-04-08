mod condenser;
mod model;
mod task_state_bridge;

pub use condenser::{TaskDocumentCondenser, TaskMutationSummary};
pub use model::{
    ActiveTurnDocument, ApprovalDocument, AssistantBlockEntry, AssistantPhase,
    CommandSessionDocument, NoticeSeverity, TaskDocument, TaskErrorState, TaskMeta, TurnDocument,
    TurnEntry, TurnOutcome,
};

#[cfg(test)]
mod tests;
