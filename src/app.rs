use crate::api::client::builtin_tool_summaries;
use crate::config::Config;
use crate::custom_commands::{load_custom_commands, CustomCommand};
use crate::mcp::McpRegistryRollup;
use crate::prompts::{
    render_custom_command_instruction, render_edit_prompt, render_explain_prompt,
    render_generate_tests_prompt, render_plan_prompt, render_review_prompt, CODER_SYSTEM_PROMPT,
};
use crate::runtime::context::RuntimeContext;
use crate::runtime::edit_loop::EditLoop;
use crate::runtime::frontend::{ScrollAction, ScrollTarget, UserInputEvent};
use crate::runtime::mode::RuntimeMode;
use crate::runtime::project_instructions::{load_project_instructions, LoadResult};
use crate::runtime::r#loop::Runtime;
use crate::runtime::task_state::SessionNote;
use crate::runtime::validation::ValidationSuite;
#[cfg(test)]
use crate::runtime::CommandResult;
#[cfg(test)]
use crate::runtime::TurnEntry;
use crate::runtime::{
    block_on_context_task, resolve_git_timeout_ms, run_git_command_with_timeout, AssembledContext,
    ContextAssembler,
};
use crate::runtime::{
    format_command_session_cancelled, format_command_session_exit, format_command_session_output,
    format_command_session_started, truncate_head_bytes, ApprovalScope, Capability, CommandRequest,
    CommandRunner, ConfiguredSandbox, DefaultCommandRunner, EditLoopOutcome, SandboxDriver,
    TaskDocument, TaskDocumentCondenser, TaskState, TaskStatus, TurnOutcome, UiUpdate,
};
#[cfg(test)]
use crate::session_notes::resolve_notes_for_injection;
use crate::session_notes::{
    build_api_client_with_notes, resolve_notes_path_for_read, resolve_notes_path_for_write,
};
#[cfg(test)]
use crate::state::StreamBlock;
use crate::state::{ConversationManager, ToolApprovalRequest, TurnToolPolicy};
use crate::tools::ToolOperator;
use crate::turn_evidence::note_changed_files_from_tool_call;
#[cfg(test)]
use crate::turn_evidence::ToolInvocationSummary;
use crate::types::ModelProfile;
use anyhow::Result;
#[cfg(test)]
use crossterm::event::{Event, KeyCode, KeyModifiers};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

mod accessors;
mod commands;
mod ctor;
mod errors;
mod facade;
mod inline;
mod input;
mod layout;
mod model_update;
mod overlay;
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
mod turn;
mod turn_start;
pub(crate) mod util;
pub use self::errors::{AppError, AppResult};
pub use self::facade::{
    build_facade_client, build_facade_runtime, execute_facade_runtime, run_tui_session,
    FacadeBootstrap,
};
pub use self::runtime_build::{build_runtime, build_runtime_with_resume};
pub use self::subtask_orchestrator::{JoinOutcome, SubtaskOrchestrator, TeamDecomposition};
pub use self::task_facade::{
    facade_delegate_session_task, facade_get_session_task, facade_list_agents,
    facade_list_session_tasks, facade_list_tasks, facade_list_todos, facade_poll_join,
    facade_release_session_task, facade_schedule_team, facade_task_graph,
    facade_update_session_task_status, facade_watch_rollup, task_graph_rollup_path,
    todos_rollup_path, write_projection_rollup, DelegateError, FacadeAgentDescriptor,
    FacadeAgentsListing, FacadeDelegateResult, FacadeJoinOutcome, FacadeScheduleTeamResult,
    FacadeSessionTaskRollup, FacadeTaskGraph, FacadeTaskGraphNode, FacadeTaskSummary,
    FacadeTeamDescriptor, FacadeTodoItem, FacadeWatchRollup, ScheduleTeamError,
    SessionTaskStatusError,
};

use self::overlay::summarize_tool_approval_context;
#[cfg(test)]
use self::overlay::{
    overlay_event_to_user_input, parse_approval_selection, render_pass_order, RenderPass,
};
#[cfg(test)]
use self::scroll::{input_rows_for_buffer, RenderGuard};
use self::util::{
    builtin_slash_command_names, capability_for_tool_name, format_inline_block, kebab_to_scope,
    list_recent_task_entries, new_task_id, parse_generate_tests_args, parse_review_args,
    resolve_repo_label, run_validation_suite_capture, sanitize_task_label, scope_to_label,
    shell_command_request,
};
pub use self::util::{capability_to_kebab, kebab_to_capability};

