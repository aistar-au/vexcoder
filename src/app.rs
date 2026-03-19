use crate::api::client::builtin_tool_summaries;
use crate::config::Config;
use crate::custom_commands::{load_custom_commands, CustomCommand};
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
use crate::runtime::validation::ValidationSuite;
#[cfg(test)]
use crate::runtime::CommandResult;
use crate::runtime::{
    format_command_session_cancelled, format_command_session_exit, format_command_session_output,
    format_command_session_started, truncate_head_bytes, ApprovalScope, Capability, CommandRequest,
    CommandRunner, DefaultCommandRunner, EditLoopOutcome, PassthroughSandbox, SandboxDriver,
    TaskState, TaskStatus, UiUpdate,
};
#[cfg(test)]
use crate::session_notes::resolve_notes_for_injection;
use crate::session_notes::{
    build_api_client_with_notes, resolve_notes_path_for_read, resolve_notes_path_for_write,
};
use crate::state::{ConversationManager, StreamBlock, ToolApprovalRequest, TurnToolPolicy};
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
use std::cell::Cell;
use std::io::Write;
use std::path::PathBuf;
#[cfg(test)]
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
#[cfg(test)]
mod tests;
mod turn;
mod turn_start;
pub(crate) mod util;
pub use self::errors::{AppError, AppResult};
pub use self::facade::{
    build_facade_client, build_facade_runtime, execute_facade_runtime, run_tui_session,
    serve_facade_local_api, FacadeBootstrap,
};
pub use self::runtime_build::{build_runtime, build_runtime_with_resume};

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
#[cfg(test)]
const MAX_INPUT_PANE_ROWS: usize = 6;

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
    Clear,
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
    Run,
    Test,
    Context,
    Tools,
    Usage,
    GenerateTests,
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
        SlashCommandId::Clear,
        SlashCommandPattern::Exact("/clear"),
        "/clear",
        "clear conversation history (keep task)",
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

#[derive(Clone, Debug, Default)]
pub struct TaskLayoutState {
    pub task_id: String,
    pub status_line: String,
    /// Legacy string rows kept for backward-compatible rendering paths.
    pub activity_rows: Vec<String>,
    /// Structured timeline entries derived from canonical task state.
    pub timeline_entries: Vec<TimelineEntry>,
    /// Index of the selected timeline entry (for inspector focus).
    pub selected_step: usize,
    /// Total number of steps (including those scrolled out of view).
    pub total_steps: usize,
    pub output_rows: Vec<String>,
    pub pending_approval: Option<String>,
    pub input_hint: String,
    /// Live composer buffer for the fullscreen task surface.
    pub composer_text: String,
    /// Cursor byte offset within `composer_text`.
    pub composer_cursor: usize,
    pub changed_files: Vec<String>,
    /// When true the timeline auto-advances to the latest entry.
    pub follow_mode: bool,
}

pub struct TuiMode {
    history_state: HistoryState,
    overlay_state: OverlayState,
    command_sessions: Vec<CommandSessionState>,
    next_command_session_id: u64,
    history_line_cap: usize,
    repo_label: String,
    instructions_path: Option<String>,
    history_content_width: Cell<usize>,
    active_stream_blocks: std::collections::HashMap<usize, StreamBlock>,
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
    /// Last completed turn's tool invocations (kept for persistent display).
    last_turn_tool_invocations: Vec<ToolInvocationSummary>,
    /// Last completed turn's response text (kept for persistent display).
    last_turn_response: String,
    /// Last completed turn's input text (kept for persistent display).
    last_turn_input_display: String,
    #[cfg(test)]
    pub last_turn_input: Option<String>,
}

/// All capabilities in stable kebab order (used for /permissions display and round-trip tests).
pub const ALL_CAPABILITIES: &[Capability] = &[
    Capability::ApplyPatch,
    Capability::Browser,
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
