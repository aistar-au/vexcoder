use crate::api::client::builtin_tool_summaries;
use crate::config::Config;
use crate::custom_commands::{CustomCommand, load_custom_commands};
use crate::mcp::McpRegistryRollup;
use crate::prompts::{
    CODER_SYSTEM_PROMPT, render_custom_command_instruction, render_edit_prompt,
    render_explain_prompt, render_generate_tests_prompt, render_plan_prompt, render_review_prompt,
};
#[cfg(test)]
use crate::pulse_evidence::ToolInvocationSummary;
use crate::pulse_evidence::note_changed_files_from_tool_call;
#[cfg(test)]
use crate::runtime::CommandResult;
#[cfg(test)]
use crate::runtime::PulseEntry;
use crate::runtime::context::RuntimeContext;
use crate::runtime::edit_loop::EditLoop;
use crate::runtime::frontend::{InputOccurrence, ScrollAction, ScrollTarget};
use crate::runtime::r#loop::Runtime;
use crate::runtime::mode::RuntimeMode;
use crate::runtime::project_instructions::InstructionSource;
use crate::runtime::task_state::SessionNote;
use crate::runtime::tokio::sync::{mpsc, oneshot};
use crate::runtime::validation::ValidationSuite;
use crate::runtime::{
    ApprovalScope, Capability, CommandRequest, CommandRunner, ConfiguredSandbox,
    DefaultCommandRunner, EditLoopOutcome, PulseOutcome, SandboxDriver, TaskDocument,
    TaskDocumentCondenser, TaskState, TaskStatus, WorkingSetRecord,
    format_command_session_cancelled, format_command_session_exit, format_command_session_output,
    format_command_session_started, truncate_head_bytes,
};
use crate::runtime::{
    AssembledContext, ContextAssembler, block_on_context_task, resolve_git_timeout_ms,
    run_git_command_with_timeout,
};
use crate::session_notes::{
    build_api_client_with_notes, resolve_notes_path_for_read, resolve_notes_path_for_write,
};
#[cfg(test)]
use crate::state::StreamBlock;
use crate::state::{ConversationManager, PulseToolPolicy, ToolApprovalRequest};
use crate::tools::ToolOperator;
use crate::types::ModelProfile;
#[cfg(test)]
use crate::ui::tui::input::{KeyCode, KeyModifiers};
use anyhow::Result;
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

mod commands;
mod ctor;
mod errors;
mod facade;
mod inline;
mod input;
mod layout;
mod model_update;
mod overlay;
mod queries;
mod runtime_build;
mod scroll;
mod shell;
pub(crate) mod subtask_orchestrator;
pub(crate) mod task_facade;
#[cfg(test)]
mod tests;
mod transcript_projection;
pub mod transcript_row;
pub use transcript_row::TranscriptRow;
mod pulse;
mod pulse_start;
pub(crate) mod util;
pub use self::errors::{AppError, AppResult};
pub use self::facade::{
    FacadeBootstrap, build_facade_client, build_facade_runtime, execute_facade_runtime,
    run_tui_session,
};
pub use self::runtime_build::{build_runtime, build_runtime_with_resume};
pub use self::subtask_orchestrator::{JoinOutcome, SubtaskOrchestrator, TeamDecomposition};
pub use self::task_facade::{
    DelegateError, FacadeAgentDescriptor, FacadeAgentsListing, FacadeDelegateResult,
    FacadeJoinOutcome, FacadeScheduleTeamResult, FacadeSessionTaskRollup, FacadeTaskGraph,
    FacadeTaskGraphNode, FacadeTaskSummary, FacadeTeamDescriptor, FacadeTodoItem,
    FacadeWatchRollup, PeerChannelError, ScheduleTeamError, SessionTaskStatusError,
    facade_delegate_session_task, facade_get_session_task, facade_list_agents,
    facade_list_session_tasks, facade_list_tasks, facade_list_todos, facade_poll_join,
    facade_post_peer_message, facade_read_peer_messages, facade_release_session_task,
    facade_schedule_team, facade_task_graph, facade_update_session_task_status,
    facade_watch_rollup, facade_working_set, task_graph_rollup_path, todos_rollup_path,
    write_projection_rollup,
};
pub use crate::runtime::UiUpdate;
pub use crate::runtime::task_state::peer_channel::PeerMessage;
pub use crate::runtime::tokio as runtime_tokio;
pub use crate::runtime::{JoinIndex, StateEnvelope, TaskState, WorkingSetRecord};