struct PendingApproval {
    step_id: Option<u64>,
    tool_name: String,
    input_preview: String,
    action: PendingApprovalAction,
}

enum PendingApprovalAction {
    Tool(tokio::sync::oneshot::Sender<bool>),
    InlineCommand(PendingInlineCommand),
}

struct PendingInlineCommand {
    command: String,
}

struct PendingPatchApproval {
    patch_preview: String,
    scroll_offset: usize,
    response_tx: Option<tokio::sync::oneshot::Sender<bool>>,
}

struct PendingResumeSelection {
    entries: Vec<ResumeTaskEntry>,
}

#[derive(Clone)]
struct ResumeTaskEntry {
    dir: PathBuf,
    id: String,
    status: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApprovalSelection {
    ApproveOnce,
    ApproveSession,
    Deny,
}

/// Fallback display column width when the host display width is unknown.
const DISPLAY_COLUMN_WIDTH_FALLBACK: usize = usize::MAX;

mod slash_commands;
use self::slash_commands::*;

#[derive(Debug, Default, PartialEq, Eq)]
struct GenerateTestsArgs {
    path: Option<String>,
    framework: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ReviewArgs {
    base: Option<String>,
    files: Option<String>,
    instruction: Option<String>,
}

#[derive(Default)]
struct OverlayState {
    pending_approval: Option<PendingApproval>,
    pending_patch_approval: Option<PendingPatchApproval>,
    pending_resume_selection: Option<PendingResumeSelection>,
    approved_tool_steps: std::collections::BTreeSet<u64>,
    auto_approve_session: bool,
    pending_memory_clear: bool,
}

/// Lifecycle state of a single orchestration step visible in the timeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepLifecycle {
    /// Tool call sent by the model, result not yet received.
    Running,
    /// Tool completed successfully.
    Completed,
    /// Tool completed with an error.
    Failed,
    /// Waiting for operator approval.
    AwaitingApproval,
    /// Operator approved the tool call; execution is proceeding.
    Approved,
    /// User prompt echo (not a tool step).
    UserInput,
    /// Active command session.
    CommandSession,
}

/// A single row in the orchestration timeline, derived from canonical task state.
#[derive(Clone, Debug)]
pub struct TimelineEntry {
    /// Monotonic identity that survives timeline re-derivation across frames.
    pub step_id: u64,
    pub lifecycle: StepLifecycle,
    pub label: String,
    /// Detail text shown in the inspector/output pane when this entry is selected.
    pub detail: String,
    /// Links command-session entries to their [`CommandSessionState`].
    pub session_id: Option<u64>,
}

/// Scroll semantics for the output pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputScrollAnchor {
    /// Scroll offsets count from the first visible row.
    Top,
    /// Scroll offsets count upward from the prompt/composer edge.
    #[default]
    Bottom,
}

#[derive(Clone, Debug, Default)]
pub struct TaskLayoutState {
    pub task_id: String,
    pub status_line: String,
    pub telemetry: TaskTelemetryState,
    /// Structured timeline entries derived from canonical task state.
    pub timeline_entries: Vec<TimelineEntry>,
    /// Index of the selected timeline entry (for inspector focus).
    pub selected_step: usize,
    /// Total number of steps (including those scrolled out of view).
    pub total_steps: usize,
    /// Human-readable title for the output pane.
    pub output_title: String,
    pub output_rows: Vec<TranscriptRow>,
    /// Scroll amount for the output pane, interpreted using `output_scroll_anchor`.
    pub output_scroll_offset: usize,
    pub output_scroll_anchor: OutputScrollAnchor,
    pub pending_approval: Option<String>,
    /// Active composer buffer for the fullscreen task surface.
    pub composer_text: String,
    /// Cursor byte offset within `composer_text`.
    pub composer_cursor: usize,
    /// Whether the composer should render as the active focus target.
    pub composer_focused: bool,
    pub changed_files: Vec<String>,
    /// When true the timeline auto-advances to the latest entry.
    pub follow_mode: bool,
    /// Floating picker overlay rendered above the composer when a picker is active.
    pub picker_overlay: Vec<PickerOverlayLine>,
    /// Workspace working directory displayed at the prompt separator.
    pub working_dir: String,
    /// Model API endpoint URL shown at the prompt separator.
    pub model_url: String,
}

