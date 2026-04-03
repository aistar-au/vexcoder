mod conversation;
mod stream_block;
pub(crate) mod transcript_delta;

#[cfg(test)]
pub(crate) use conversation::{clear_codebase_index_for_tests, codebase_index_names_for_tests};
pub(crate) use conversation::{force_full_reindex_with_config, warm_codebase_index_with_config};
pub use conversation::{
    ConversationManager, ConversationStreamUpdate, ToolApprovalRequest, TurnToolPolicy,
    UndoCheckpoint,
};
pub use stream_block::{StreamBlock, ToolStatus};
pub(crate) use transcript_delta::{
    bounded_incremental_suffix, DeltaAccumulator, TranscriptBlockKind, TranscriptDelta,
};
