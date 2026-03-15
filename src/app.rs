use crate::api::client::builtin_tool_summaries;
use crate::config::Config;
use crate::custom_commands::{load_custom_commands, CustomCommand};
use crate::prompts::{
    render_custom_command_instruction, render_edit_prompt, render_explain_prompt,
    render_generate_tests_prompt, CODER_SYSTEM_PROMPT,
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
#[cfg(test)]
use crate::ui::render::input_visual_rows;
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

mod commands;
#[cfg(test)]
mod tests;
pub(crate) mod util;

use self::util::{
    builtin_slash_command_names, capability_for_tool_name, format_inline_block, kebab_to_scope,
    list_recent_task_entries, new_task_id, parse_generate_tests_args, resolve_history_line_cap,
    resolve_repo_label, run_validation_suite_capture, sanitize_task_label, scope_to_label,
    shell_command_request, summarize_tool_outcome,
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

#[derive(Clone, Debug, Default)]
pub struct TaskLayoutState {
    pub task_id: String,
    pub status_line: String,
    pub activity_rows: Vec<String>,
    pub output_rows: Vec<String>,
    pub pending_approval: Option<String>,
    pub input_hint: String,
    pub changed_files: Vec<String>,
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

impl TuiMode {
    pub fn new() -> Self {
        Self::new_with_config(None, Config::default_for_tui())
    }

    pub fn new_with_notes(notes_path: Option<PathBuf>) -> Self {
        let mut config = Config::default_for_tui();
        config.notes_path = notes_path;
        Self::new_with_config(config.notes_path.clone(), config)
    }

    pub fn new_with_config(notes_path: Option<PathBuf>, config: Config) -> Self {
        let custom_commands =
            load_custom_commands(&config.working_dir, &builtin_slash_command_names());
        Self {
            history_state: HistoryState::default(),
            overlay_state: OverlayState::default(),
            command_sessions: Vec::new(),
            next_command_session_id: 1,
            history_line_cap: resolve_history_line_cap(),
            repo_label: resolve_repo_label(),
            instructions_path: None,
            history_content_width: Cell::new(HISTORY_CONTENT_WIDTH_FALLBACK),
            active_stream_blocks: std::collections::HashMap::new(),
            pending_quit: false,
            quit_requested: false,
            notes_path,
            current_task: crate::runtime::TaskState::new(new_task_id()),
            model_name: config.model_name.clone(),
            model_backend: config.model_backend,
            model_profile: config.model_profile.clone(),
            working_dir: config.working_dir.clone(),
            custom_commands,
            last_assembled_context: None,
            read_only_turn_active: false,
            active_edit_loop: None,
            current_turn_input: String::new(),
            current_turn_response: String::new(),
            current_turn_changed_files: std::collections::BTreeSet::new(),
            current_turn_command_history: Vec::new(),
            current_turn_tool_invocations: Vec::new(),
            pending_turn_tool_calls: std::collections::HashMap::new(),
            #[cfg(test)]
            last_turn_input: None,
        }
    }

    fn mode_status_label(&self) -> &'static str {
        if self.overlay_active() {
            "overlay"
        } else if self.command_session_active() {
            "command-session"
        } else if self.pending_quit {
            "quit-arm"
        } else if self.history_state.cancel_pending {
            "cancelling"
        } else if self.history_state.turn_in_progress {
            "streaming"
        } else {
            "ready"
        }
    }

    fn approval_status_label(&self) -> &'static str {
        if self.overlay_active() {
            "pending"
        } else if self.overlay_state.auto_approve_session {
            "auto"
        } else {
            "none"
        }
    }

    pub fn status_line(&self) -> String {
        let history_rows =
            history_visual_line_count(&self.history_state.lines, self.history_content_width.get());
        format!(
            "mode:{} approval:{} history:{} repo:{} inst:{}",
            self.mode_status_label(),
            self.approval_status_label(),
            history_rows,
            self.repo_label,
            self.instructions_path.as_deref().unwrap_or("none")
        )
    }

    pub fn current_task_id(&self) -> String {
        self.current_task.id.clone()
    }

    pub fn overlay_active(&self) -> bool {
        self.overlay_state.pending_approval.is_some()
            || self.overlay_state.pending_patch_approval.is_some()
            || self.overlay_state.pending_resume_selection.is_some()
            || self.overlay_state.pending_memory_clear
    }

    fn patch_overlay_active(&self) -> bool {
        self.overlay_state.pending_patch_approval.is_some()
    }

    pub fn history_lines(&self) -> &[String] {
        &self.history_state.lines
    }

    pub fn active_assistant_index(&self) -> Option<usize> {
        self.history_state.active_assistant_index
    }

    pub fn history_scroll_offset(&self) -> usize {
        self.history_state.scroll_offset
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn pending_patch_overlay(&self) -> Option<(&str, usize)> {
        self.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|pending| (pending.patch_preview.as_str(), pending.scroll_offset))
    }

    pub fn pending_tool_overlay(&self) -> Option<(&str, &str, bool)> {
        self.overlay_state.pending_approval.as_ref().map(|pending| {
            (
                pending.tool_name.as_str(),
                pending.input_preview.as_str(),
                self.overlay_state.auto_approve_session,
            )
        })
    }

    pub fn pending_memory_clear_overlay(&self) -> bool {
        self.overlay_state.pending_memory_clear
    }

    pub fn command_session_active(&self) -> bool {
        !self.command_sessions.is_empty()
    }

    pub fn set_history_content_width(&self, width: usize) {
        self.history_content_width.set(width.max(1));
    }

    fn command_session_rows(&self) -> Option<Vec<String>> {
        if self.command_sessions.is_empty() {
            return None;
        }
        let mut rows = Vec::new();
        for (i, session) in self.command_sessions.iter().enumerate() {
            if i > 0 {
                rows.push(String::new());
            }
            rows.push(format!("command: {}", session.command));
            rows.push(format!(
                "pid    : {}",
                session
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "pending".to_string())
            ));
            rows.push(format!("status : {}", session.status));
        }
        Some(rows)
    }

    pub fn task_layout_state(&self) -> Option<TaskLayoutState> {
        if !self.history_state.turn_in_progress && !self.overlay_active() {
            return None;
        }

        let pending_approval = if self.overlay_state.pending_patch_approval.is_some() {
            Some("ApplyPatch".to_string())
        } else if self.overlay_state.pending_resume_selection.is_some() {
            Some("Resume saved task\n[type 1-5 or n to cancel]".to_string())
        } else {
            self.overlay_state.pending_approval.as_ref().map(|pending| {
                summarize_tool_approval_context(&pending.tool_name, &pending.input_preview)
            })
        };

        let activity_rows = self.command_session_rows().unwrap_or_else(|| {
            self.history_state
                .lines
                .iter()
                .rev()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        });
        let input_hint = if let Some(approval) = pending_approval.clone() {
            format!("{approval}\n[y/n/s] ")
        } else if self.command_session_active() {
            "[command session active — Ctrl+C to cancel]".to_string()
        } else {
            "> ".to_string()
        };
        Some(TaskLayoutState {
            task_id: self.current_task.id.clone(),
            status_line: self.status_line(),
            activity_rows,
            output_rows: self.history_state.lines.clone(),
            pending_approval,
            input_hint,
            changed_files: self
                .current_task
                .changed_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        })
    }

    fn resolve_pending_approval(&mut self, approved: bool, ctx: &RuntimeContext) {
        if let Some(pending) = self.overlay_state.pending_approval.take() {
            match pending.action {
                PendingApprovalAction::Tool(response_tx) => {
                    let _ = response_tx.send(approved);
                }
                PendingApprovalAction::InlineCommand(command) => {
                    if approved {
                        self.start_command_session(command.command, ctx);
                    }
                }
            }
        }
    }

    fn handle_approval_input(&mut self, input: &str, ctx: &mut RuntimeContext) {
        if self.overlay_state.pending_resume_selection.is_some() {
            self.handle_resume_selection_input(input, ctx);
            return;
        }
        let context = self
            .overlay_state
            .pending_approval
            .as_ref()
            .map(|p| summarize_tool_approval_context(&p.tool_name, &p.input_preview))
            .unwrap_or_else(|| "unknown".to_string());
        match parse_approval_selection(input) {
            Some(ApprovalSelection::ApproveOnce) => {
                self.push_history_line(format!("[tool approval accepted once: {context}]"));
                self.resolve_pending_approval(true, ctx);
            }
            Some(ApprovalSelection::ApproveSession) => {
                self.overlay_state.auto_approve_session = true;
                self.push_history_line(format!("[tool approval enabled for session: {context}]"));
                self.resolve_pending_approval(true, ctx);
            }
            Some(ApprovalSelection::Deny) => {
                self.push_history_line(format!("[tool approval denied: {context}]"));
                self.resolve_pending_approval(false, ctx);
            }
            None => {
                self.push_history_line("[invalid selection, expected 1/2/3]".to_string());
            }
        }
    }

    fn resolve_pending_patch_approval(&mut self, approved: bool) {
        if let Some(mut pending) = self.overlay_state.pending_patch_approval.take() {
            if let Some(tx) = pending.response_tx.take() {
                let _ = tx.send(approved);
            }
            let decision = if approved { "accepted" } else { "denied" };
            self.push_history_line(format!("[patch approval {decision}]"));
        }
    }

    fn apply_patch_overlay_scroll_action(&mut self, action: ScrollAction) {
        if let Some(pending) = self.overlay_state.pending_patch_approval.as_mut() {
            let max = pending.patch_preview.lines().count().saturating_sub(1);
            match action {
                ScrollAction::LineUp => {
                    pending.scroll_offset = pending.scroll_offset.saturating_sub(1);
                }
                ScrollAction::LineDown => {
                    pending.scroll_offset = pending.scroll_offset.saturating_add(1).min(max);
                }
                ScrollAction::PageUp(step) => {
                    pending.scroll_offset = pending.scroll_offset.saturating_sub(step.max(1));
                }
                ScrollAction::PageDown(step) => {
                    pending.scroll_offset =
                        pending.scroll_offset.saturating_add(step.max(1)).min(max);
                }
                ScrollAction::Home => {
                    pending.scroll_offset = 0;
                }
                ScrollAction::End => {
                    pending.scroll_offset = max;
                }
            }
        }
    }

    fn handle_patch_overlay_input(&mut self, input: &str) {
        if self.overlay_state.pending_patch_approval.is_none() {
            return;
        }

        match parse_approval_selection(input) {
            Some(ApprovalSelection::ApproveOnce) => self.resolve_pending_patch_approval(true),
            Some(ApprovalSelection::Deny) => self.resolve_pending_patch_approval(false),
            Some(ApprovalSelection::ApproveSession) | None => {}
        }
    }

    fn push_history_line(&mut self, line: String) {
        self.history_state.lines.push(line);
        self.enforce_history_cap();
        if self.history_state.auto_follow {
            self.set_scroll_to_bottom();
        } else {
            self.clamp_scroll_offset();
        }
    }

    fn enforce_history_cap(&mut self) {
        let cap = self.history_line_cap;
        if self.history_state.lines.len() <= cap {
            return;
        }

        let excess = self.history_state.lines.len() - cap;
        self.history_state.lines.drain(..excess);
        self.history_state.active_assistant_index = self
            .history_state
            .active_assistant_index
            .and_then(|idx| idx.checked_sub(excess));
        self.history_state.scroll_offset = self.history_state.scroll_offset.saturating_sub(excess);
        self.clamp_scroll_offset();
    }

    fn max_scroll_offset(&self) -> usize {
        history_visual_line_count(&self.history_state.lines, self.history_content_width.get())
            .saturating_sub(1)
    }

    fn set_scroll_to_bottom(&mut self) {
        self.history_state.scroll_offset = self.max_scroll_offset();
    }

    fn clamp_scroll_offset(&mut self) {
        let max = self.max_scroll_offset();
        self.history_state.scroll_offset = self.history_state.scroll_offset.min(max);
    }

    fn apply_page_up(&mut self, page_step: usize) {
        self.history_state.scroll_offset = self
            .history_state
            .scroll_offset
            .saturating_sub(page_step.max(1));
        self.history_state.auto_follow = false;
    }

    fn apply_page_down(&mut self, page_step: usize) {
        let max = self.max_scroll_offset();
        self.history_state.scroll_offset = self
            .history_state
            .scroll_offset
            .saturating_add(page_step.max(1))
            .min(max);
        self.history_state.auto_follow = self.history_state.scroll_offset >= max;
    }

    fn apply_home(&mut self) {
        self.history_state.scroll_offset = 0;
        self.history_state.auto_follow = false;
    }

    fn apply_end(&mut self) {
        self.set_scroll_to_bottom();
        self.history_state.auto_follow = true;
    }

    fn apply_history_scroll_action(&mut self, action: ScrollAction) {
        match action {
            ScrollAction::LineUp => self.apply_page_up(1),
            ScrollAction::LineDown => self.apply_page_down(1),
            ScrollAction::PageUp(step) => self.apply_page_up(step),
            ScrollAction::PageDown(step) => self.apply_page_down(step),
            ScrollAction::Home => self.apply_home(),
            ScrollAction::End => self.apply_end(),
        }
    }

    fn reset_conversation_window(&mut self, ctx: &RuntimeContext) {
        ctx.clear_conversation();
        self.history_state.lines.clear();
        self.history_state.turn_in_progress = false;
        self.history_state.cancel_pending = false;
        self.command_sessions.clear();
        self.history_state.active_assistant_index = None;
        self.history_state.scroll_offset = 0;
        self.history_state.auto_follow = true;
        self.active_stream_blocks.clear();
        self.last_assembled_context = None;
        self.read_only_turn_active = false;
        self.reset_turn_capture();
    }

    fn apply_resumed_task(&mut self, state: TaskState, ctx: &RuntimeContext) {
        let restored_id = state.id.clone();
        let status = format!("{:?}", state.status);
        self.current_task = state;
        if let Some(path) = self.current_task.instructions_path.clone() {
            self.instructions_path = Some(path);
        } else {
            self.current_task.instructions_path = self.instructions_path.clone();
        }
        self.active_edit_loop = None;
        ctx.reset_session_tokens();
        self.reset_conversation_window(ctx);
        self.push_history_line(format!("[resumed: {restored_id} status={status}]"));
    }

    fn reset_turn_capture(&mut self) {
        self.current_turn_input.clear();
        self.current_turn_response.clear();
        self.current_turn_changed_files.clear();
        self.current_turn_command_history.clear();
        self.current_turn_tool_invocations.clear();
        self.pending_turn_tool_calls.clear();
    }

    fn begin_turn_capture(&mut self, input: String) {
        self.reset_turn_capture();
        self.current_turn_input = input;
        self.current_task.status = TaskStatus::Running;
    }

    fn begin_command_session(&mut self, command: String) -> u64 {
        let session_id = self.next_command_session_id;
        self.begin_command_session_with_id(session_id, command);
        session_id
    }

    fn begin_command_session_with_id(&mut self, session_id: u64, command: String) {
        self.next_command_session_id = self
            .next_command_session_id
            .max(session_id.saturating_add(1));
        if self
            .command_sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            return;
        }
        self.command_sessions.push(CommandSessionState {
            id: session_id,
            command,
            pid: None,
            status: "running".to_string(),
        });
        self.current_task.status = TaskStatus::Running;
    }

    fn commit_completed_turn(&mut self, ctx: &RuntimeContext) {
        if self.current_turn_input.trim().is_empty()
            && self.current_turn_response.trim().is_empty()
            && self.current_turn_changed_files.is_empty()
            && self.current_turn_command_history.is_empty()
            && self.current_turn_tool_invocations.is_empty()
        {
            self.current_task.status = TaskStatus::Completed;
            self.reset_turn_capture();
            return;
        }

        let changed_files = self
            .current_turn_changed_files
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for path in &changed_files {
            let path_buf = PathBuf::from(path);
            if !self
                .current_task
                .changed_files
                .iter()
                .any(|existing| existing == &path_buf)
            {
                self.current_task.changed_files.push(path_buf);
            }
        }

        let command_history = std::mem::take(&mut self.current_turn_command_history);
        self.current_task
            .command_history
            .extend(command_history.iter().cloned());
        self.current_task.instructions_path = self.instructions_path.clone();
        self.current_task.status = TaskStatus::Completed;
        self.current_task.turns.push(TurnEvidenceState {
            input: std::mem::take(&mut self.current_turn_input),
            response: std::mem::take(&mut self.current_turn_response),
            changed_files,
            command_history,
            tool_invocations: std::mem::take(&mut self.current_turn_tool_invocations),
            tokens: ctx.session_tokens_snapshot().last_turn(),
        });

        let dir = TaskState::state_dir();
        if let Err(error) = self.current_task.save(&dir) {
            self.push_history_line(format!("[state] save failed: {error}"));
        }
        self.reset_turn_capture();
    }

    fn summarize_usage_line_suffix(estimated: bool) -> &'static str {
        if estimated {
            " (estimated)"
        } else {
            ""
        }
    }

    fn prompt_resume_selection(&mut self, entries: Vec<ResumeTaskEntry>) {
        self.push_history_line("[resume] choose a task to resume:".to_string());
        for (index, entry) in entries.iter().enumerate() {
            self.push_history_line(format!("  {}. {} ({})", index + 1, entry.id, entry.status));
        }
        self.push_history_line("[resume] type 1-5 to select or n to cancel".to_string());
        self.overlay_state.pending_resume_selection = Some(PendingResumeSelection { entries });
    }

    fn handle_resume_selection_input(&mut self, input: &str, ctx: &mut RuntimeContext) {
        let trimmed = input.trim();
        if matches!(trimmed.to_ascii_lowercase().as_str(), "n" | "no" | "esc") {
            self.overlay_state.pending_resume_selection = None;
            self.push_history_line("[resume] cancelled".to_string());
            return;
        }

        let Some(selection) = trimmed.parse::<usize>().ok() else {
            self.push_history_line("[resume] invalid selection, expected 1-5 or n".to_string());
            return;
        };

        let Some(entry) = self
            .overlay_state
            .pending_resume_selection
            .as_ref()
            .and_then(|pending| pending.entries.get(selection.saturating_sub(1)))
            .cloned()
        else {
            self.push_history_line("[resume] invalid selection, expected 1-5 or n".to_string());
            return;
        };

        self.overlay_state.pending_resume_selection = None;
        match TaskState::load_from_search_dirs(&entry.id) {
            Ok(state) => self.apply_resumed_task(state, ctx),
            Err(_) => {
                self.push_history_line(format!("[resume: task '{}' not found]", entry.id));
            }
        }
    }

    fn registered_slash_command(input: &str) -> Option<(&'static SlashCommandSpec, &str)> {
        let trimmed = input.trim();
        SLASH_COMMANDS
            .iter()
            .find_map(|spec| spec.pattern.parse(trimmed).map(|args| (spec, args)))
    }

    fn registered_custom_command<'a>(
        &'a self,
        input: &'a str,
    ) -> Option<(&'a CustomCommand, &'a str)> {
        let trimmed = input.trim();
        let raw = trimmed.strip_prefix('/')?;
        let (name, args) = raw
            .find(char::is_whitespace)
            .map(|index| (&raw[..index], raw[index..].trim()))
            .unwrap_or((raw, ""));
        self.custom_commands
            .iter()
            .find(|command| command.name == name)
            .map(|command| (command, args))
    }

    fn is_reentrant_edit_command(input: &str) -> bool {
        Self::registered_slash_command(input)
            .map(|(spec, _)| matches!(spec.id, SlashCommandId::Edit | SlashCommandId::Fix))
            .unwrap_or(false)
    }

    fn expand_inline_file_tokens(&self, input: &str) -> String {
        if input.starts_with('/') {
            return input.to_string();
        }

        let assembler = ContextAssembler::default();
        let operator = ToolOperator::new(self.working_dir.clone());
        let mut output = String::new();
        let mut token = String::new();

        for ch in input.chars() {
            if ch.is_whitespace() {
                if !token.is_empty() {
                    output.push_str(&self.expand_inline_token(&token, &operator, &assembler));
                    token.clear();
                }
                output.push(ch);
            } else {
                token.push(ch);
            }
        }

        if !token.is_empty() {
            output.push_str(&self.expand_inline_token(&token, &operator, &assembler));
        }

        output
    }

    fn expand_inline_token(
        &self,
        token: &str,
        operator: &ToolOperator,
        assembler: &ContextAssembler,
    ) -> String {
        let Some(path) = token.strip_prefix('@') else {
            return token.to_string();
        };

        if path.is_empty() {
            return token.to_string();
        }

        match operator.existing_path(path) {
            Ok(Some(resolved)) if resolved.is_dir() => {
                match operator.list_files(Some(path), assembler.max_related) {
                    Ok(listing) => format_inline_block("dir", path, &listing, false, None),
                    Err(error) => format!("[dir: {path} \u{2014} {error}]"),
                }
            }
            Ok(Some(_)) => match operator.read_file(path) {
                Ok(content) => {
                    let (content, truncated) =
                        truncate_head_bytes(&content, assembler.max_file_bytes);
                    format_inline_block(
                        "file",
                        path,
                        &content,
                        truncated,
                        Some(assembler.max_file_bytes),
                    )
                }
                Err(error) => format!("[file: {path} \u{2014} {error}]"),
            },
            Ok(None) => format!("[file: {path} \u{2014} not found]"),
            Err(error) => format!("[file: {path} \u{2014} {error}]"),
        }
    }

    fn handle_bang_command(&mut self, command: &str, ctx: &RuntimeContext) {
        let command = command.trim();
        if command.is_empty() {
            self.push_history_line("[shell] usage: !<command>".to_string());
            return;
        }

        if self.overlay_state.auto_approve_session {
            self.push_history_line("[auto-approved tool: run_command session]".to_string());
            self.start_command_session(command.to_string(), ctx);
            return;
        }

        if let Some(scope) = self
            .current_task
            .active_grants
            .get(&Capability::RunCommand)
            .copied()
        {
            if matches!(scope, ApprovalScope::Once) {
                self.current_task
                    .active_grants
                    .remove(&Capability::RunCommand);
            }
            self.push_history_line(format!(
                "[auto-approved tool: run_command {} grant]",
                scope_to_label(scope)
            ));
            self.start_command_session(command.to_string(), ctx);
            return;
        }

        let summary = summarize_tool_approval_context("run_command", command);
        self.push_history_line(format!("[tool approval requested: {summary}]"));
        self.overlay_state.pending_approval = Some(PendingApproval {
            tool_name: "run_command".to_string(),
            input_preview: command.to_string(),
            action: PendingApprovalAction::InlineCommand(PendingInlineCommand {
                command: command.to_string(),
            }),
        });
    }

    fn start_command_session(&mut self, command: String, ctx: &RuntimeContext) {
        let starting_batch = self.command_sessions.is_empty();
        self.history_state.turn_in_progress = true;
        self.history_state.cancel_pending = false;
        self.history_state.active_assistant_index = None;
        if starting_batch {
            self.begin_turn_capture(format!("!{command}"));
        }
        let session_id = self.begin_command_session(command.clone());

        let ctx = ctx.clone();
        let cancel = ctx.turn_cancellation_token();
        let working_dir = self.working_dir.clone();
        tokio::spawn(async move {
            let runner = DefaultCommandRunner::new();
            // User-initiated !command execution — the Capability::RunCommand
            // approval gate is the security boundary.  PassthroughSandbox is
            // intentional here; a future ADR-024 follow-up may thread the
            // operator-configured sandbox driver through TuiMode.
            let request = match PassthroughSandbox
                .wrap(shell_command_request(command.clone(), working_dir))
            {
                Ok(request) => request,
                Err(error) => {
                    ctx.emit_transcript_line(format!("[command session] error: {error}"));
                    ctx.emit_command_session_finished(session_id);
                    ctx.emit_turn_complete();
                    return;
                }
            };
            let (output_tx, mut output_rx) = mpsc::channel(128);
            let mut handle = match runner.run_streaming(request, output_tx).await {
                Ok(handle) => handle,
                Err(error) => {
                    ctx.emit_transcript_line(format!("[command session] error: {error}"));
                    ctx.emit_command_session_finished(session_id);
                    ctx.emit_turn_complete();
                    return;
                }
            };
            ctx.emit_command_session_attached(session_id, handle.pid());
            ctx.emit_transcript_line(format_command_session_started(&command, handle.pid()));

            let mut cancel_requested = false;
            loop {
                tokio::select! {
                    _ = cancel.cancelled(), if !cancel_requested => {
                        cancel_requested = true;
                        let _ = handle.cancel();
                        ctx.emit_transcript_line("[command session cancellation requested]".to_string());
                    }
                    chunk = output_rx.recv() => {
                        match chunk {
                            Some(chunk) => {
                                for line in format_command_session_output(chunk) {
                                    ctx.emit_transcript_line(line);
                                }
                            }
                            None => break,
                        }
                    }
                }
            }

            match handle.wait().await {
                Ok(result) => {
                    if cancel_requested {
                        ctx.emit_transcript_line(format_command_session_cancelled());
                    } else {
                        ctx.emit_transcript_line(format_command_session_exit(result.exit_code));
                    }
                }
                Err(error) => {
                    ctx.emit_transcript_line(format!("[command session] error: {error}"));
                }
            }
            ctx.emit_command_session_finished(session_id);
            ctx.emit_turn_complete();
        });
    }

    fn start_single_turn(
        &mut self,
        rendered: String,
        ctx: &mut RuntimeContext,
        read_only: bool,
        supplementary_system_prompt: Option<&str>,
    ) {
        self.start_single_turn_with_policy(
            rendered,
            ctx,
            read_only,
            supplementary_system_prompt,
            TurnToolPolicy::Default,
        );
    }

    fn start_single_turn_with_policy(
        &mut self,
        rendered: String,
        ctx: &mut RuntimeContext,
        read_only: bool,
        supplementary_system_prompt: Option<&str>,
        turn_tool_policy: TurnToolPolicy,
    ) {
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        self.read_only_turn_active = read_only;
        self.begin_turn_capture(rendered.clone());
        #[cfg(test)]
        {
            self.last_turn_input = Some(rendered.clone());
        }
        ctx.start_turn_with_system_prompt_and_policy(
            rendered,
            supplementary_system_prompt.map(ToString::to_string),
            turn_tool_policy,
        );
    }

    fn selected_system_prompt(&self) -> &'static str {
        self.model_profile
            .system_prompt_text()
            .unwrap_or(CODER_SYSTEM_PROMPT)
    }

    fn assemble_rendered_context(&mut self, scope_instruction: &str) -> String {
        let assembler = ContextAssembler::default();
        let render_assembler = assembler.clone();
        let operator = ToolOperator::new(self.working_dir.clone());
        let scope_instruction_for_task = scope_instruction.to_string();
        let assembled = block_on_context_task(async move {
            tokio::task::spawn_blocking(move || {
                assembler.assemble(&scope_instruction_for_task, &operator)
            })
            .await
            .map_err(|error| anyhow::anyhow!("failed to join context assembly task: {error}"))?
        })
        .ok();
        if let Some(context) = assembled.clone() {
            self.last_assembled_context = Some(context);
        }
        assembled
            .as_ref()
            .map(|context| render_assembler.render(context))
            .unwrap_or_else(|| "## Context\n[context: unavailable]\n".to_string())
    }

    fn resolved_notes_path(&self) -> Option<PathBuf> {
        resolve_notes_path_for_write(self.notes_path.as_deref())
    }

    fn resolved_existing_notes_path(&self) -> Option<PathBuf> {
        resolve_notes_path_for_read(self.notes_path.as_deref())
    }
}

