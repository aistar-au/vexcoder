use crate::api::client::builtin_tool_summaries;
use crate::config::Config;
use crate::custom_commands::{load_custom_commands, CustomCommand};
use crate::mcp::McpRegistrySnapshot;
use crate::prompts::{
    render_custom_command_instruction, render_edit_prompt, render_explain_prompt,
    render_generate_tests_prompt, render_plan_prompt, render_review_prompt, CODER_SYSTEM_PROMPT,
};
use crate::runtime::context::RuntimeContext;
use crate::runtime::context_assembler::{
    block_on_context_task, resolve_git_timeout_ms, run_git_command_with_timeout, AssembledContext,
    ContextAssembler,
};
use crate::runtime::edit_loop::EditLoop;
use crate::runtime::frontend::{ScrollAction, ScrollTarget, UserInputEvent};
use crate::runtime::mode::RuntimeMode;
use crate::runtime::policy::sanitize_assistant_text;
use crate::runtime::project_instructions::{load_project_instructions, LoadResult};
use crate::runtime::r#loop::Runtime;
use crate::runtime::task_state::SessionNote;
use crate::runtime::validation::ValidationSuite;
#[cfg(test)]
use crate::runtime::CommandResult;
use crate::runtime::{
    format_command_session_cancelled, format_command_session_exit, format_command_session_output,
    format_command_session_started, truncate_head_bytes, ApprovalScope, Capability, CommandRequest,
    CommandRunner, ConfiguredSandbox, DefaultCommandRunner, EditLoopOutcome, SandboxDriver,
    TaskState, TaskStatus, UiUpdate,
};
#[cfg(test)]
use crate::session_notes::resolve_notes_for_injection;
use crate::session_notes::{
    build_api_client_with_notes, resolve_notes_path_for_read, resolve_notes_path_for_write,
};
use crate::state::{ConversationManager, StreamBlock, ToolApprovalRequest, TurnToolPolicy};
use crate::tool_preview::{preview_tool_input, ToolPreviewStyle};
use crate::tools::ToolOperator;
use crate::turn_evidence::{
    command_evidence_from_tool_result, note_changed_files_from_tool_call, ToolInvocationSummary,
    TurnEvidenceState,
};
use crate::types::ModelProfile;
use crate::ui::render::history_visual_line_count;
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
    facade_update_session_task_status, facade_watch_snapshot, task_graph_snapshot_path,
    todos_snapshot_path, write_projection_snapshot, DelegateError, FacadeAgentDescriptor,
    FacadeAgentsListing, FacadeDelegateResult, FacadeJoinOutcome, FacadeScheduleTeamResult,
    FacadeSessionTaskSnapshot, FacadeTaskGraph, FacadeTaskGraphNode, FacadeTaskSummary,
    FacadeTeamDescriptor, FacadeTodoItem, FacadeWatchSnapshot, ScheduleTeamError,
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
    resolve_history_line_cap, resolve_repo_label, run_validation_suite_capture,
    sanitize_task_label, scope_to_label, shell_command_request, summarize_tool_outcome,
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

