use crate::api::client::builtin_tool_summaries;
use crate::config::Config;
use crate::custom_commands::{CustomCommand, load_custom_commands};
use crate::mcp::McpRegistryRollup;
use crate::prompts::{
    CODER_SYSTEM_PROMPT, render_custom_command_instruction, render_edit_prompt,
    render_explain_prompt, render_generate_tests_prompt, render_plan_prompt, render_review_prompt,
};
#[cfg(test)]
use crate::runtime::CommandResult;
#[cfg(test)]
use crate::runtime::TurnEntry;
use crate::runtime::context::RuntimeContext;
use crate::runtime::edit_loop::EditLoop;
use crate::runtime::frontend::{ScrollAction, ScrollTarget, UserInputEvent};
use crate::runtime::r#loop::Runtime;
use crate::runtime::mode::RuntimeMode;
use crate::runtime::project_instructions::{LoadResult, load_project_instructions};
use crate::runtime::task_state::SessionNote;
use crate::runtime::tokio::sync::{mpsc, oneshot};
use crate::runtime::validation::ValidationSuite;
use crate::runtime::{
    ApprovalScope, Capability, CommandRequest, CommandRunner, ConfiguredSandbox,
    DefaultCommandRunner, EditLoopOutcome, SandboxDriver, TaskDocument, TaskDocumentCondenser,
    TaskState, TaskStatus, TurnOutcome, format_command_session_cancelled,
    format_command_session_exit, format_command_session_output, format_command_session_started,
    truncate_head_bytes,
};
use crate::runtime::{
    AssembledContext, ContextAssembler, block_on_context_task, resolve_git_timeout_ms,
    run_git_command_with_timeout,
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
#[cfg(test)]
use crate::turn_evidence::ToolInvocationSummary;
use crate::turn_evidence::note_changed_files_from_tool_call;
use crate::types::ModelProfile;
#[cfg(test)]
use crate::ui::tui::event::{Event, KeyCode, KeyModifiers};
use anyhow::Result;
use std::cell::{Cell, RefCell};
use std::io::Write;
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
mod turn;
mod turn_start;
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
    facade_watch_rollup, task_graph_rollup_path, todos_rollup_path, write_projection_rollup,
};
pub use crate::runtime::UiUpdate;
pub use crate::runtime::tokio as runtime_tokio;

use self::overlay::summarize_tool_approval_context;
#[cfg(test)]
use self::overlay::{
    RenderPass, overlay_event_to_user_input, parse_approval_selection, render_pass_order,
};
#[cfg(test)]
use self::scroll::{RenderGuard, input_rows_for_buffer};
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
    Tool(oneshot::Sender<bool>),
    InlineCommand(PendingInlineCommand),
}

struct PendingInlineCommand {
    command: String,
}

struct PendingPatchApproval {
    patch_preview: String,
    scroll_offset: usize,
    response_tx: Option<oneshot::Sender<bool>>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepLifecycle {
    Running,

    Completed,

    Failed,

    AwaitingApproval,

    Approved,

    UserInput,

    CommandSession,
}

#[derive(Clone, Debug)]
pub struct TimelineEntry {
    pub step_id: u64,
    pub lifecycle: StepLifecycle,
    pub label: String,

    pub detail: String,

    pub session_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputScrollAnchor {
    Top,

    #[default]
    Bottom,
}

#[derive(Clone, Debug, Default)]
pub struct TaskLayoutState {
    pub task_id: String,
    pub status_line: String,
    pub telemetry: TaskTelemetryState,

    pub timeline_entries: Vec<TimelineEntry>,

    pub selected_step: usize,

    pub total_steps: usize,

    pub output_title: String,
    pub output_rows: Vec<TranscriptRow>,

    pub output_scroll_offset: usize,
    pub output_scroll_anchor: OutputScrollAnchor,
    pub pending_approval: Option<String>,

    pub composer_text: String,