#[cfg(test)]
fn input_rows_for_buffer(input: &str, width: usize) -> u16 {
    input_visual_rows(input, width).clamp(1, MAX_INPUT_PANE_ROWS) as u16
}

#[cfg(test)]
struct RenderGuard {
    dirty: bool,
    cursor_tick: Duration,
    status_tick: Duration,
    last_draw_at: Instant,
    last_render_state_hash: Option<u64>,
}

#[cfg(test)]
impl RenderGuard {
    fn with_intervals(cursor_tick: Duration, status_tick: Duration, now: Instant) -> Self {
        Self {
            dirty: true,
            cursor_tick,
            status_tick,
            last_draw_at: now,
            last_render_state_hash: None,
        }
    }

    fn poll_timeout(&self) -> Duration {
        self.cursor_tick.min(self.status_tick)
    }

    fn should_draw(&mut self, now: Instant, state_hash: u64) -> bool {
        if self.last_render_state_hash != Some(state_hash) {
            self.dirty = true;
        }

        if self.dirty || now.saturating_duration_since(self.last_draw_at) >= self.poll_timeout() {
            self.dirty = false;
            self.last_draw_at = now;
            self.last_render_state_hash = Some(state_hash);
            true
        } else {
            false
        }
    }
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
                } else if target == ScrollTarget::History {
                    self.apply_history_scroll_action(action);
                }
            }
        }
    }

    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        if self.overlay_active() {
            if self.overlay_state.pending_memory_clear {
                self.handle_memory_clear_input(&input);
                return;
            } else if self.patch_overlay_active() {
                self.handle_patch_overlay_input(&input);
                return;
            } else {
                self.handle_approval_input(&input, ctx);
                return;
            }
        }

        if self.history_state.turn_in_progress {
            let trimmed = input.trim();
            let reentrant_edit_command =
                self.active_edit_loop.is_some() && Self::is_reentrant_edit_command(trimmed);
            if reentrant_edit_command {
                self.push_history_line(format!("> {input}"));
                self.push_history_line(String::new());
                let _ = self.try_handle_slash_command(&input, ctx);
                return;
            }
            if self.history_state.cancel_pending {
                self.push_history_line(
                    "[busy - cancelling current turn, input ignored]".to_string(),
                );
            } else {
                // Allow additional shell commands only while an existing
                // command-session batch is active. This avoids clobbering
                // model-turn capture state with unrelated inline commands.
                let trimmed = input.trim();
                if let Some(command) = trimmed.strip_prefix('!') {
                    if self.command_session_active() && !command.trim().is_empty() {
                        self.push_history_line(format!("> {input}"));
                        self.push_history_line(String::new());
                        self.handle_bang_command(command, ctx);
                        return;
                    }
                }
                self.push_history_line("[busy - turn in progress, input ignored]".to_string());
            }
            return;
        }

        self.pending_quit = false;
        self.quit_requested = false;
        self.history_state.cancel_pending = false;
        self.push_history_line(format!("> {input}"));
        self.push_history_line(String::new());

        let turn_input = self.expand_inline_file_tokens(&input);

        let trimmed = turn_input.trim();
        if let Some(command) = trimmed.strip_prefix('!') {
            self.handle_bang_command(command, ctx);
            return;
        }

        if turn_input.starts_with('/') && self.try_handle_slash_command(&turn_input, ctx) {
            return;
        }

        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        self.begin_turn_capture(turn_input.clone());

        #[cfg(test)]
        {
            self.last_turn_input = Some(turn_input.clone());
        }

        ctx.start_turn(turn_input);
    }

    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::TranscriptLine(line) => {
                if self.history_state.turn_in_progress {
                    if !self.current_turn_response.is_empty() {
                        self.current_turn_response.push('\n');
                    }
                    self.current_turn_response.push_str(&line);
                }
                self.push_history_line(line);
            }
            UiUpdate::StreamDelta(text) => {
                if self.history_state.cancel_pending {
                    return;
                }
                self.current_turn_response.push_str(&text);
                let idx = match self.history_state.active_assistant_index {
                    Some(idx) => idx,
                    None => {
                        if !self.history_state.turn_in_progress {
                            return;
                        }
                        self.push_history_line(String::new());
                        let idx = self.history_state.lines.len() - 1;
                        self.history_state.active_assistant_index = Some(idx);
                        idx
                    }
                };
                if let Some(line) = self.history_state.lines.get_mut(idx) {
                    line.push_str(&text);
                    *line = sanitize_assistant_text(line);
                }
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                }
            }
            UiUpdate::StreamBlockStart { index, block } => {
                match &block {
                    StreamBlock::ToolCall {
                        id, name, input, ..
                    } => {
                        self.pending_turn_tool_calls.insert(
                            id.clone(),
                            PendingTurnToolCall {
                                name: name.clone(),
                                input: input.clone(),
                            },
                        );
                    }
                    StreamBlock::ToolResult {
                        tool_call_id,
                        output,
                        is_error,
                    } => {
                        if let Some(pending) = self.pending_turn_tool_calls.remove(tool_call_id) {
                            if !*is_error {
                                note_changed_files_from_tool_call(
                                    &mut self.current_turn_changed_files,
                                    &pending.name,
                                    &pending.input,
                                );
                            }
                            if let Some(evidence) =
                                command_evidence_from_tool_result(&pending.name, *is_error)
                            {
                                self.current_turn_command_history.push(evidence);
                            }
                            self.current_turn_tool_invocations
                                .push(ToolInvocationSummary {
                                    name: pending.name,
                                    outcome: summarize_tool_outcome(output, *is_error).to_string(),
                                });
                        }
                    }
                    StreamBlock::Thinking { .. } | StreamBlock::FinalText { .. } => {}
                }
                self.active_stream_blocks.insert(index, block);
            }
            UiUpdate::StreamBlockDelta { index, delta } => {
                if let Some(block) = self.active_stream_blocks.get_mut(&index) {
                    match block {
                        StreamBlock::Thinking { content, .. } => content.push_str(&delta),
                        StreamBlock::FinalText { content } => content.push_str(&delta),
                        StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                    }
                }
            }
            UiUpdate::StreamBlockComplete { index } => {
                self.active_stream_blocks.remove(&index);
            }
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name,
                input_preview,
                response_tx,
            }) => {
                if self.history_state.cancel_pending {
                    let _ = response_tx.send(false);
                    return;
                }

                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);

                if self.read_only_turn_active {
                    let _ = response_tx.send(false);
                    return;
                }

                if self.overlay_state.auto_approve_session {
                    let _ = response_tx.send(true);
                    self.push_history_line(format!("[auto-approved tool: {tool_name} session]"));
                    return;
                }

                if let Some((capability, scope)) =
                    capability_for_tool_name(&tool_name).and_then(|capability| {
                        self.current_task
                            .active_grants
                            .get(&capability)
                            .copied()
                            .map(|scope| (capability, scope))
                    })
                {
                    if matches!(scope, ApprovalScope::Once) {
                        self.current_task.active_grants.remove(&capability);
                    }
                    let _ = response_tx.send(true);
                    self.push_history_line(format!(
                        "[auto-approved tool: {tool_name} {} grant]",
                        scope_to_label(scope)
                    ));
                    return;
                }

                let summary = summarize_tool_approval_context(&tool_name, &input_preview);
                self.push_history_line(format!("[tool approval requested: {summary}]"));
                self.overlay_state.pending_approval = Some(PendingApproval {
                    tool_name,
                    input_preview,
                    action: PendingApprovalAction::Tool(response_tx),
                });
            }
            UiUpdate::EditLoopComplete {
                outcome,
                last_validation_result,
            } => {
                self.command_sessions.clear();
                if let Some(result) = last_validation_result {
                    if let Some(edit_loop) = self.active_edit_loop.as_mut() {
                        edit_loop.set_last_validation_result(result);
                    }
                }
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.history_state.cancel_pending = false;
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                match outcome {
                    EditLoopOutcome::Success {
                        patch_applied,
                        validate_passed,
                    } => {
                        let summary = format!(
                            "[edit loop complete: patch_applied={} validate_passed={}]",
                            patch_applied, validate_passed
                        );
                        self.push_history_line(summary);
                    }
                    EditLoopOutcome::MaxTurnsReached { last_error } => {
                        let summary = match last_error {
                            Some(err) => {
                                format!("[edit loop reached max turns — last error: {err}]")
                            }
                            None => "[edit loop reached max turns]".to_string(),
                        };
                        self.push_history_line(summary);
                    }
                    EditLoopOutcome::ApprovalDenied => {
                        self.push_history_line("[edit loop aborted: approval denied]".to_string());
                    }
                    EditLoopOutcome::Cancelled => {
                        self.push_history_line("[edit loop cancelled]".to_string());
                    }
                }
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                } else {
                    self.clamp_scroll_offset();
                }
            }
            UiUpdate::CommandSessionStarted {
                session_id,
                command,
            } => {
                self.begin_command_session_with_id(session_id, command);
            }
            UiUpdate::CommandSessionAttached { session_id, pid } => {
                if let Some(session) = self
                    .command_sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    session.pid = pid;
                }
            }
            UiUpdate::CommandSessionFinished { session_id } => {
                if let Some(pos) = self
                    .command_sessions
                    .iter()
                    .position(|session| session.id == session_id)
                {
                    self.command_sessions.remove(pos);
                }
            }
            UiUpdate::TurnComplete => {
                if !self.command_sessions.is_empty() {
                    return;
                }
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.commit_completed_turn(ctx);
                self.history_state.cancel_pending = false;
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                self.read_only_turn_active = false;
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                } else {
                    self.clamp_scroll_offset();
                }
            }
            UiUpdate::Error(msg) => {
                self.command_sessions.clear();
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.reset_turn_capture();
                self.history_state.cancel_pending = false;
                self.push_history_line(format!("[error] {msg}"));
                self.current_task.status = TaskStatus::Failed;
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                self.read_only_turn_active = false;
            }
        }
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        if self.history_state.turn_in_progress {
            if self.history_state.cancel_pending {
                return;
            }
            ctx.cancel_turn();
            self.resolve_pending_approval(false, ctx);
            self.resolve_pending_patch_approval(false);
            self.history_state.cancel_pending = true;
            if !self.command_sessions.is_empty() {
                for session in &mut self.command_sessions {
                    session.status = "cancelling".to_string();
                }
                self.current_task.status = TaskStatus::Cancelling;
                self.push_history_line("[command session cancellation requested]".to_string());
            } else {
                self.push_history_line("[turn cancellation requested]".to_string());
            }
            self.pending_quit = false;
            self.quit_requested = false;
            return;
        }

        if self.pending_quit {
            self.quit_requested = true;
        } else {
            self.pending_quit = true;
            self.push_history_line("[press Ctrl+C again to exit]".to_string());
        }
    }

    fn is_turn_in_progress(&self) -> bool {
        self.history_state.turn_in_progress
    }
}