#[derive(Clone)]
struct PendingTurnToolCall {
    step_id: u64,
    name: String,
    input_preview: String,
    input: serde_json::Value,
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

// Interactive sessions keep terminal-style scrollback by default. Bounding
// memory is future work for a paged or file-backed transcript store rather than
// default truncation of the live session history.
const DEFAULT_MAX_HISTORY_LINES: usize = usize::MAX;
const MAX_HISTORY_LINES_ENV: &str = "VEX_MAX_HISTORY_LINES";
const HISTORY_CONTENT_WIDTH_FALLBACK: usize = usize::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlashCommandId {
    Quit,
    Exit,
    About,
    MemoryShow,
    MemoryAdd,
    MemoryClear,
    New,
    Resume,
    Compact,
    Fork,
    Permissions,
    Allow,
    Deny,
    Model,
    Diff,
    Edit,
    Fix,
    Explain,
    Review,
    Plan,
    Init,
    Run,
    Test,
    Context,
    Mcp,
    Tools,
    Usage,
    GenerateTests,
    Agents,
    Delegate,
    Watch,
    Undo,
    Commands,
    Help,
}

#[derive(Clone, Copy, Debug)]
enum SlashCommandPattern {
    Exact(&'static str),
    ExactOrPrefix {
        exact: &'static str,
        prefix: &'static str,
    },
}

impl SlashCommandPattern {
    fn parse<'a>(&self, input: &'a str) -> Option<&'a str> {
        match self {
            SlashCommandPattern::Exact(command) => (input == *command).then_some(""),
            SlashCommandPattern::ExactOrPrefix { exact, prefix } => {
                if input == *exact {
                    Some("")
                } else {
                    input.strip_prefix(prefix).map(str::trim)
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SlashCommandSpec {
    id: SlashCommandId,
    pattern: SlashCommandPattern,
    display: &'static str,
    description: &'static str,
}

impl SlashCommandSpec {
    const fn new(
        id: SlashCommandId,
        pattern: SlashCommandPattern,
        display: &'static str,
        description: &'static str,
    ) -> Self {
        assert!(
            !display.is_empty(),
            "slash command display must not be empty"
        );
        assert!(
            !description.is_empty(),
            "slash command description must not be empty"
        );
        Self {
            id,
            pattern,
            display,
            description,
        }
    }
}

const SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec::new(
        SlashCommandId::Edit,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/edit",
            prefix: "/edit ",
        },
        "/edit <instruction>",
        "start an edit loop",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Fix,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/fix",
            prefix: "/fix ",
        },
        "/fix",
        "re-run edit loop from last error",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Explain,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/explain",
            prefix: "/explain ",
        },
        "/explain [path]",
        "explain a file or region; no patch",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Review,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/review",
            prefix: "/review ",
        },
        "/review [--base <git-ref>] [--files <glob>] [<instruction>]",
        "review a diff or file set; no patch",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Plan,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/plan",
            prefix: "/plan ",
        },
        "/plan <instruction>",
        "generate an implementation plan; no patch",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Init,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/init",
            prefix: "/init ",
        },
        "/init [environment]",
        "scaffold .vex files for the current workspace",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Run,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/run",
            prefix: "/run ",
        },
        "/run [command]",
        "run a command; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Test,
        SlashCommandPattern::Exact("/test"),
        "/test",
        "run full validation suite; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Context,
        SlashCommandPattern::Exact("/context"),
        "/context",
        "show session context summary; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Mcp,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/mcp",
            prefix: "/mcp ",
        },
        "/mcp [list|show <server>]",
        "show loaded MCP servers and tools; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Tools,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/tools",
            prefix: "/tools ",
        },
        "/tools [desc]",
        "show live tool registry; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Usage,
        SlashCommandPattern::Exact("/usage"),
        "/usage",
        "show session token usage; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::GenerateTests,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/generate-tests",
            prefix: "/generate-tests ",
        },
        "/generate-tests [path] [--framework <name>]",
        "generate tests for a path; test files only",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Model,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/model",
            prefix: "/model ",
        },
        "/model [<n>]",
        "show or switch active model name",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Permissions,
        SlashCommandPattern::Exact("/permissions"),
        "/permissions",
        "show active capability grants",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Allow,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/allow",
            prefix: "/allow ",
        },
        "/allow <cap> [once|session]",
        "grant a capability",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Deny,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/deny",
            prefix: "/deny ",
        },
        "/deny <cap>",
        "revoke a capability",
    ),
    SlashCommandSpec::new(
        SlashCommandId::New,
        SlashCommandPattern::Exact("/new"),
        "/new",
        "save and reset session",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Resume,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/resume",
            prefix: "/resume ",
        },
        "/resume [<task-id>]",
        "resume a saved session",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Compact,
        SlashCommandPattern::Exact("/compact"),
        "/compact",
        "compact conversation history (keep task)",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Fork,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/fork",
            prefix: "/fork ",
        },
        "/fork [<label>]",
        "fork current session to new task-id",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryShow,
        SlashCommandPattern::Exact("/memory"),
        "/memory [add <note>|clear]",
        "view or edit persistent user notes",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryAdd,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/memory add",
            prefix: "/memory add ",
        },
        "/memory [add <note>|clear]",
        "view or edit persistent user notes",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryClear,
        SlashCommandPattern::Exact("/memory clear"),
        "/memory [add <note>|clear]",
        "view or edit persistent user notes",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Agents,
        SlashCommandPattern::Exact("/agents"),
        "/agents",
        "show configured agents and teams",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Delegate,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/delegate",
            prefix: "/delegate ",
        },
        "/delegate <agent> <prompt>",
        "create a session task for a configured agent",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Watch,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/watch",
            prefix: "/watch ",
        },
        "/watch [task-id|agent-id]",
        "show session-task status for the current repo",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Undo,
        SlashCommandPattern::Exact("/undo"),
        "/undo",
        "undo last file-modifying tool call",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Commands,
        SlashCommandPattern::Exact("/commands"),
        "/commands",
        "show this list",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Help,
        SlashCommandPattern::Exact("/help"),
        "/help",
        "alias for /commands",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Quit,
        SlashCommandPattern::Exact("/quit"),
        "/quit",
        "save and exit",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Exit,
        SlashCommandPattern::Exact("/exit"),
        "/exit",
        "alias for /quit",
    ),
    SlashCommandSpec::new(
        SlashCommandId::About,
        SlashCommandPattern::Exact("/about"),
        "/about",
        "show version and build info",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Diff,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/diff",
            prefix: "/diff ",
        },
        "/diff [--staged]",
        "show working-tree diff; no model turn",
    ),
];