    pub composer_cursor: usize,

    pub composer_focused: bool,
    pub changed_files: Vec<String>,

    pub follow_mode: bool,

    pub picker_overlay: Vec<PickerOverlayLine>,

    pub working_dir: String,

    pub model_url: String,
}

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

    pub tokens_sent: u64,

    pub tokens_received: u64,
    pub active_tools: usize,
    pub active_commands: usize,
    pub waiting_summary: Option<String>,
    pub timing_summary: Option<String>,

    pub git_branch: String,
}

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
    pub total_matches: usize,
}

pub(crate) fn file_picker_match_summary(
    prefix: &str,
    visible_count: usize,
    total_matches: usize,
) -> String {
    if let Some((dir_prefix, is_filtered)) = directory_picker_context(prefix) {
        if is_filtered && visible_count < total_matches {
            format!("[file] {visible_count} shown of {total_matches} in {dir_prefix}")
        } else {
            format!("[file] {total_matches} item(s) in {dir_prefix}")
        }
    } else if visible_count < total_matches {
        format!("[file] {visible_count} of {total_matches} match(es)")
    } else {
        format!("[file] {visible_count} match(es)")
    }
}

pub(crate) fn file_picker_no_match_summary(prefix: &str, total_matches: usize) -> String {
    if let Some((dir_prefix, is_filtered)) = directory_picker_context(prefix) {
        if is_filtered && total_matches > 0 {
            format!("[file] no matches for {prefix} - {total_matches} in {dir_prefix}")
        } else {
            format!("[file] no matches for {prefix}")
        }
    } else {
        format!("[file] no matches for {prefix}")
    }
}

fn directory_picker_context(prefix: &str) -> Option<(String, bool)> {
    let normalized = prefix.replace('\\', "/");
    let slash_pos = normalized.rfind('/')?;
    let dir_prefix = normalized[..=slash_pos].to_string();
    let is_filtered = slash_pos + 1 < normalized.len();
    Some((dir_prefix, is_filtered))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashPickerMatch {
    pub command: String,

    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashPickerState {
    pub prefix: String,
    pub matches: Vec<SlashPickerMatch>,
}

pub struct TuiMode {
    overlay_state: OverlayState,

    repo_label: String,
    git_branch: String,
    instructions_path: Option<String>,
    mcp_rollup: Option<McpRegistryRollup>,

    display_column_width: Cell<usize>,

    pending_quit: bool,
    quit_requested: bool,
    notes_path: Option<PathBuf>,

    model_name: String,
    model_backend: crate::runtime::ModelBackendKind,
    model_profile: ModelProfile,
    working_dir: PathBuf,
    model_url: String,
    search_config: crate::config::SearchConfig,
    context_assembler: ContextAssembler,
    sandbox: ConfiguredSandbox,
    file_prompt_entries: RefCell<Option<Vec<String>>>,
    custom_commands: Vec<CustomCommand>,
    last_assembled_context: Option<AssembledContext>,

    task_doc: TaskDocument,
    task_doc_condenser: TaskDocumentCondenser,

    pre_session_notices: Vec<String>,

    stream_uses_structured_final_output: bool,

    read_only_turn_active: bool,
    active_edit_loop: Option<EditLoop>,

    selected_timeline_index: usize,

    timeline_follow_mode: bool,
    transcript_scroll_offset: usize,
    inspector_scroll_offset: usize,

    turn_started_at: Option<Instant>,

    ttft: Option<Duration>,

    last_turn_ttft: Option<Duration>,

    last_turn_duration: Option<Duration>,

    last_error_message: Option<String>,

    turn_completion_pending: bool,

    plan_turn_active: bool,

    #[cfg(not(test))]
    auto_memory_enabled: bool,
    #[cfg(test)]
    pub auto_memory_enabled: bool,

    auto_memory_max_notes: usize,
    #[cfg(test)]
    pub last_turn_input: Option<String>,
}

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