fn summarize_tool_approval_context(tool_name: &str, input_preview: &str) -> String {
    let mut path: Option<&str> = None;
    let mut summary_line: Option<&str> = None;

    for line in input_preview.lines().take(8) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if path.is_none() && trimmed.starts_with("path:") {
            path = Some(trimmed);
            continue;
        }
        if summary_line.is_none()
            && (trimmed.starts_with("change:") || trimmed.starts_with("content:"))
        {
            summary_line = Some(trimmed);
            continue;
        }
        if summary_line.is_none() {
            summary_line = Some(trimmed);
        }
    }

    match (path, summary_line) {
        (Some(path), Some(summary)) => format!("{tool_name} {path} {summary}"),
        (Some(path), None) => format!("{tool_name} {path}"),
        (None, Some(summary)) => format!("{tool_name} {summary}"),
        (None, None) => tool_name.to_string(),
    }
}

fn parse_approval_selection(input: &str) -> Option<ApprovalSelection> {
    let normalized = input.trim().to_lowercase();
    match normalized.as_str() {
        "1" | "y" | "yes" => Some(ApprovalSelection::ApproveOnce),
        "2" | "a" | "always" => Some(ApprovalSelection::ApproveSession),
        "3" | "n" | "no" | "esc" => Some(ApprovalSelection::Deny),
        _ => None,
    }
}