/// Minimal projection of task state consumed exclusively by the renderer.
///
/// Contains only the fields that `render_task_layout` and its helpers in
/// `src/ui/render/` actually read.  The full `TaskLayoutState` (with
/// telemetry, timeline, inspector fields etc.) remains available for tests
/// and the activity-pane inspector, but is never passed into the render path.
#[derive(Clone, Debug, Default)]
pub struct TaskViewProjection {
    pub status_line: String,
    pub output_rows: Vec<TranscriptRow>,
    pub output_scroll_offset: usize,
    pub output_scroll_anchor: OutputScrollAnchor,
    pub composer_text: String,
    pub composer_cursor: usize,
    pub composer_focused: bool,
    pub picker_overlay: Vec<PickerOverlayLine>,
}

impl TaskLayoutState {
    /// Extract the renderer-facing subset into a `TaskViewProjection`.
    pub fn into_view_projection(self) -> TaskViewProjection {
        TaskViewProjection {
            status_line: self.status_line,
            output_rows: self.output_rows,
            output_scroll_offset: self.output_scroll_offset,
            output_scroll_anchor: self.output_scroll_anchor,
            composer_text: self.composer_text,
            composer_cursor: self.composer_cursor,
            composer_focused: self.composer_focused,
            picker_overlay: self.picker_overlay,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskContextSummaryState {
    pub file_rollups: usize,
    pub related_paths: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub git_context_included: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskTelemetryState {
    pub mode: String,
    pub approval: String,
    pub model_name: String,
    pub model_backend: Option<crate::runtime::ModelBackendKind>,
    pub sandbox_kind: Option<crate::runtime::SandboxKind>,
    pub context_summary: Option<TaskContextSummaryState>,
    pub history_rows: usize,
    pub total_tokens: u64,
    /// Cumulative input (prompt) tokens sent across all turns.
    pub tokens_sent: u64,
    /// Cumulative output (completion) tokens received across all turns.
    pub tokens_received: u64,
    pub active_tools: usize,
    pub active_commands: usize,
    pub waiting_summary: Option<String>,
    pub timing_summary: Option<String>,
    /// Current git branch name (empty when not in a git repository).
    pub git_branch: String,
}

/// A single line in the floating picker overlay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickerOverlayLine {
    pub text: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMentionPickerState {
    pub range: Range<usize>,
    pub prefix: String,
    pub matches: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashPickerMatch {
    /// Command text inserted on selection (e.g. "/edit ").
    pub command: String,
    /// Display label shown in the picker (e.g. "/edit <instruction> · start an edit loop").
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashPickerState {
    pub prefix: String,
    pub matches: Vec<SlashPickerMatch>,
}

pub struct TuiMode {
    // ── Overlay / approval state ──────────────────────────────────────────
    overlay_state: OverlayState,
    // ── Session metadata (set once at startup, rarely changed) ────────────
    repo_label: String,
    git_branch: String,
    instructions_path: Option<String>,
    mcp_rollup: Option<McpRegistryRollup>,
    /// Effective display column width used for word-wrap scroll math.
    display_column_width: Cell<usize>,
    // ── Persistence / quit flow ───────────────────────────────────────────
    pending_quit: bool,
    quit_requested: bool,
    notes_path: Option<PathBuf>,
    // ── Model config ──────────────────────────────────────────────────────
    model_name: String,
    model_backend: crate::runtime::ModelBackendKind,
    model_profile: ModelProfile,
    working_dir: PathBuf,
    model_url: String,
    search_config: crate::config::SearchConfig,
    sandbox: ConfiguredSandbox,
    file_prompt_entries: RefCell<Option<Vec<String>>>,
    custom_commands: Vec<CustomCommand>,
    last_assembled_context: Option<AssembledContext>,
    // ── Canonical task document ───────────────────────────────────────────
    /// Single source of truth for all task state: turns, entries, session
    /// grants, and metadata.  Replaces the legacy `TaskState` + per-turn
    /// transcript buffers.
    task_doc: TaskDocument,
    task_doc_condenser: TaskDocumentCondenser,
    /// System notices that arrived before the first turn opened (e.g. notes
    /// warnings, sandbox state).  Shown at the top of the transcript.
    pre_session_notices: Vec<String>,
    /// Raw partial-JSON accumulator for streaming tool-call input fragments,
    /// keyed by block index.  Cleared when the block completes or turn ends.
    streaming_tool_input_buffers: std::collections::HashMap<usize, String>,
    /// Set to `true` once a `StreamBlockDelta` updates a Final-phase
    /// assistant block, so that flat `StreamDelta` events are skipped to
    /// avoid double-counting the same content.
    stream_uses_block_deltas: bool,
    // ── Turn lifecycle flags ───────────────────────────────────────────────
    read_only_turn_active: bool,
    active_edit_loop: Option<EditLoop>,
    // ── Timeline viewport ─────────────────────────────────────────────────
    selected_timeline_index: usize,
    /// When true, selection auto-advances to the latest timeline entry.
    timeline_follow_mode: bool,
    transcript_scroll_offset: usize,
    inspector_scroll_offset: usize,
    // ── Turn telemetry (wall-clock; not persisted in task_doc) ────────────
    turn_started_at: Option<Instant>,
    /// Client-side time-to-first-token for the active turn.
    ttft: Option<Duration>,
    /// TTFT from the most recently completed turn (kept for display).
    last_turn_ttft: Option<Duration>,
    /// Duration of the most recently completed or failed turn.
    last_turn_duration: Option<Duration>,
    /// Last visible error message for the task surface.
    last_error_message: Option<String>,
    // ── Turn flow control ─────────────────────────────────────────────────
    /// Buffered turn-completion event waiting for command sessions to drain.
    turn_completion_pending: bool,
    /// Tracks whether the current turn is a `/plan` command.
    plan_turn_active: bool,
    // ── Auto-memory config ────────────────────────────────────────────────
    #[cfg(not(test))]
    auto_memory_enabled: bool,
    #[cfg(test)]
    pub auto_memory_enabled: bool,
    /// Maximum notes to extract per turn (from config).
    auto_memory_max_notes: usize,
    #[cfg(test)]
    pub last_turn_input: Option<String>,
}

/// All capabilities in stable kebab order (used for /permissions display and round-trip tests).
pub const ALL_CAPABILITIES: &[Capability] = &[
    Capability::ApplyPatch,
    Capability::Browser,
    Capability::McpTool,
    Capability::Network,
    Capability::ReadFile,
    Capability::RunCommand,
    Capability::WriteFile,
];

#[cfg(test)]
async fn run_shell_command_with_runner<R, S>(
    runner: R,
    sandbox: S,
    command: String,
    working_dir: PathBuf,
) -> Result<CommandResult>
where
    R: CommandRunner,
    S: SandboxDriver,
{
    let request = sandbox.wrap(shell_command_request(command, working_dir))?;
    runner.run_one_shot(request).await
}

impl Default for TuiMode {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeMode for TuiMode {
    fn on_frontend_event(&mut self, event: UserInputEvent, ctx: &mut RuntimeContext) {
        match event {
            UserInputEvent::Text(input) => self.on_user_input(input, ctx),
            UserInputEvent::Interrupt => self.on_interrupt(ctx),
            UserInputEvent::Scroll { target, action } => {
                if self.overlay_active() {
                    if target == ScrollTarget::Overlay {
                        self.apply_patch_overlay_scroll_action(action);
                    }
                } else if target == ScrollTarget::Timeline {
                    let total = self.timeline_entry_count();
                    self.apply_timeline_scroll_action(action, total);
                } else if target == ScrollTarget::Output {
                    self.apply_output_scroll_action(action);
                }
            }
        }
    }

    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        TuiMode::on_user_input(self, input, ctx);
    }

    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        TuiMode::on_model_update(self, update, ctx);
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        TuiMode::on_interrupt(self, ctx);
    }

    fn is_turn_in_progress(&self) -> bool {
        self.task_doc.active_turn.is_some()
    }
}