fn slash_command_menu_group(id: SlashCommandId) -> &'static str {
    match id {
        SlashCommandId::Plan
        | SlashCommandId::Explain
        | SlashCommandId::Review
        | SlashCommandId::Context
        | SlashCommandId::Mcp
        | SlashCommandId::Tools
        | SlashCommandId::GenerateTests
        | SlashCommandId::Agents
        | SlashCommandId::Watch => "retrieve + context",
        SlashCommandId::Edit | SlashCommandId::Fix | SlashCommandId::Diff => "edit + inspect",
        SlashCommandId::Run | SlashCommandId::Test | SlashCommandId::Delegate => {
            "validate + execute"
        }
        SlashCommandId::Init
        | SlashCommandId::Model
        | SlashCommandId::Permissions
        | SlashCommandId::Allow
        | SlashCommandId::Deny
        | SlashCommandId::Usage
        | SlashCommandId::Commands
        | SlashCommandId::Help
        | SlashCommandId::MemoryShow
        | SlashCommandId::MemoryAdd
        | SlashCommandId::MemoryClear
        | SlashCommandId::New
        | SlashCommandId::Resume
        | SlashCommandId::Compact
        | SlashCommandId::Fork
        | SlashCommandId::Quit
        | SlashCommandId::Exit
        | SlashCommandId::About => "session + control",
        SlashCommandId::Undo => "edit + inspect",
    }
}

fn slash_command_mode_summary(id: SlashCommandId) -> &'static str {
    match id {
        SlashCommandId::Plan => "read-only plan from current repo context",
        SlashCommandId::Explain => "read-only explanation with context assembly",
        SlashCommandId::Review => "read-only review over assembled context",
        SlashCommandId::Context => "session status, git state, and token summary",
        SlashCommandId::Mcp => "inspect loaded MCP servers and per-server tool inventory",
        SlashCommandId::Tools => "tool directory plus retrieval workflow guidance",
        SlashCommandId::GenerateTests => "assemble context and draft tests for one path",
        SlashCommandId::Agents => "show configured agents, teams, and live session-task counts",
        SlashCommandId::Watch => "inspect persisted session-task status by id or agent",
        SlashCommandId::Edit | SlashCommandId::Fix => "edit loop that may patch files",
        SlashCommandId::Diff => "git diff preview without starting a model turn",
        SlashCommandId::Run | SlashCommandId::Test => "local validation only; no model turn",
        SlashCommandId::Delegate => "create a persisted session task for a configured agent",
        SlashCommandId::Init => "write .vex scaffolding in the current workspace",
        SlashCommandId::Model => "show or switch the active model name",
        SlashCommandId::Permissions | SlashCommandId::Allow | SlashCommandId::Deny => {
            "inspect or change capability grants"
        }
        SlashCommandId::Usage => "show last-turn and session token counts",
        SlashCommandId::Commands | SlashCommandId::Help => "show grouped operator command menu",
        SlashCommandId::MemoryShow | SlashCommandId::MemoryAdd | SlashCommandId::MemoryClear => {
            "view or update persistent notes"
        }
        SlashCommandId::New
        | SlashCommandId::Resume
        | SlashCommandId::Compact
        | SlashCommandId::Fork => "manage saved session state",
        SlashCommandId::Quit | SlashCommandId::Exit => "save state and exit",
        SlashCommandId::About => "show build and environment info",
        SlashCommandId::Undo => "revert last file-modifying tool call from checkpoint stack",
    }
}

fn builtin_tool_menu_group(name: &str) -> &'static str {
    match name {
        "list_files" | "list_directory" | "list_dir" | "glob_files" | "find_files" | "search"
        | "search_files" | "search_content" | "codebase_search" | "read_file" => "retrieve",
        "write_file" | "edit_file" | "apply_patch" | "rename_file" => "mutate",
        "git_status" | "git_diff" | "git_log" | "git_show" | "git_add" | "git_commit" => "git",
        _ => "other",
    }
}

fn builtin_tool_usage_hint(name: &str) -> &'static str {
    match name {
        "list_files" | "list_directory" | "list_dir" => {
            "start broad at the workspace or directory level"
        }
        "glob_files" => "find files by name pattern across the workspace",
        "find_files" => "narrow to filename matches before reading content",
        "search" | "search_content" => "scan exact text or regex hits across files",
        "codebase_search" => "rank functions, types, and code snippets before opening files",
        "read_file" => "read exact paths after discovery narrows the target",
        "write_file" | "edit_file" | "apply_patch" | "rename_file" => {
            "make targeted workspace mutations"
        }
        "git_status" | "git_diff" | "git_log" | "git_show" | "git_add" | "git_commit" => {
            "inspect and record repository state"
        }
        _ => "built-in tool",
    }
}

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