#[cfg(test)]
fn overlay_event_to_user_input(event: Event) -> Option<UserInputEvent> {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Interrupt)
            }
            KeyCode::Esc => Some(UserInputEvent::Text("esc".to_string())),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                Some(UserInputEvent::Text(ch.to_string()))
            }
            _ => None,
        },
        Event::Paste(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(UserInputEvent::Text(trimmed.to_string()))
            }
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
enum RenderPass {
    Header,
    History,
    Input,
    Overlay,
}

#[cfg(test)]
fn render_pass_order(mode: &TuiMode) -> Vec<RenderPass> {
    let mut order = vec![RenderPass::Header, RenderPass::History, RenderPass::Input];
    if mode.overlay_active() {
        order.push(RenderPass::Overlay);
    }
    order
}

pub fn build_runtime(config: Config) -> Result<(Runtime<TuiMode>, RuntimeContext)> {
    let (instructions_text, instructions_path) = match load_project_instructions(
        &config.working_dir,
        config.max_project_instructions_tokens,
    ) {
        LoadResult::Loaded(project_instructions) => {
            let display = project_instructions.path.to_string_lossy().into_owned();
            (Some(project_instructions.content), Some(display))
        }
        LoadResult::OverBudget {
            path,
            estimated_tokens,
        } => {
            eprintln!(
                "[project instructions] {} skipped: estimated {} tokens exceeds budget of {}",
                path.display(),
                estimated_tokens,
                config.max_project_instructions_tokens,
            );
            (None, None)
        }
        LoadResult::NotFound => (None, None),
    };

    let (client, notes_warning) = build_api_client_with_notes(&config)?;
    let client = client.with_project_instructions(instructions_text);
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new_with_hooks(client, operator, config.hooks.clone());

    let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

    let mut mode = TuiMode::new_with_config(config.notes_path.clone(), config);
    mode.instructions_path = instructions_path;
    mode.current_task.instructions_path = mode.instructions_path.clone();
    if let Some(warning) = notes_warning {
        mode.push_history_line(warning);
    }
    let runtime = Runtime::new(mode, update_rx);
    Ok((runtime, ctx))
}

/// Build a runtime and immediately apply a pre-loaded resume state.
/// Called from `src/bin/vex.rs` when `--resume` is passed at startup.
pub fn build_runtime_with_resume(
    config: Config,
    resume_state: TaskState,
) -> Result<(Runtime<TuiMode>, RuntimeContext)> {
    let (mut runtime, ctx) = build_runtime(config)?;
    let restored_id = resume_state.id.clone();
    let status = format!("{:?}", resume_state.status);
    runtime.mode.current_task = resume_state;
    if let Some(path) = runtime.mode.current_task.instructions_path.clone() {
        runtime.mode.instructions_path = Some(path);
    } else {
        runtime.mode.current_task.instructions_path = runtime.mode.instructions_path.clone();
    }
    runtime
        .mode
        .push_history_line(format!("[resumed: {restored_id} status={status}]"));
    Ok((runtime, ctx))
}