struct HistoryState {
    lines: Vec<String>,
    turn_in_progress: bool,
    cancel_pending: bool,
    active_assistant_index: Option<usize>,
    scroll_offset: usize,
    auto_follow: bool,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            turn_in_progress: false,
            cancel_pending: false,
            active_assistant_index: None,
            scroll_offset: 0,
            auto_follow: true,
        }
    }
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

#[derive(Clone, Debug, Default)]
struct CommandSessionState {
    id: u64,
    command: String,
    pid: Option<u32>,
    status: String,
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
    /// Structured timeline entries derived from canonical task state.
    pub timeline_entries: Vec<TimelineEntry>,
    /// Index of the selected timeline entry (for inspector focus).
    pub selected_step: usize,
    /// Total number of steps (including those scrolled out of view).
    pub total_steps: usize,
    /// Human-readable title for the output pane.
    pub output_title: String,
    pub output_rows: Vec<String>,
    /// Scroll amount for the output pane, interpreted using `output_scroll_anchor`.
    pub output_scroll_offset: usize,
    pub output_scroll_anchor: OutputScrollAnchor,
    pub pending_approval: Option<String>,
    pub input_hint: String,
    /// Live composer buffer for the fullscreen task surface.
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
    history_state: HistoryState,
    overlay_state: OverlayState,
    command_sessions: Vec<CommandSessionState>,
    next_command_session_id: u64,
    history_line_cap: usize,
    repo_label: String,
    instructions_path: Option<String>,
    mcp_snapshot: Option<McpRegistrySnapshot>,
    history_content_width: Cell<usize>,
    active_stream_blocks: std::collections::HashMap<usize, StreamBlock>,
    /// Raw partial-JSON accumulator for streaming tool-call input, keyed by block index.
    /// Cleared when the block completes or the turn ends. ADR-021 Item 22.
    tool_input_raw_buffers: std::collections::HashMap<usize, String>,
    pending_quit: bool,
    quit_requested: bool,
    notes_path: Option<PathBuf>,
    current_task: crate::runtime::TaskState,
    /// Active model name, updated by `/model <n>`.
    model_name: String,
    /// Locked at session start; `/model` rejects changes that require a different backend.
    model_backend: crate::runtime::ModelBackendKind,
    /// Selected coding profile for edit-loop and semantic-command prompts.
    model_profile: ModelProfile,
    /// Working directory for workspace-relative commands like `/diff`.
    working_dir: PathBuf,
    sandbox: ConfiguredSandbox,
    file_prompt_entries: RefCell<Option<Vec<String>>>,
    custom_commands: Vec<CustomCommand>,
    last_assembled_context: Option<AssembledContext>,
    read_only_turn_active: bool,
    active_edit_loop: Option<EditLoop>,
    current_turn_input: String,
    current_turn_response: String,
    current_turn_changed_files: std::collections::BTreeSet<String>,
    current_turn_command_history: Vec<crate::runtime::CommandEvidence>,
    current_turn_tool_invocations: Vec<ToolInvocationSummary>,
    pending_turn_tool_calls: std::collections::HashMap<String, PendingTurnToolCall>,
    /// Index of the currently selected timeline entry in the activity pane.
    selected_timeline_index: usize,
    /// Monotonic counter for stable [`TimelineEntry::step_id`] values.
    next_step_id: u64,
    /// When true, selection auto-advances to the latest timeline entry.
    /// Set to false when the operator scrolls manually; reset on new turn.
    timeline_follow_mode: bool,
    /// Transcript scrollback measured upward from the composer edge.
    transcript_scroll_offset: usize,
    /// Inspector/detail scroll offset measured downward from the top.
    inspector_scroll_offset: usize,
    /// Wall-clock start instant for the active turn.
    turn_started_at: Option<Instant>,
    /// Last completed turn's tool invocations (kept for persistent display).
    last_turn_tool_invocations: Vec<ToolInvocationSummary>,
    /// Last completed turn's response text (kept for persistent display).
    last_turn_response: String,
    /// Last completed turn's input text (kept for persistent display).
    last_turn_input_display: String,
    /// Last completed or failed turn duration.
    last_turn_duration: Option<Duration>,
    /// Last visible terminal error for the task surface.
    last_error_message: Option<String>,
    /// Remembers a runtime turn completion event until the last command session exits.
    turn_completion_pending: bool,
    /// Tracks whether the current turn is a `/plan` command (ADR-029 plan persistence).
    plan_turn_active: bool,
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
                } else if target == ScrollTarget::History {
                    self.apply_history_scroll_action(action);
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
        self.history_state.turn_in_progress
    }
}
