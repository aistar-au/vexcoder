use crate::config::Config;
use crate::prompts::{render_explain_prompt, CODER_SYSTEM_PROMPT};
use crate::runtime::command::PtySession;
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
use crate::runtime::validation::{ValidationSuite, VALIDATION_TAIL_BYTES};
use crate::runtime::{
    truncate_head_bytes, truncate_tail_bytes, ApprovalScope, Capability, CommandHandle,
    CommandRequest, CommandResult, CommandRunner, DefaultCommandRunner, EditLoopOutcome,
    PassthroughSandbox, SandboxDriver, TaskState, UiUpdate,
};
#[cfg(test)]
use crate::session_notes::resolve_notes_for_injection;
use crate::session_notes::{
    build_api_client_with_notes, resolve_notes_path_for_read, resolve_notes_path_for_write,
};
use crate::state::{ConversationManager, StreamBlock, ToolApprovalRequest};
use crate::tools::ToolOperator;
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

const DEFAULT_MAX_HISTORY_LINES: usize = 2000;
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

struct WorkingDirCommandRunner {
    working_dir: PathBuf,
    fallback: DefaultCommandRunner,
    sandbox: PassthroughSandbox,
}

impl WorkingDirCommandRunner {
    fn new(working_dir: PathBuf) -> Self {
        Self {
            working_dir,
            fallback: DefaultCommandRunner::new(),
            sandbox: PassthroughSandbox,
        }
    }
}

impl CommandRunner for WorkingDirCommandRunner {
    async fn run_one_shot(&self, req: CommandRequest) -> Result<CommandResult> {
        let CommandRequest {
            program,
            args,
            working_dir,
        } = req;
        let wrapped_req = self.sandbox.wrap(CommandRequest {
            program,
            args,
            working_dir: working_dir.or_else(|| Some(self.working_dir.clone())),
        })?;
        self.fallback.run_one_shot(wrapped_req).await
    }

    async fn run_streaming(
        &self,
        req: CommandRequest,
        tx: tokio::sync::mpsc::Sender<crate::runtime::OutputChunk>,
    ) -> Result<CommandHandle> {
        let CommandRequest {
            program,
            args,
            working_dir,
        } = req;
        let wrapped_req = self.sandbox.wrap(CommandRequest {
            program,
            args,
            working_dir: working_dir.or_else(|| Some(self.working_dir.clone())),
        })?;
        self.fallback.run_streaming(wrapped_req, tx).await
    }

    async fn cancel(&self, handle: CommandHandle) -> Result<()> {
        self.fallback.cancel(handle).await
    }

    fn attach_pty(&self, req: CommandRequest) -> Result<PtySession> {
        let CommandRequest {
            program,
            args,
            working_dir,
        } = req;
        let wrapped_req = self.sandbox.wrap(CommandRequest {
            program,
            args,
            working_dir: working_dir.or_else(|| Some(self.working_dir.clone())),
        })?;
        self.fallback.attach_pty(wrapped_req)
    }
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
pub struct TaskLayoutState {
    pub task_id: String,
    pub status_line: String,
    pub activity_rows: Vec<String>,
    pub output_rows: Vec<String>,
    pub pending_approval: Option<String>,
    pub changed_files: Vec<String>,
}

pub struct TuiMode {
    history_state: HistoryState,
    overlay_state: OverlayState,
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
    /// Working directory for workspace-relative commands like `/diff`.
    working_dir: PathBuf,
    last_assembled_context: Option<AssembledContext>,
    read_only_turn_active: bool,
    active_edit_loop: Option<EditLoop>,
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

pub fn capability_to_kebab(cap: Capability) -> &'static str {
    match cap {
        Capability::ReadFile => "read-file",
        Capability::WriteFile => "write-file",
        Capability::ApplyPatch => "apply-patch",
        Capability::RunCommand => "run-command",
        Capability::Network => "network",
        Capability::Browser => "browser",
    }
}

pub fn kebab_to_capability(s: &str) -> Option<Capability> {
    match s {
        "read-file" => Some(Capability::ReadFile),
        "write-file" => Some(Capability::WriteFile),
        "apply-patch" => Some(Capability::ApplyPatch),
        "run-command" => Some(Capability::RunCommand),
        "network" => Some(Capability::Network),
        "browser" => Some(Capability::Browser),
        _ => None,
    }
}

fn capability_for_tool_name(tool_name: &str) -> Option<Capability> {
    match tool_name {
        "read_file" | "list_files" | "list_directory" | "search" | "search_files"
        | "search_content" | "find_files" | "git_status" | "git_diff" | "git_log" | "git_show" => {
            Some(Capability::ReadFile)
        }
        "write_file" | "edit_file" | "rename_file" => Some(Capability::WriteFile),
        "apply_patch" | "git_add" | "git_commit" => Some(Capability::ApplyPatch),
        "run_command" => Some(Capability::RunCommand),
        _ => None,
    }
}

fn scope_to_label(scope: ApprovalScope) -> &'static str {
    match scope {
        ApprovalScope::Once => "once",
        ApprovalScope::Task => "task",
        ApprovalScope::Session => "session",
    }
}

fn kebab_to_scope(s: &str) -> Option<ApprovalScope> {
    match s {
        "once" => Some(ApprovalScope::Once),
        "session" => Some(ApprovalScope::Session),
        _ => None,
    }
}

fn shell_command_request(command: String, working_dir: PathBuf) -> CommandRequest {
    if cfg!(windows) {
        CommandRequest {
            program: "cmd".to_string(),
            args: vec!["/C".to_string(), command],
            working_dir: Some(working_dir),
        }
    } else {
        CommandRequest {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), command],
            working_dir: Some(working_dir),
        }
    }
}

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

async fn run_inline_shell_command(command: String, working_dir: PathBuf) -> Result<CommandResult> {
    let runner = WorkingDirCommandRunner::new(working_dir.clone());
    runner
        .run_one_shot(shell_command_request(command, working_dir))
        .await
}

fn format_inline_block(
    kind: &str,
    path: &str,
    content: &str,
    truncated: bool,
    byte_limit: Option<usize>,
) -> String {
    let mut rendered = format!("[{kind}: {path}]\n```text\n{content}\n```");
    if truncated {
        if let Some(limit) = byte_limit {
            rendered.push_str(&format!(
                "\n[{kind}: {path} \u{2014} truncated to {limit} bytes]"
            ));
        } else {
            rendered.push_str(&format!("\n[{kind}: {path} \u{2014} truncated]"));
        }
    }
    rendered
}

fn render_inline_command_result_lines(result: &CommandResult) -> Vec<String> {
    let mut rendered = String::new();

    for line in result.stdout.lines() {
        rendered.push_str("stdout: ");
        rendered.push_str(line);
        rendered.push('\n');
    }
    for line in result.stderr.lines() {
        rendered.push_str("stderr: ");
        rendered.push_str(line);
        rendered.push('\n');
    }

    let mut lines = Vec::new();
    let (tail, truncated) = truncate_tail_bytes(&rendered, VALIDATION_TAIL_BYTES);
    if truncated {
        lines.push(format!(
            "[output truncated \u{2014} showing last {} bytes]",
            VALIDATION_TAIL_BYTES
        ));
    }
    lines.extend(tail.lines().map(ToOwned::to_owned));
    lines.push(format!("[exit: {}]", result.exit_code));
    lines
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
        Self {
            history_state: HistoryState::default(),
            overlay_state: OverlayState::default(),
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
            working_dir: config.working_dir.clone(),
            last_assembled_context: None,
            read_only_turn_active: false,
            active_edit_loop: None,
            #[cfg(test)]
            last_turn_input: None,
        }
    }

    fn mode_status_label(&self) -> &'static str {
        if self.overlay_active() {
            "overlay"
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

    pub fn set_history_content_width(&self, width: usize) {
        self.history_content_width.set(width.max(1));
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

        let activity_rows = self
            .history_state
            .lines
            .iter()
            .rev()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        Some(TaskLayoutState {
            task_id: self.current_task.id.clone(),
            status_line: self.status_line(),
            activity_rows,
            output_rows: self.history_state.lines.clone(),
            pending_approval,
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
                        self.start_inline_command(command.command, ctx);
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
        self.history_state.active_assistant_index = None;
        self.history_state.scroll_offset = 0;
        self.history_state.auto_follow = true;
        self.active_stream_blocks.clear();
        self.last_assembled_context = None;
        self.read_only_turn_active = false;
    }

    fn apply_resumed_task(&mut self, state: TaskState, ctx: &RuntimeContext) {
        let restored_id = state.id.clone();
        let status = format!("{:?}", state.status);
        self.current_task = state;
        self.active_edit_loop = None;
        self.reset_conversation_window(ctx);
        self.push_history_line(format!("[resumed: {restored_id} status={status}]"));
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

    fn is_reentrant_edit_command(input: &str) -> bool {
        Self::registered_slash_command(input)
            .map(|(spec, _)| matches!(spec.id, SlashCommandId::Edit | SlashCommandId::Fix))
            .unwrap_or(false)
    }

    fn try_handle_slash_command(&mut self, input: &str, ctx: &mut RuntimeContext) -> bool {
        let Some((spec, args)) = Self::registered_slash_command(input) else {
            return false;
        };

        match spec.id {
            SlashCommandId::Quit | SlashCommandId::Exit => self.handle_quit_command(),
            SlashCommandId::About => self.handle_about_command(),
            SlashCommandId::MemoryShow => self.handle_memory_display(),
            SlashCommandId::MemoryAdd => {
                if args.is_empty() {
                    self.push_history_line("[memory] usage: /memory add <note>".to_string());
                } else {
                    self.handle_memory_add(args.to_string());
                }
            }
            SlashCommandId::MemoryClear => {
                self.overlay_state.pending_memory_clear = true;
                self.push_history_line(
                    "[memory] clear all notes? type y to confirm or n to cancel".to_string(),
                );
            }
            SlashCommandId::New => self.handle_new_command(ctx),
            SlashCommandId::Resume => self.handle_resume_command(args, ctx),
            SlashCommandId::Clear => self.handle_clear_command(ctx),
            SlashCommandId::Fork => self.handle_fork_command(args, ctx),
            SlashCommandId::Permissions => self.handle_permissions_command(),
            SlashCommandId::Allow => self.handle_allow_command(args),
            SlashCommandId::Deny => self.handle_deny_command(args),
            SlashCommandId::Model => self.handle_model_command(args, ctx),
            SlashCommandId::Diff => self.handle_diff_command(args),
            SlashCommandId::Edit => self.handle_edit_command(args, ctx),
            SlashCommandId::Fix => self.handle_fix_command(ctx),
            SlashCommandId::Explain => self.handle_explain_command(args, ctx),
            SlashCommandId::Run => self.handle_run_command(args),
            SlashCommandId::Test => self.handle_test_command(),
            SlashCommandId::Context => self.handle_context_command(ctx),
            SlashCommandId::Commands | SlashCommandId::Help => self.handle_commands_command(),
        }

        true
    }

    fn handle_edit_command(&mut self, instruction: &str, ctx: &mut RuntimeContext) {
        if self.active_edit_loop.is_some() && self.history_state.turn_in_progress {
            self.push_history_line(
                "[edit loop already active \u{2014} cancel with Ctrl+C before starting a new task]"
                    .to_string(),
            );
            return;
        }
        if instruction.is_empty() {
            self.push_history_line("[edit] usage: /edit <instruction>".to_string());
            return;
        }
        let task_id = self.current_task.id.clone();
        let edit_loop = EditLoop::new(task_id)
            .with_profile(ModelProfile::default_for_backend(self.model_backend));
        self.active_edit_loop = Some(edit_loop.clone());
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        #[cfg(test)]
        {
            self.last_turn_input = Some(instruction.to_string());
        }
        ctx.start_edit_loop(edit_loop, instruction.to_string());
    }

    fn handle_fix_command(&mut self, ctx: &mut RuntimeContext) {
        if self.active_edit_loop.is_some() && self.history_state.turn_in_progress {
            self.push_history_line(
                "[edit loop already active \u{2014} cancel with Ctrl+C before starting a new task]"
                    .to_string(),
            );
            return;
        }
        let last_result = self
            .active_edit_loop
            .as_ref()
            .and_then(|l| l.last_validation_result())
            .filter(|result| !result.passed)
            .cloned();
        let Some(result) = last_result else {
            self.push_history_line(
                "[no recent validation failure in this session \u{2014} run /edit or /test first]"
                    .to_string(),
            );
            return;
        };
        let instruction = result
            .outputs
            .iter()
            .find(|o| o.exit_code != 0)
            .map(|o| format!("fix the {} failure", o.label))
            .unwrap_or_else(|| "fix the validation failure".to_string());
        let task_id = self.current_task.id.clone();
        let edit_loop = EditLoop::new(task_id)
            .with_profile(ModelProfile::default_for_backend(self.model_backend));
        self.active_edit_loop = Some(edit_loop.clone());
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        #[cfg(test)]
        {
            self.last_turn_input = Some(instruction.clone());
        }
        ctx.start_edit_loop(edit_loop, instruction);
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
            self.start_inline_command(command.to_string(), ctx);
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
            self.start_inline_command(command.to_string(), ctx);
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

    fn start_inline_command(&mut self, command: String, ctx: &RuntimeContext) {
        self.history_state.turn_in_progress = true;
        self.history_state.cancel_pending = false;
        self.history_state.active_assistant_index = None;

        let ctx = ctx.clone();
        let cancel = ctx.turn_cancellation_token();
        let working_dir = self.working_dir.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {
                    ctx.emit_transcript_line("[shell] cancelled".to_string());
                }
                result = run_inline_shell_command(
                    command,
                    working_dir,
                ) => {
                    match result {
                        Ok(result) => {
                            for line in render_inline_command_result_lines(&result) {
                                ctx.emit_transcript_line(line);
                            }
                        }
                        Err(error) => ctx.emit_transcript_line(format!("[shell] error: {error}")),
                    }
                }
            }
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
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        self.read_only_turn_active = read_only;
        #[cfg(test)]
        {
            self.last_turn_input = Some(rendered.clone());
        }
        ctx.start_turn_with_system_prompt(
            rendered,
            supplementary_system_prompt.map(ToString::to_string),
        );
    }

    fn handle_explain_command(&mut self, path_hint: &str, ctx: &mut RuntimeContext) {
        let requested_path = if !path_hint.is_empty() {
            Some(path_hint.to_string())
        } else {
            self.current_task
                .changed_files
                .last()
                .map(|path| path.to_string_lossy().into_owned())
        };
        let scope_instruction = requested_path
            .as_deref()
            .map(|path| format!("explain {path}"))
            .unwrap_or_else(|| "explain the current workspace state".to_string());

        let assembler = ContextAssembler::default();
        let render_assembler = assembler.clone();
        let operator = ToolOperator::new(self.working_dir.clone());
        let scope_instruction_for_task = scope_instruction.clone();
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
        let rendered_context = assembled
            .as_ref()
            .map(|context| render_assembler.render(context))
            .unwrap_or_else(|| "## Context\n[context: unavailable]\n".to_string());

        let prompt = render_explain_prompt(&scope_instruction, &rendered_context);
        self.start_single_turn(prompt, ctx, true, Some(CODER_SYSTEM_PROMPT));
    }

    fn handle_run_command(&mut self, command_str: &str) {
        let suite = if command_str.is_empty() {
            let mut inferred = ValidationSuite::load_or_infer(&self.working_dir);
            inferred.commands.truncate(1);
            inferred
        } else {
            let mut parts = command_str.split_whitespace();
            let Some(program) = parts.next() else {
                self.push_history_line("[run] usage: /run [command]".to_string());
                return;
            };
            ValidationSuite {
                commands: vec![crate::runtime::ValidationCommand {
                    label: command_str.to_string(),
                    program: program.to_string(),
                    args: parts.map(ToString::to_string).collect(),
                    timeout_secs: 60,
                }],
            }
        };

        self.run_validation_suite_to_transcript(suite, "run", false);
    }

    fn handle_test_command(&mut self) {
        let suite = ValidationSuite::load_or_infer(&self.working_dir);
        self.run_validation_suite_to_transcript(suite, "test", true);
    }

    fn run_validation_suite_to_transcript(
        &mut self,
        suite: ValidationSuite,
        label: &str,
        remember_for_fix: bool,
    ) {
        if suite.commands.is_empty() {
            self.push_history_line(format!("[{label}] no commands configured"));
            return;
        }

        let runner = WorkingDirCommandRunner::new(self.working_dir.clone());
        match block_on_context_task(async move { suite.run(&runner).await }) {
            Ok(result) => {
                if remember_for_fix {
                    let mut edit_loop = self.active_edit_loop.clone().unwrap_or_else(|| {
                        EditLoop::new(self.current_task.id.clone())
                            .with_profile(ModelProfile::default_for_backend(self.model_backend))
                    });
                    edit_loop.set_last_validation_result(result.clone());
                    self.active_edit_loop = Some(edit_loop);
                }
                self.push_validation_result_lines(label, &result);
            }
            Err(error) => {
                self.push_history_line(format!("[{label}] error: {error}"));
            }
        }
    }

    fn push_validation_result_lines(
        &mut self,
        label: &str,
        result: &crate::runtime::ValidationResult,
    ) {
        for output in &result.outputs {
            let status = if output.exit_code == 0 {
                "ok".to_string()
            } else {
                format!("exit {}", output.exit_code)
            };
            self.push_history_line(format!("[{label}] {} [{status}]", output.label));
            if !output.stdout_tail.trim().is_empty() {
                for line in output.stdout_tail.lines() {
                    self.push_history_line(format!("  stdout: {line}"));
                }
            }
            if !output.stderr_tail.trim().is_empty() {
                for line in output.stderr_tail.lines() {
                    self.push_history_line(format!("  stderr: {line}"));
                }
            }
        }

        let summary = if result.passed {
            "all commands passed"
        } else {
            "one or more commands failed"
        };
        self.push_history_line(format!("[{label}] {summary}"));
    }

    fn handle_context_command(&mut self, ctx: &RuntimeContext) {
        let turns = if self.active_edit_loop.is_some() && self.history_state.turn_in_progress {
            "1".to_string()
        } else {
            "\u{2014}".to_string()
        };
        let profile_name = self
            .active_edit_loop
            .as_ref()
            .map(|edit_loop| edit_loop.profile_name())
            .unwrap_or("default")
            .to_string();
        let files = self
            .last_assembled_context
            .as_ref()
            .map(|context| context.file_snapshots.len())
            .unwrap_or(0);
        let git_summary = self.resolve_context_git_summary();

        self.push_history_line("[context]".to_string());
        self.push_history_line(format!("  model     : {}", self.model_name));
        self.push_history_line(format!("  backend   : {:?}", self.model_backend));
        self.push_history_line(format!("  profile   : {profile_name}"));
        self.push_history_line(format!("  task      : {}", self.current_task.id));
        self.push_history_line(format!("  status    : {:?}", self.current_task.status));
        self.push_history_line(format!("  turns     : {turns}"));
        self.push_history_line(format!("  files     : {files}"));
        self.push_history_line(format!("  git       : {git_summary}"));
        self.push_history_line(format!(
            "  approvals : {} active grant(s)",
            self.current_task.active_grants.len()
        ));
        self.push_history_line(format!(
            "  tokens    : ~{}",
            ctx.estimated_conversation_tokens()
        ));
    }

    fn resolve_context_git_summary(&self) -> String {
        let defaults = ContextAssembler::default();
        let timeout_ms = resolve_git_timeout_ms(defaults.git_timeout_ms);
        match block_on_context_task(run_git_command_with_timeout(
            self.working_dir.clone(),
            vec!["status".to_string(), "--short".to_string()],
            timeout_ms,
        )) {
            Ok(result) => {
                if result.non_git_repo {
                    "no git".to_string()
                } else if result.timed_out {
                    "timed out".to_string()
                } else {
                    result
                        .output
                        .and_then(|text| {
                            let first = text.lines().next().unwrap_or("clean").trim().to_string();
                            (!first.is_empty()).then_some(first)
                        })
                        .unwrap_or_else(|| "clean".to_string())
                }
            }
            Err(_) => "no git".to_string(),
        }
    }

    fn handle_commands_command(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.push_history_line("[commands]".to_string());
        for spec in SLASH_COMMANDS {
            if seen.insert(spec.display) {
                self.push_history_line(format!("  {:32} — {}", spec.display, spec.description));
            }
        }
    }

    /// PC-01: `/model <n>` — name-only switch within the same backend/protocol.
    fn handle_model_command(&mut self, name: &str, ctx: &RuntimeContext) {
        if name.is_empty() {
            self.push_history_line(format!("[model] {}", self.model_name));
            return;
        }
        // Models prefixed with `local/` are local-runtime-only; all other
        // names are assumed compatible with the API backend. Reject any
        // name that would require switching backends mid-session.
        let target_is_local = name.starts_with("local/");
        let current_is_local = self.model_backend == crate::runtime::ModelBackendKind::LocalRuntime;

        if target_is_local != current_is_local {
            let required_backend = if target_is_local {
                "local-runtime"
            } else {
                "api-server"
            };
            self.push_history_line(format!(
                "[model] rejected: '{}' requires {} backend \
                 (current: {:?}). Restart vex with the desired backend.",
                name, required_backend, self.model_backend,
            ));
            return;
        }

        if let Err(error) = ctx.set_model_name(name.to_string()) {
            self.push_history_line(format!("[model] error: {error}"));
            return;
        }

        let old = std::mem::replace(&mut self.model_name, name.to_string());
        self.push_history_line(format!("[model] {} -> {}", old, self.model_name));
    }

    /// PK-07: `/diff [--staged]` — show git diff output, truncated at 200 lines.
    fn handle_diff_command(&mut self, args: &str) {
        let diff_defaults = ContextAssembler::default();
        let max_diff_lines = diff_defaults.max_diff_lines;
        let timeout_ms = resolve_git_timeout_ms(diff_defaults.git_timeout_ms);
        let staged = match args.split_whitespace().collect::<Vec<_>>().as_slice() {
            [] => false,
            ["--staged"] | ["--cached"] => true,
            _ => {
                self.push_history_line("[diff] usage: /diff [--staged]".to_string());
                return;
            }
        };

        let git_args = if staged {
            vec!["diff".to_string(), "--cached".to_string()]
        } else {
            vec!["diff".to_string(), "HEAD".to_string()]
        };

        match block_on_context_task(run_git_command_with_timeout(
            self.working_dir.clone(),
            git_args,
            timeout_ms,
        )) {
            Ok(result) => {
                if result.non_git_repo {
                    self.push_history_line("[diff] not a git repository".to_string());
                    return;
                }
                if result.timed_out {
                    self.push_history_line(format!(
                        "[diff] error: git diff timed out after {timeout_ms}ms"
                    ));
                    return;
                }
                let Some(text) = result.output else {
                    self.push_history_line("[diff] error: git diff failed".to_string());
                    return;
                };
                if text.trim().is_empty() {
                    self.push_history_line("[diff] working tree is clean".to_string());
                    return;
                }

                let lines: Vec<&str> = text.lines().collect();
                for line in lines.iter().take(max_diff_lines) {
                    self.push_history_line(line.to_string());
                }
                if lines.len() > max_diff_lines {
                    self.push_history_line(format!(
                        "[diff truncated \u{2014} showing first {max_diff_lines} lines]"
                    ));
                }
            }
            Err(error) => {
                self.push_history_line(format!("[diff] error: {error}"));
            }
        }
    }

    fn handle_permissions_command(&mut self) {
        self.push_history_line("[permissions]".to_string());
        for &cap in ALL_CAPABILITIES {
            let cap_name = capability_to_kebab(cap);
            let scope_label = self
                .current_task
                .active_grants
                .get(&cap)
                .map(|scope| scope_to_label(*scope))
                .unwrap_or("(none)");
            self.push_history_line(format!("  {cap_name}  {scope_label}"));
        }
    }

    fn handle_allow_command(&mut self, rest: &str) {
        if rest.is_empty() {
            self.push_history_line(
                "[allow: usage: /allow <capability> [once|session]]".to_string(),
            );
            return;
        }
        let mut parts = rest.splitn(2, ' ');
        let cap_str = parts.next().unwrap_or("").trim();
        let scope_str = parts.next().unwrap_or("").trim();

        let Some(cap) = kebab_to_capability(cap_str) else {
            self.push_history_line(format!("[allow: unknown capability '{cap_str}']"));
            return;
        };

        let scope = if scope_str.is_empty() {
            ApprovalScope::Once
        } else {
            match kebab_to_scope(scope_str) {
                Some(s) => s,
                None => {
                    self.push_history_line(format!(
                        "[allow: unknown scope '{scope_str}'; valid: once | session]"
                    ));
                    return;
                }
            }
        };

        let scope_label = scope_to_label(scope);
        self.current_task.active_grants.insert(cap, scope);
        self.push_history_line(format!("[allow: {cap_str} granted for {scope_label}]"));
    }

    fn handle_deny_command(&mut self, rest: &str) {
        if rest.is_empty() {
            self.push_history_line("[deny: usage: /deny <capability>]".to_string());
            return;
        }
        let cap_str = rest.trim();
        let Some(cap) = kebab_to_capability(cap_str) else {
            self.push_history_line(format!("[deny: unknown capability '{cap_str}']"));
            return;
        };

        if self.current_task.active_grants.remove(&cap).is_some() {
            self.push_history_line(format!("[deny: {cap_str} removed]"));
        } else {
            self.push_history_line(format!("[deny: {cap_str} not in active grants]"));
        }
    }

    fn handle_new_command(&mut self, ctx: &mut RuntimeContext) {
        let dir = TaskState::state_dir();
        if let Err(e) = self.current_task.save(&dir) {
            self.push_history_line(format!("[new] save failed: {e} - session not reset"));
            return;
        }
        let new_id = new_task_id();
        self.current_task = TaskState::new(new_id.clone());
        self.active_edit_loop = None;
        self.reset_conversation_window(ctx);
        self.push_history_line(format!("[new session: {new_id}]"));
    }

    fn handle_resume_command(&mut self, task_id: &str, ctx: &mut RuntimeContext) {
        if task_id.is_empty() {
            let entries = list_recent_task_entries(5);
            if entries.is_empty() {
                self.push_history_line("[resume] no saved tasks found".to_string());
                return;
            }
            self.prompt_resume_selection(entries);
            return;
        }
        match TaskState::load_from_search_dirs(task_id) {
            Ok(state) => self.apply_resumed_task(state, ctx),
            Err(_) => {
                self.push_history_line(format!("[resume: task '{task_id}' not found]"));
            }
        }
    }

    fn handle_clear_command(&mut self, ctx: &mut RuntimeContext) {
        let task_id = self.current_task.id.clone();
        self.active_edit_loop = None;
        self.reset_conversation_window(ctx);
        self.push_history_line(format!(
            "[cleared: conversation history reset; task {task_id} continues]"
        ));
    }

    fn handle_fork_command(&mut self, label: &str, ctx: &mut RuntimeContext) {
        let dir = TaskState::state_dir();
        if let Err(e) = self.current_task.save(&dir) {
            self.push_history_line(format!("[fork] save failed: {e} - fork aborted"));
            return;
        }
        let sanitized_label = sanitize_task_label(label);
        let new_id = if sanitized_label.is_empty() {
            format!("{}-fork", new_task_id())
        } else {
            format!("{}-{sanitized_label}", new_task_id())
        };
        let parent_id = self.current_task.id.clone();
        let mut fork = TaskState::new(new_id.clone());
        fork.active_grants = self.current_task.active_grants.clone();
        fork.changed_files = self.current_task.changed_files.clone();
        fork.status = self.current_task.status.clone();
        self.current_task = fork;
        self.reset_conversation_window(ctx);
        self.push_history_line(format!("[fork: {new_id} branched from {parent_id}]"));
    }

    fn handle_quit_command(&mut self) {
        self.quit_requested = true;
    }

    fn handle_about_command(&mut self) {
        let version = env!("CARGO_PKG_VERSION");
        let commit = env!("GIT_COMMIT_SHORT");
        let build_date = env!("BUILD_DATE");
        self.push_history_line(format!("vex {version}"));
        self.push_history_line(format!("  build     : {build_date}"));
        self.push_history_line(format!("  commit    : {commit}"));
        self.push_history_line(format!("  repo      : {}", self.repo_label));
        self.push_history_line(format!(
            "  inst      : {}",
            self.instructions_path.as_deref().unwrap_or("none")
        ));
    }

    fn resolved_notes_path(&self) -> Option<PathBuf> {
        resolve_notes_path_for_write(self.notes_path.as_deref())
    }

    fn resolved_existing_notes_path(&self) -> Option<PathBuf> {
        resolve_notes_path_for_read(self.notes_path.as_deref())
    }

    fn handle_memory_display(&mut self) {
        let content = self
            .resolved_existing_notes_path()
            .and_then(|path| std::fs::read_to_string(path).ok());
        match content {
            Some(content) if !content.trim().is_empty() => {
                for line in content.lines() {
                    self.push_history_line(line.to_string());
                }
            }
            _ => {
                self.push_history_line("[memory] no notes".to_string());
            }
        }
    }

    fn handle_memory_add(&mut self, note: String) {
        if note.is_empty() {
            self.push_history_line("[memory] usage: /memory add <note>".to_string());
            return;
        }
        let path = self
            .resolved_existing_notes_path()
            .or_else(|| self.resolved_notes_path());
        let Some(path) = path else {
            self.push_history_line("[memory] error resolving notes path".to_string());
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(mut f) => {
                if let Err(e) = writeln!(f, "{}", note) {
                    self.push_history_line(format!("[memory] error writing: {e}"));
                    return;
                }
                self.push_history_line("[memory: note added]".to_string());
            }
            Err(e) => {
                self.push_history_line(format!("[memory] error opening file: {e}"));
            }
        }
    }

    fn handle_memory_clear_input(&mut self, input: &str) {
        self.overlay_state.pending_memory_clear = false;
        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => {
                let path = self
                    .resolved_existing_notes_path()
                    .or_else(|| self.resolved_notes_path());
                let Some(path) = path else {
                    self.push_history_line("[memory] error resolving notes path".to_string());
                    return;
                };
                if path.exists() {
                    if let Err(e) = std::fs::write(&path, "") {
                        self.push_history_line(format!("[memory] error clearing: {e}"));
                        return;
                    }
                }
                self.push_history_line("[memory: cleared]".to_string());
            }
            _ => {
                self.push_history_line("[memory: cancelled]".to_string());
            }
        }
    }
}

fn new_task_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static LAST_TASK_MS: AtomicU64 = AtomicU64::new(0);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let ms = LAST_TASK_MS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |previous| {
        Some(now_ms.max(previous.saturating_add(1)))
    });
    // `fetch_update` returns the previous value, so recompute the stored
    // monotonic millisecond from that prior state before formatting the id.
    let stable_ms = ms
        .map(|previous| now_ms.max(previous.saturating_add(1)))
        .unwrap_or(now_ms);

    format!("task-{stable_ms}")
}

fn resolve_history_line_cap() -> usize {
    std::env::var(MAX_HISTORY_LINES_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|cap| *cap > 0)
        .unwrap_or(DEFAULT_MAX_HISTORY_LINES)
}

fn resolve_repo_label() -> String {
    std::env::var("VEX_REPO_LABEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or_else(|| "workspace".to_string())
}

fn sanitize_task_label(label: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;

    for ch in label.trim().chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if matches!(ch, '-' | '_' | ' ') {
            Some('-')
        } else {
            None
        };

        let Some(ch) = normalized else {
            continue;
        };
        if ch == '-' {
            if out.is_empty() || last_was_dash {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }
        out.push(ch);
    }

    out.trim_matches('-').to_string()
}

fn list_recent_task_entries(limit: usize) -> Vec<ResumeTaskEntry> {
    TaskState::state_files()
        .into_iter()
        .take(limit)
        .map(|file| match TaskState::load(&file.dir, &file.id) {
            Ok(state) => ResumeTaskEntry {
                id: state.id,
                status: format!("{:?}", state.status),
            },
            Err(_) => ResumeTaskEntry {
                id: file.id,
                status: "Unreadable".to_string(),
            },
        })
        .collect()
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
                    "[busy - cancelling current turn, input discarded]".to_string(),
                );
            } else {
                self.push_history_line("[busy - turn in progress, input discarded]".to_string());
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

        #[cfg(test)]
        {
            self.last_turn_input = Some(turn_input.clone());
        }

        ctx.start_turn(turn_input);
    }

    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::TranscriptLine(line) => {
                self.push_history_line(line);
            }
            UiUpdate::StreamDelta(text) => {
                if self.history_state.cancel_pending {
                    return;
                }
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
            UiUpdate::TurnComplete => {
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
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
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.history_state.cancel_pending = false;
                self.push_history_line(format!("[error] {msg}"));
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
            self.push_history_line("[turn cancellation requested]".to_string());
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
    runtime
        .mode
        .push_history_line(format!("[resumed: {restored_id} status={status}]"));
    Ok((runtime, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::ui::editor::{InputAction, InputEditor};
    use crossterm::event::KeyEvent;
    use futures::FutureExt;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn setup_ctx() -> RuntimeContext {
        let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        RuntimeContext::new(conversation, tx, CancellationToken::new())
    }

    fn setup_ctx_with_updates() -> (RuntimeContext, mpsc::UnboundedReceiver<UiUpdate>) {
        let (tx, rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        (
            RuntimeContext::new(conversation, tx, CancellationToken::new()),
            rx,
        )
    }

    fn setup_ctx_with_responses(responses: Vec<Vec<String>>) -> RuntimeContext {
        let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(responses)));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        RuntimeContext::new(conversation, tx, CancellationToken::new())
    }

    fn successful_run_input() -> String {
        if cfg!(windows) {
            "/run cmd /C exit 0".to_string()
        } else {
            "/run sh -c true".to_string()
        }
    }

    fn successful_bang_input() -> String {
        "!echo inline-shell".to_string()
    }

    async fn drain_until_turn_complete(
        mode: &mut TuiMode,
        ctx: &mut RuntimeContext,
        rx: &mut mpsc::UnboundedReceiver<UiUpdate>,
    ) {
        loop {
            let update = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .expect("timed out waiting for ui update")
                .expect("ui update channel closed");
            let done = matches!(update, UiUpdate::TurnComplete | UiUpdate::Error(_));
            mode.on_model_update(update, ctx);
            if done {
                break;
            }
        }
    }

    #[derive(Clone)]
    struct RecordingSandbox {
        wrapped: Arc<AtomicBool>,
    }

    impl SandboxDriver for RecordingSandbox {
        fn wrap(&self, request: CommandRequest) -> Result<CommandRequest> {
            self.wrapped.store(true, Ordering::SeqCst);
            Ok(request)
        }
    }

    #[tokio::test]
    async fn test_ref_03_tui_mode_overlay_blocks_input() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();

        let (response_tx, _rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        mode.on_user_input("blocked".to_string(), &mut ctx);
        assert!(
            !mode.history_state.turn_in_progress,
            "overlay must block input dispatch"
        );

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(
            !mode.overlay_active(),
            "overlay should clear after decision"
        );

        mode.on_user_input("resume".to_string(), &mut ctx);
        assert!(
            mode.history_state.turn_in_progress,
            "dispatch should resume after overlay clears"
        );
    }

    #[test]
    fn overlay_blocks_submit() {
        let overlay_none = overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(
            overlay_none.is_none(),
            "overlay keymap must not route Enter as normal submit"
        );

        match overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Char('1'),
            KeyModifiers::NONE,
        ))) {
            Some(UserInputEvent::Text(value)) => assert_eq!(value, "1"),
            _ => panic!("overlay key '1' must route to modal action"),
        }

        match overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))) {
            Some(UserInputEvent::Text(value)) => assert_eq!(value, "esc"),
            _ => panic!("overlay Esc must route to modal deny action"),
        }
    }

    #[test]
    fn approval_selection_parser_handles_shared_overlay_inputs() {
        assert_eq!(
            parse_approval_selection("1"),
            Some(ApprovalSelection::ApproveOnce)
        );
        assert_eq!(
            parse_approval_selection("yes"),
            Some(ApprovalSelection::ApproveOnce)
        );
        assert_eq!(
            parse_approval_selection("2"),
            Some(ApprovalSelection::ApproveSession)
        );
        assert_eq!(
            parse_approval_selection("always"),
            Some(ApprovalSelection::ApproveSession)
        );
        assert_eq!(parse_approval_selection("3"), Some(ApprovalSelection::Deny));
        assert_eq!(
            parse_approval_selection("esc"),
            Some(ApprovalSelection::Deny)
        );
        assert_eq!(parse_approval_selection("later"), None);
    }

    #[test]
    fn test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("hello".to_string(), &mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);

        assert_eq!(mode.history_state.lines[0], "> hello");
        assert_eq!(mode.history_state.lines[1], "assistant");
    }

    #[test]
    fn test_stream_delta_strips_tagged_tool_markup_from_history() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("show diff".to_string(), &mut ctx);
        mode.on_model_update(
            UiUpdate::StreamDelta(
                "I will check.\n<function=git_diff>\n</function>\nDone.".to_string(),
            ),
            &mut ctx,
        );

        assert_eq!(mode.history_state.lines[1], "I will check.\n\nDone.");
        assert!(!mode.history_state.lines[1].contains("<function="));
    }

    #[test]
    fn test_stream_delta_hides_incomplete_tool_tag_suffix() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("status".to_string(), &mut ctx);
        mode.on_model_update(
            UiUpdate::StreamDelta("Checking\n<function=git_status".to_string()),
            &mut ctx,
        );

        assert_eq!(mode.history_state.lines[1], "Checking\n");
    }

    #[test]
    fn test_transcript_does_not_exceed_cap_after_n_turns() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var(MAX_HISTORY_LINES_ENV, "10");

        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        assert_eq!(mode.history_line_cap, 10);

        for i in 0..20 {
            mode.on_user_input(format!("user-{i}"), &mut ctx);
            assert!(
                mode.history_state.lines.len() <= 10,
                "history must be capped after on_user_input"
            );
            if let Some(idx) = mode.history_state.active_assistant_index {
                assert!(
                    idx < mode.history_state.lines.len(),
                    "active assistant index must remain valid after cap enforcement"
                );
            }

            mode.on_model_update(UiUpdate::StreamDelta(format!("assistant-{i}")), &mut ctx);
            assert!(
                mode.history_state.lines.len() <= 10,
                "history must be capped after stream update"
            );
            if let Some(idx) = mode.history_state.active_assistant_index {
                assert!(
                    idx < mode.history_state.lines.len(),
                    "active assistant index must remain valid during streaming"
                );
            }

            mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
            assert!(
                mode.history_state.lines.len() <= 10,
                "history must stay capped after turn completion"
            );
        }

        std::env::remove_var(MAX_HISTORY_LINES_ENV);
    }

    #[test]
    fn test_history_cap_env_invalid_uses_default() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var(MAX_HISTORY_LINES_ENV, "invalid-cap");

        let mode = TuiMode::new();
        assert_eq!(mode.history_line_cap, DEFAULT_MAX_HISTORY_LINES);

        std::env::remove_var(MAX_HISTORY_LINES_ENV);
    }

    #[test]
    fn test_scrollback_retains_position_during_streaming() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.history_state.lines = (0..20).map(|i| format!("line-{i}")).collect();
        mode.history_state.active_assistant_index = Some(10);
        mode.history_state.scroll_offset = 5;
        mode.history_state.auto_follow = false;

        mode.on_model_update(UiUpdate::StreamDelta(" assistant".to_string()), &mut ctx);

        assert_eq!(
            mode.history_state.scroll_offset, 5,
            "scrollback position must not be forced while auto-follow is disabled"
        );
    }

    #[test]
    fn test_scrollback_commands_update_scroll_state() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.history_state.lines = (0..100).map(|i| format!("line-{i}")).collect();
        mode.history_state.scroll_offset = 80;
        mode.history_state.auto_follow = true;

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageUp(10),
            },
            &mut ctx,
        );
        assert_eq!(mode.history_state.scroll_offset, 70);
        assert!(!mode.history_state.auto_follow);

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::PageDown(200),
            },
            &mut ctx,
        );
        assert_eq!(mode.history_state.scroll_offset, 99);
        assert!(mode.history_state.auto_follow);

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::Home,
            },
            &mut ctx,
        );
        assert_eq!(mode.history_state.scroll_offset, 0);
        assert!(!mode.history_state.auto_follow);

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::History,
                action: ScrollAction::End,
            },
            &mut ctx,
        );
        assert_eq!(mode.history_state.scroll_offset, 99);
        assert!(mode.history_state.auto_follow);
        assert!(
            !mode.history_state.turn_in_progress,
            "scroll commands must not dispatch new turns"
        );
    }

    #[test]
    fn test_history_status_and_scroll_use_visual_rows() {
        let mode = TuiMode {
            history_state: HistoryState {
                lines: vec!["a\nb\nc".to_string()],
                ..HistoryState::default()
            },
            ..TuiMode::new()
        };

        assert_eq!(mode.max_scroll_offset(), 2);
        assert!(mode.status_line().contains("history:3"));
    }

    #[test]
    fn test_idle_interrupt_shows_feedback() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        assert!(!mode.history_state.turn_in_progress);
        assert!(!mode.pending_quit);
        assert!(!mode.quit_requested);

        mode.on_interrupt(&mut ctx);
        assert!(mode.pending_quit, "first idle interrupt must arm quit");
        assert!(!mode.quit_requested, "first idle interrupt must not quit");
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line.contains("[press Ctrl+C again to exit]")),
            "first idle interrupt must show user-visible feedback"
        );

        mode.on_interrupt(&mut ctx);
        assert!(
            mode.quit_requested,
            "second idle interrupt must request quit"
        );
        assert!(
            mode.quit_requested(),
            "frontend quit path must observe mode quit request"
        );
    }

    #[test]
    fn test_input_drop_shows_feedback() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.history_state.turn_in_progress = true;
        mode.on_user_input("hello".to_string(), &mut ctx);

        assert!(
            mode.history_state.turn_in_progress,
            "busy input must not start a new turn"
        );
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line.starts_with("[busy")),
            "busy input must produce visible rejection feedback"
        );
        assert!(
            !mode
                .history_state
                .lines
                .iter()
                .any(|line| line == "> hello"),
            "discarded busy input must not be appended as user message"
        );
    }

    #[test]
    fn test_pending_quit_resets_on_new_turn_accept() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_interrupt(&mut ctx);
        assert!(mode.pending_quit);

        mode.on_user_input("resume".to_string(), &mut ctx);
        assert!(
            !mode.pending_quit,
            "pending quit must reset when a new turn is accepted"
        );
        assert!(!mode.quit_requested);
        assert!(mode.history_state.turn_in_progress);
    }

    #[test]
    fn overlay_renders_after_base_panes() {
        let mode = TuiMode::new();
        assert_eq!(
            render_pass_order(&mode),
            vec![RenderPass::Header, RenderPass::History, RenderPass::Input]
        );

        let mut overlay_mode = TuiMode::new();
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
        overlay_mode.overlay_state.pending_approval = Some(PendingApproval {
            tool_name: "read_file".to_string(),
            input_preview: "{\"path\":\"Cargo.toml\"}".to_string(),
            action: PendingApprovalAction::Tool(response_tx),
        });
        assert_eq!(
            render_pass_order(&overlay_mode),
            vec![
                RenderPass::Header,
                RenderPass::History,
                RenderPass::Input,
                RenderPass::Overlay,
            ],
            "overlay must always render last"
        );
    }

    #[test]
    fn test_render_not_called_when_state_unchanged() {
        let start = Instant::now();
        let mut guard = RenderGuard::with_intervals(
            Duration::from_millis(500),
            Duration::from_millis(120),
            start,
        );

        assert!(
            guard.should_draw(start, 11),
            "first render should draw because the guard starts dirty"
        );
        assert!(
            !guard.should_draw(start + Duration::from_millis(20), 11),
            "unchanged state before tick interval must not draw"
        );
        assert!(
            !guard.should_draw(start + Duration::from_millis(100), 11),
            "unchanged state still below tick interval must not draw"
        );
        assert!(
            guard.should_draw(start + Duration::from_millis(121), 11),
            "unchanged state should draw when tick interval elapses"
        );
        assert!(
            guard.should_draw(start + Duration::from_millis(122), 12),
            "changed state should mark dirty and draw immediately"
        );
    }

    #[test]
    fn test_render_guard_poll_timeout_uses_min_tick_interval() {
        let guard = RenderGuard::with_intervals(
            Duration::from_millis(500),
            Duration::from_millis(120),
            Instant::now(),
        );
        assert_eq!(guard.poll_timeout(), Duration::from_millis(120));
    }

    #[test]
    fn header_stable_during_streaming() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        let ready_status = mode.status_line();
        assert!(
            ready_status.contains("mode:ready"),
            "ready state must publish mode token"
        );
        assert!(
            ready_status.contains("approval:none"),
            "ready state must publish approval token"
        );
        assert!(
            ready_status.contains("history:0"),
            "ready state must publish history count"
        );
        assert!(
            ready_status.contains("repo:"),
            "ready state must publish repo token"
        );
        assert_eq!(
            render_pass_order(&mode).first(),
            Some(&RenderPass::Header),
            "header row must remain first in render order"
        );

        mode.on_user_input("hello".to_string(), &mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);
        let streaming_status = mode.status_line();
        assert!(
            streaming_status.contains("mode:streaming"),
            "streaming state must publish mode token"
        );
        assert!(
            streaming_status.contains("approval:none"),
            "streaming state must preserve approval token"
        );
        assert!(
            streaming_status.contains("history:2"),
            "streaming state must keep compact history count"
        );
        assert_eq!(
            render_pass_order(&mode).first(),
            Some(&RenderPass::Header),
            "header row must remain first while streaming"
        );

        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );
        let overlay_status = mode.status_line();
        assert!(
            overlay_status.contains("mode:overlay"),
            "overlay state must publish overlay mode token"
        );
        assert!(
            overlay_status.contains("approval:pending"),
            "overlay state must publish pending approval token"
        );
        assert_eq!(
            render_pass_order(&mode).first(),
            Some(&RenderPass::Header),
            "header row must remain first under overlay"
        );
    }

    #[test]
    fn multiline_submit_outside_overlay_only() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        let mut editor = InputEditor::new();

        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        let submitted = match editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            InputAction::Submit(value) => value,
            _ => panic!("enter outside overlay must submit multiline buffer"),
        };
        assert_eq!(submitted, "a\nb\nc");

        mode.on_user_input(submitted.clone(), &mut ctx);
        assert!(
            mode.history_state.turn_in_progress,
            "outside overlay, enter must submit and start a turn"
        );
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line == "> a\nb\nc"),
            "submitted multiline prompt should be recorded in history"
        );

        mode.history_state.turn_in_progress = false;
        mode.history_state.active_assistant_index = None;
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.overlay_state.pending_approval = Some(PendingApproval {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            action: PendingApprovalAction::Tool(response_tx),
        });

        let overlay_enter = overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(
            overlay_enter.is_none(),
            "enter in overlay keymap must not route to submit"
        );

        mode.on_user_input("overlay\nattempt".to_string(), &mut ctx);
        assert!(
            mode.overlay_active(),
            "overlay should remain active after non-decision input"
        );
        assert!(
            !mode
                .history_state
                .lines
                .iter()
                .any(|line| line == "> overlay\nattempt"),
            "overlay-focused input must not submit as a user prompt"
        );
    }

    #[test]
    fn history_stable_during_overlay() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        let mut editor = InputEditor::new();

        editor.input_state.buffer = "first".to_string();
        let _ = editor.submit();
        editor.input_state.buffer = "second".to_string();
        let _ = editor.submit();
        editor.input_state.buffer = "draft".to_string();
        editor.input_state.cursor = editor.input_state.buffer.len();

        editor.history_up();
        let before_overlay_buffer = editor.input_state.buffer.clone();
        let before_overlay_index = editor.input_state.history_index;
        let before_overlay_history_len = editor.input_state.history.len();

        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.overlay_state.pending_approval = Some(PendingApproval {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            action: PendingApprovalAction::Tool(response_tx),
        });
        assert!(mode.overlay_active());

        let up =
            overlay_event_to_user_input(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        let down = overlay_event_to_user_input(Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        )));
        assert!(
            up.is_none(),
            "overlay keymap must consume history navigation"
        );
        assert!(
            down.is_none(),
            "overlay keymap must consume history navigation"
        );

        assert_eq!(editor.input_state.buffer, before_overlay_buffer);
        assert_eq!(editor.input_state.history_index, before_overlay_index);
        assert_eq!(editor.input_state.history.len(), before_overlay_history_len);

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(!mode.overlay_active(), "overlay should clear on decision");

        editor.history_down();
        assert_eq!(editor.input_state.history_index, None);
        assert_eq!(
            editor.input_state.buffer, "draft",
            "prompt draft must restore after overlay transition"
        );
    }

    #[tokio::test]
    async fn diff_overlay_scrolls() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        let patch_preview = [
            "@@ -1,3 +1,4".to_string(),
            " context line".to_string(),
            "-old value".to_string(),
            "+new value".to_string(),
            " context tail".to_string(),
            "-removed again".to_string(),
            "+added again".to_string(),
        ]
        .join("\n");

        let (approve_tx, approve_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.overlay_state.pending_patch_approval = Some(PendingPatchApproval {
            patch_preview: patch_preview.clone(),
            scroll_offset: 0,
            response_tx: Some(approve_tx),
        });

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineDown,
            },
            &mut ctx,
        );
        assert_eq!(
            mode.overlay_state
                .pending_patch_approval
                .as_ref()
                .map(|p| p.scroll_offset),
            Some(1),
            "down must advance diff overlay scroll"
        );

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageDown(3),
            },
            &mut ctx,
        );
        assert_eq!(
            mode.overlay_state
                .pending_patch_approval
                .as_ref()
                .map(|p| p.scroll_offset),
            Some(4),
            "page down must advance by requested step"
        );

        mode.on_frontend_event(
            UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::End,
            },
            &mut ctx,
        );
        assert_eq!(
            mode.overlay_state
                .pending_patch_approval
                .as_ref()
                .map(|p| p.scroll_offset),
            Some(patch_preview.lines().count().saturating_sub(1)),
            "end must jump to last diff line"
        );

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(
            approve_rx.await.expect("patch approval should resolve"),
            "approve binding must resolve true"
        );
        assert!(
            !mode.patch_overlay_active(),
            "overlay must clear after approve decision"
        );

        let (deny_tx, deny_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.overlay_state.pending_patch_approval = Some(PendingPatchApproval {
            patch_preview,
            scroll_offset: 2,
            response_tx: Some(deny_tx),
        });
        mode.on_user_input("n".to_string(), &mut ctx);
        assert!(
            !deny_rx.await.expect("patch denial should resolve"),
            "deny binding must resolve false"
        );
        assert!(
            !mode.patch_overlay_active(),
            "overlay must clear after deny decision"
        );
    }

    #[test]
    fn input_pane_expands_then_clamps_to_max_rows() {
        assert_eq!(input_rows_for_buffer("", 80), 1);

        let multiline = (0..12)
            .map(|idx| format!("line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            input_rows_for_buffer(&multiline, 80),
            MAX_INPUT_PANE_ROWS as u16
        );
    }

    #[test]
    fn test_editor_cursor_navigation() {
        let mut editor = InputEditor::new();
        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "aXbc");
    }

    #[test]
    fn test_editor_history_up_down() {
        let mut editor = InputEditor::new();
        editor.input_state.buffer = "first".to_string();
        let _ = editor.submit();
        editor.input_state.buffer = "second".to_string();
        let _ = editor.submit();

        editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "second");
        editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "first");
        editor.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "second");
    }

    #[test]
    fn test_editor_history_stash_restore() {
        let mut editor = InputEditor::new();

        editor.input_state.buffer = "first".to_string();
        let _ = editor.submit();
        editor.input_state.buffer = "second".to_string();
        let _ = editor.submit();

        editor.input_state.buffer = "draft".to_string();
        editor.input_state.cursor = editor.input_state.buffer.len();

        editor.history_up();
        assert_eq!(editor.input_state.buffer, "second");
        assert_eq!(editor.input_state.history_index, Some(1));

        editor.history_down();
        assert_eq!(editor.input_state.history_index, None);
        assert_eq!(editor.input_state.buffer, "draft");
        assert_eq!(editor.input_state.cursor, "draft".len());
    }

    #[test]
    fn test_editor_multiline_shortcuts() {
        let mut editor = InputEditor::new();
        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(editor.input_state.buffer, "a\nb\nc");
    }

    #[test]
    fn test_editor_undo_redo() {
        let mut editor = InputEditor::new();
        editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        editor.apply_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(editor.input_state.buffer, "a");
        editor.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!(editor.input_state.buffer, "ab");
    }

    #[test]
    fn test_editor_paste_handling() {
        let mut editor = InputEditor::new();
        let _ = editor.apply_event(Event::Paste("hello".to_string()));
        assert_eq!(editor.input_state.buffer, "hello");
    }

    #[test]
    fn test_input_editor_unicode_cursor_backspace_delete_safe() {
        let mut editor = InputEditor::new();
        editor.insert_str("a\u{1F600}b");
        editor.input_state.cursor = editor.input_state.buffer.len();
        editor.backspace();
        assert_eq!(editor.input_state.buffer, "a\u{1F600}");
        editor.backspace();
        assert_eq!(editor.input_state.buffer, "a");

        editor.insert_str("\u{1F600}b");
        editor.input_state.cursor = 2; // intentionally non-boundary (inside emoji codepoint)
        editor.delete();
        assert_eq!(editor.input_state.buffer, "ab");
    }

    #[tokio::test]
    async fn test_invalid_approval_input_keeps_overlay_active_with_feedback() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        mode.on_user_input("x".to_string(), &mut ctx);
        assert!(
            mode.overlay_active(),
            "overlay should stay active on invalid input"
        );
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line.contains("[invalid selection, expected 1/2/3]")),
            "expected invalid selection feedback line"
        );
    }

    #[tokio::test]
    async fn test_interrupt_is_typed_event_not_magic_string_collision() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();

        mode.on_user_input("__VEX_INTERRUPT__".to_string(), &mut ctx);
        assert!(
            mode.history_state.turn_in_progress,
            "plain text matching old sentinel must be treated as normal user input"
        );

        mode.on_interrupt(&mut ctx);
        assert!(
            mode.history_state.turn_in_progress,
            "typed interrupt should keep turn active until TurnComplete drains"
        );
        assert!(
            mode.history_state.cancel_pending,
            "typed interrupt should arm cancel-pending state"
        );
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line.contains("[turn cancellation requested]")),
            "cancel path should provide visible feedback"
        );

        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
        assert!(!mode.history_state.turn_in_progress);
        assert!(!mode.history_state.cancel_pending);
    }

    #[test]
    fn test_stream_delta_ignored_without_active_turn_slot() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_model_update(UiUpdate::StreamDelta("ghost delta".to_string()), &mut ctx);
        assert!(
            mode.history_state.lines.is_empty(),
            "stale stream deltas must be ignored after turn completion/cancel"
        );
    }

    #[test]
    fn test_cancel_pending_blocks_stream_delta_appends() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("hello".to_string(), &mut ctx);
        mode.on_interrupt(&mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("stale".to_string()), &mut ctx);
        assert_eq!(mode.history_state.lines[0], "> hello");
        assert_eq!(mode.history_state.lines[1], "");
    }

    #[tokio::test]
    async fn test_tool_approval_accept_once() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );
        mode.on_user_input("1".to_string(), &mut ctx);

        assert!(response_rx.await.expect("response should resolve"));
    }

    #[tokio::test]
    async fn test_tool_approval_deny() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );
        mode.on_user_input("n".to_string(), &mut ctx);

        assert!(!response_rx.await.expect("response should resolve"));
    }

    #[tokio::test]
    async fn approval_sender_resolved_exactly_once() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();

        let (first_tx, first_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "read_file".to_string(),
                input_preview: "first".to_string(),
                response_tx: first_tx,
            }),
            &mut ctx,
        );

        let mut first_rx = Box::pin(first_rx);
        assert!(
            first_rx.as_mut().now_or_never().is_none(),
            "first approval sender must remain unresolved while overlay is active"
        );

        let (second_tx, second_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "write_file".to_string(),
                input_preview: "second".to_string(),
                response_tx: second_tx,
            }),
            &mut ctx,
        );

        assert!(
            !first_rx
                .await
                .expect("first sender should resolve when replaced"),
            "replaced approval sender must resolve false exactly once"
        );

        let mut second_rx = Box::pin(second_rx);
        assert!(
            second_rx.as_mut().now_or_never().is_none(),
            "second approval sender must remain unresolved before decision"
        );

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(
            second_rx
                .await
                .expect("second sender should resolve on accept"),
            "approved overlay should resolve true exactly once"
        );

        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
        mode.on_model_update(UiUpdate::Error("post-resolution".to_string()), &mut ctx);
        assert!(
            !mode.overlay_active(),
            "overlay lifecycle should clear cleanly after sender resolution"
        );
    }

    #[test]
    fn test_tui_memory_renders_empty_notes() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new_with_notes(Some(notes_path));
        mode.on_user_input("/memory".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[memory] no notes")),
            "expected '[memory] no notes' in history"
        );
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_tui_memory_add_appends_to_file() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
        mode.on_user_input("/memory add hello world".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[memory: note added]")),
            "expected '[memory: note added]' in history"
        );
        let content = std::fs::read_to_string(&notes_path).unwrap();
        assert!(content.contains("hello world"));
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_tui_memory_clear_requires_confirmation() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        std::fs::write(&notes_path, "existing note\n").unwrap();
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
        mode.on_user_input("/memory clear".to_string(), &mut ctx);
        assert!(
            mode.pending_memory_clear_overlay(),
            "memory clear must enter overlay state"
        );
        assert!(
            mode.overlay_active(),
            "overlay must be active during memory clear"
        );
        // File must not be cleared until confirmed
        let content = std::fs::read_to_string(&notes_path).unwrap();
        assert!(content.contains("existing note"));
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_tui_memory_clear_cancellable() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        std::fs::write(&notes_path, "keep this note\n").unwrap();
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
        mode.on_user_input("/memory clear".to_string(), &mut ctx);
        mode.on_user_input("n".to_string(), &mut ctx);
        assert!(
            !mode.pending_memory_clear_overlay(),
            "overlay must clear after cancel"
        );
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[memory: cancelled]")),
            "expected '[memory: cancelled]' in history"
        );
        let content = std::fs::read_to_string(&notes_path).unwrap();
        assert!(
            content.contains("keep this note"),
            "file must not be cleared on cancel"
        );
    }

    #[test]
    fn test_tui_memory_does_not_call_start_turn() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        std::fs::write(&notes_path, "a note\n").unwrap();
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));

        // /memory
        mode.on_user_input("/memory".to_string(), &mut ctx);
        assert!(!mode.is_turn_in_progress(), "/memory must not start a turn");

        // /memory add
        mode.on_user_input("/memory add another".to_string(), &mut ctx);
        assert!(
            !mode.is_turn_in_progress(),
            "/memory add must not start a turn"
        );

        // /memory clear + cancel
        mode.on_user_input("/memory clear".to_string(), &mut ctx);
        assert!(
            !mode.is_turn_in_progress(),
            "/memory clear must not start a turn"
        );
        mode.on_user_input("n".to_string(), &mut ctx);
        assert!(!mode.is_turn_in_progress(), "cancel must not start a turn");
    }

    #[test]
    fn test_tui_memory_reads_legacy_fallback_notes() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join(".vex")).unwrap();
        std::fs::write(home.join(".vex/memory.md"), "legacy note\n").unwrap();

        let old_home = std::env::var("HOME").ok();
        let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("HOME", home.as_os_str());
        std::env::remove_var("XDG_CONFIG_HOME");

        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new_with_notes(None);
        mode.on_user_input("/memory".to_string(), &mut ctx);

        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_xdg {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }

        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains("legacy note")),
            "expected legacy fallback notes to render"
        );
    }

    #[test]
    fn test_memory_injection_within_budget_returns_content() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        std::fs::write(&notes_path, "my project note\n").unwrap();
        let (content, warning) = resolve_notes_for_injection(Some(notes_path.as_path()), 2048);
        assert!(warning.is_none(), "notes within budget must not warn");
        let content = content.as_deref().unwrap_or("");
        assert!(
            content.contains("my project note"),
            "notes content must be returned for system prompt injection"
        );
    }

    #[test]
    fn test_memory_injection_over_budget_emits_startup_warning() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        let big_content = "x".repeat((2048 * 4) + 1);
        std::fs::write(&notes_path, &big_content).unwrap();

        let config = Config {
            model_token: None,
            model_name: "mock-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            working_dir: temp.path().to_path_buf(),
            model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
            model_protocol: crate::runtime::ModelProtocol::MessagesV1,
            tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            model_headers: reqwest::header::HeaderMap::new(),
            notes_path: Some(notes_path),
            hooks: Vec::new(),
        };

        let (runtime, _ctx) = build_runtime(config).expect("runtime should build");
        let has_warning = runtime
            .mode
            .history_lines()
            .iter()
            .any(|l| l.contains("notes exceed token budget"));
        assert!(has_warning, "expected startup budget warning in history");
    }

    // -- PI-04 / PI-05 / PJ-01 / PJ-02 ---------------------------------------

    #[test]
    fn test_tui_new_saves_current_state_before_reset() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut mode = TuiMode::new();
        mode.push_history_line("stale transcript".to_string());
        let original_id = mode.current_task_id();
        let mut ctx = setup_ctx();

        mode.on_user_input("/new".to_string(), &mut ctx);

        let state_file = temp.path().join(format!("{original_id}.json"));
        assert!(state_file.exists(), "/new must save the prior task state");
        assert_eq!(
            mode.history_lines().len(),
            1,
            "/new must reset the transcript"
        );
        assert!(
            mode.history_lines()[0].starts_with("[new session: task-"),
            "expected new-session marker, got: {:?}",
            mode.history_lines()
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_new_creates_fresh_task_id() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut mode = TuiMode::new();
        let original_id = mode.current_task_id();
        let mut ctx = setup_ctx();
        mode.on_user_input("/new".to_string(), &mut ctx);

        assert_ne!(
            mode.current_task_id(),
            original_id,
            "/new must assign a new task-id"
        );
        assert!(
            !mode.is_turn_in_progress(),
            "/new must not leave a stale turn active"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_new_clears_active_edit_loop() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/new".to_string(), &mut ctx);

        assert!(
            !mode.is_turn_in_progress(),
            "/new must clear active edit-loop state"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_resume_restores_active_grants() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut saved = TaskState::new("task-resume-001".to_string());
        saved.active_grants.insert(
            crate::runtime::Capability::ApplyPatch,
            crate::runtime::ApprovalScope::Session,
        );
        saved.changed_files.push(PathBuf::from("src/app.rs"));
        saved.status = crate::runtime::TaskStatus::Completed;
        saved.save(temp.path()).unwrap();

        let mut mode = TuiMode::new();
        mode.push_history_line("stale transcript".to_string());
        let mut ctx = setup_ctx();
        mode.on_user_input("/resume task-resume-001".to_string(), &mut ctx);

        assert_eq!(mode.current_task_id(), "task-resume-001");
        assert!(mode
            .current_task
            .active_grants
            .contains_key(&crate::runtime::Capability::ApplyPatch));
        assert_eq!(
            mode.current_task.changed_files,
            vec![PathBuf::from("src/app.rs")]
        );
        assert_eq!(
            mode.current_task.status,
            crate::runtime::TaskStatus::Completed
        );
        assert_eq!(
            mode.history_lines().len(),
            1,
            "/resume must reset the transcript"
        );
        assert!(
            mode.history_lines()[0].contains("[resumed: task-resume-001 status=Completed]"),
            "expected resume confirmation in history"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_resume_without_id_offers_recent_task_selection() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let older = TaskState::new("task-resume-older".to_string());
        older.save(temp.path()).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let mut newer = TaskState::new("task-resume-newer".to_string());
        newer.status = crate::runtime::TaskStatus::Running;
        newer.save(temp.path()).unwrap();

        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/resume".to_string(), &mut ctx);

        assert!(
            mode.overlay_active(),
            "/resume without id must open a selection overlay"
        );
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains("task-resume-newer (Running)")),
            "expected recent-task list in history"
        );

        mode.on_user_input("1".to_string(), &mut ctx);

        assert_eq!(mode.current_task_id(), "task-resume-newer");
        assert_eq!(
            mode.history_lines().len(),
            1,
            "resume selection must reset transcript"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_resume_does_not_restore_conversation() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let saved = TaskState::new("task-resume-002".to_string());
        saved.save(temp.path()).unwrap();

        let mut mode = TuiMode::new();
        mode.push_history_line("stale transcript".to_string());
        let mut ctx = setup_ctx();
        mode.on_user_input("/resume task-resume-002".to_string(), &mut ctx);

        assert_eq!(
            mode.history_lines().len(),
            1,
            "/resume must clear prior transcript state"
        );
        assert!(
            !mode.is_turn_in_progress(),
            "/resume must not start a model turn"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_resume_unknown_id_emits_error() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/resume task-does-not-exist".to_string(), &mut ctx);

        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[resume: task 'task-does-not-exist' not found]")),
            "expected not-found message in history"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_resume_restores_legacy_subdir_state() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let old_cwd = std::env::current_dir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        let legacy_state_dir = nested.join(".vex/state");
        std::fs::create_dir_all(&legacy_state_dir).unwrap();

        let mut saved = TaskState::new("task-legacy-ui".to_string());
        saved.status = crate::runtime::TaskStatus::Completed;
        saved.save(&legacy_state_dir).unwrap();

        std::env::remove_var("VEX_STATE_DIR");
        std::env::set_current_dir(&nested).unwrap();

        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/resume task-legacy-ui".to_string(), &mut ctx);

        std::env::set_current_dir(old_cwd).unwrap();

        assert_eq!(mode.current_task_id(), "task-legacy-ui");
        assert!(
            mode.history_lines()[0].contains("[resumed: task-legacy-ui status=Completed]"),
            "expected resume confirmation in history"
        );
    }

    #[test]
    fn test_tui_clear_resets_conversation_history() {
        let mut mode = TuiMode::new();
        mode.push_history_line("stale transcript".to_string());
        let mut ctx = setup_ctx();

        mode.on_user_input("/clear".to_string(), &mut ctx);

        assert_eq!(
            mode.history_lines().len(),
            1,
            "/clear must reset the transcript"
        );
        assert!(
            mode.history_lines()[0].starts_with("[cleared: conversation history reset; task "),
            "expected cleared confirmation"
        );
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_tui_clear_preserves_task_id_and_grants() {
        let mut mode = TuiMode::new();
        let original_id = mode.current_task_id();
        mode.current_task.active_grants.insert(
            crate::runtime::Capability::RunCommand,
            crate::runtime::ApprovalScope::Session,
        );
        let mut ctx = setup_ctx();

        mode.on_user_input("/clear".to_string(), &mut ctx);

        assert_eq!(
            mode.current_task_id(),
            original_id,
            "/clear must not change task-id"
        );
        assert!(
            mode.current_task
                .active_grants
                .contains_key(&crate::runtime::Capability::RunCommand),
            "/clear must preserve active grants"
        );
        assert!(
            !mode.is_turn_in_progress(),
            "/clear must clear active edit-loop state"
        );
    }

    #[test]
    fn test_tui_clear_clears_active_edit_loop() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/clear".to_string(), &mut ctx);

        assert!(
            !mode.is_turn_in_progress(),
            "/clear must clear active edit-loop state"
        );
    }

    #[test]
    fn test_tui_fork_saves_parent_before_branching() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut mode = TuiMode::new();
        let parent_id = mode.current_task_id();
        let mut ctx = setup_ctx();
        mode.on_user_input("/fork".to_string(), &mut ctx);

        let parent_file = temp.path().join(format!("{parent_id}.json"));
        assert!(parent_file.exists(), "/fork must save parent state file");
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_fork_creates_new_task_id() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut mode = TuiMode::new();
        let parent_id = mode.current_task_id();
        mode.current_task.active_grants.insert(
            crate::runtime::Capability::RunCommand,
            crate::runtime::ApprovalScope::Session,
        );
        mode.current_task
            .changed_files
            .push(PathBuf::from("src/app.rs"));
        mode.current_task.status = crate::runtime::TaskStatus::Running;
        mode.push_history_line("stale transcript".to_string());
        let mut ctx = setup_ctx();

        mode.on_user_input("/fork feature work".to_string(), &mut ctx);

        assert_ne!(
            mode.current_task_id(),
            parent_id,
            "/fork must assign a new task-id"
        );
        assert!(mode.current_task_id().ends_with("-feature-work"));
        assert!(mode
            .current_task
            .active_grants
            .contains_key(&crate::runtime::Capability::RunCommand));
        assert_eq!(
            mode.current_task.changed_files,
            vec![PathBuf::from("src/app.rs")]
        );
        assert_eq!(
            mode.current_task.status,
            crate::runtime::TaskStatus::Running
        );
        assert_eq!(mode.history_lines().len(), 1, "/fork must reset transcript");
        assert!(
            mode.history_lines()[0].contains(&format!("branched from {parent_id}")),
            "expected fork confirmation in history"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_fork_does_not_copy_conversation() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut mode = TuiMode::new();
        mode.push_history_line("stale transcript".to_string());
        let mut ctx = setup_ctx();
        mode.on_user_input("/fork".to_string(), &mut ctx);

        assert_eq!(
            mode.history_lines().len(),
            1,
            "/fork must clear prior transcript state"
        );
        assert!(
            !mode.is_turn_in_progress(),
            "/fork must not start a model turn"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_fork_aborts_on_save_failure() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        let blocking_path = temp.path().join("state-file");
        std::fs::write(&blocking_path, "occupied").unwrap();
        std::env::set_var("VEX_STATE_DIR", blocking_path.as_os_str());

        let mut mode = TuiMode::new();
        let original_id = mode.current_task_id();
        let mut ctx = setup_ctx();
        mode.on_user_input("/fork".to_string(), &mut ctx);

        assert_eq!(
            mode.current_task_id(),
            original_id,
            "/fork must not change task-id when parent save fails"
        );
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[fork] save failed")),
            "expected save failure message"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    // -- PK-01: /quit and /exit ------------------------------------------------

    #[test]
    fn test_tui_quit_command_requests_quit() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_user_input("/quit".to_string(), &mut ctx);
        assert!(
            mode.quit_requested(),
            "/quit must set quit_requested immediately"
        );
        assert!(
            !mode.history_state.turn_in_progress,
            "/quit must not start a model turn"
        );
    }

    #[test]
    fn test_tui_exit_is_alias_for_quit() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_user_input("/exit".to_string(), &mut ctx);
        assert!(
            mode.quit_requested(),
            "/exit must behave identically to /quit"
        );
        assert!(
            !mode.history_state.turn_in_progress,
            "/exit must not start a model turn"
        );
    }

    // -- PK-02: /about ---------------------------------------------------------

    #[test]
    fn test_tui_about_renders_without_model_turn() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_user_input("/about".to_string(), &mut ctx);
        assert!(
            !mode.history_state.turn_in_progress,
            "/about must not start a model turn"
        );
        let has_version = mode
            .history_state
            .lines
            .iter()
            .any(|l| l.starts_with("vex "));
        assert!(has_version, "/about must render version line");
        let has_build = mode.history_state.lines.iter().any(|l| l.contains("build"));
        assert!(has_build, "/about must render build metadata");
        let has_commit = mode
            .history_state
            .lines
            .iter()
            .any(|l| l.contains("commit"));
        assert!(has_commit, "/about must render commit metadata");
    }

    // -- PI-01 / PI-02 / PI-03 -------------------------------------------------

    #[test]
    fn test_permissions_empty_grants() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/permissions".to_string(), &mut ctx);
        assert!(
            mode.history_lines().iter().any(|l| l == "[permissions]"),
            "expected permissions header"
        );
        for &cap in ALL_CAPABILITIES {
            let cap_name = capability_to_kebab(cap);
            assert!(
                mode.history_lines()
                    .iter()
                    .any(|l| l.contains(cap_name) && l.contains("(none)")),
                "expected {cap_name} with (none) in empty-grants permissions output"
            );
        }
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_permissions_lists_active_grants() {
        let mut mode = TuiMode::new();
        mode.current_task
            .active_grants
            .insert(Capability::RunCommand, ApprovalScope::Session);
        mode.current_task
            .active_grants
            .insert(Capability::Network, ApprovalScope::Once);
        let mut ctx = setup_ctx();
        mode.on_user_input("/permissions".to_string(), &mut ctx);
        let lines = mode.history_lines().to_vec();
        let has_header = lines.iter().any(|l| l == "[permissions]");
        let has_run_command = lines
            .iter()
            .any(|l| l.contains("run-command") && l.contains("session"));
        let has_network = lines
            .iter()
            .any(|l| l.contains("network") && l.contains("once"));
        let has_apply_patch_none = lines
            .iter()
            .any(|l| l.contains("apply-patch") && l.contains("(none)"));
        assert!(has_header, "expected active grants header");
        assert!(has_run_command, "expected run-command session entry");
        assert!(has_network, "expected network once entry");
        assert!(
            has_apply_patch_none,
            "expected apply-patch (none) for absent grant"
        );
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_allow_inserts_grant() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/allow run-command session".to_string(), &mut ctx);
        assert_eq!(
            mode.current_task.active_grants.get(&Capability::RunCommand),
            Some(&ApprovalScope::Session),
            "allow must insert the grant with session scope"
        );
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[allow: run-command granted for session]")),
            "expected grant confirmation"
        );
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_allow_defaults_to_once_scope() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/allow write-file".to_string(), &mut ctx);
        assert_eq!(
            mode.current_task.active_grants.get(&Capability::WriteFile),
            Some(&ApprovalScope::Once),
            "allow without scope must default to once"
        );
    }

    #[test]
    fn test_allow_unknown_capability_emits_error() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/allow bogus-cap".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[allow: unknown capability 'bogus-cap']")),
            "expected unknown-capability error"
        );
        assert!(mode.current_task.active_grants.is_empty());
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_allow_task_scope_emits_error() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/allow network task".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[allow: unknown scope 'task'; valid: once | session]")),
            "expected task scope rejection"
        );
        assert!(mode.current_task.active_grants.is_empty());
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_allow_unknown_scope_emits_error() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/allow network forever".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[allow: unknown scope 'forever'; valid: once | session]")),
            "expected unknown-scope error"
        );
        assert!(mode.current_task.active_grants.is_empty());
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_deny_removes_grant() {
        let mut mode = TuiMode::new();
        mode.current_task
            .active_grants
            .insert(Capability::ApplyPatch, ApprovalScope::Task);
        let mut ctx = setup_ctx();
        mode.on_user_input("/deny apply-patch".to_string(), &mut ctx);
        assert!(
            !mode
                .current_task
                .active_grants
                .contains_key(&Capability::ApplyPatch),
            "deny must remove the grant"
        );
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[deny: apply-patch removed]")),
            "expected revoke confirmation"
        );
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_deny_no_grant_emits_info() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/deny browser".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[deny: browser not in active grants]")),
            "expected no-active-grant info message"
        );
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_deny_unknown_capability_emits_error() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/deny not-a-thing".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[deny: unknown capability 'not-a-thing']")),
            "expected unknown-capability error"
        );
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_capability_kebab_round_trip() {
        for &cap in ALL_CAPABILITIES {
            let kebab = capability_to_kebab(cap);
            let round_tripped = kebab_to_capability(kebab);
            assert_eq!(
                round_tripped,
                Some(cap),
                "capability {kebab} failed round-trip through kebab_to_capability"
            );
        }
    }

    #[test]
    fn test_capability_for_tool_name_maps_builtin_tools() {
        assert_eq!(
            capability_for_tool_name("read_file"),
            Some(Capability::ReadFile)
        );
        assert_eq!(
            capability_for_tool_name("write_file"),
            Some(Capability::WriteFile)
        );
        assert_eq!(
            capability_for_tool_name("apply_patch"),
            Some(Capability::ApplyPatch)
        );
        assert_eq!(
            capability_for_tool_name("run_command"),
            Some(Capability::RunCommand)
        );
        assert_eq!(
            capability_for_tool_name("git_commit"),
            Some(Capability::ApplyPatch)
        );
        assert_eq!(capability_for_tool_name("unknown_tool"), None);
    }

    #[tokio::test]
    async fn test_tool_approval_auto_approves_matching_session_grant() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        mode.current_task
            .active_grants
            .insert(Capability::RunCommand, ApprovalScope::Session);
        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "run_command".to_string(),
                input_preview: "{\"tool\":\"write_file\"}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        assert!(response_rx.await.expect("response should resolve"));
        assert_eq!(
            mode.current_task.active_grants.get(&Capability::RunCommand),
            Some(&ApprovalScope::Session),
            "session grant must remain after auto-approval"
        );
        assert!(
            mode.overlay_state.pending_approval.is_none(),
            "matching grant must not open the approval overlay"
        );
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains("[auto-approved tool: run_command session grant]")),
            "expected auto-approval transcript entry"
        );
    }

    #[tokio::test]
    async fn test_tool_approval_consumes_matching_once_grant() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        mode.current_task
            .active_grants
            .insert(Capability::ApplyPatch, ApprovalScope::Once);
        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "apply_patch".to_string(),
                input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        assert!(response_rx.await.expect("response should resolve"));
        assert!(
            !mode
                .current_task
                .active_grants
                .contains_key(&Capability::ApplyPatch),
            "once grant must be consumed after auto-approval"
        );
        assert!(
            mode.overlay_state.pending_approval.is_none(),
            "matching once grant must not open the approval overlay"
        );
    }

    #[tokio::test]
    async fn test_tool_approval_prompts_when_grant_does_not_match_tool() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        mode.current_task
            .active_grants
            .insert(Capability::ApplyPatch, ApprovalScope::Session);
        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "run_command".to_string(),
                input_preview: "{\"tool\":\"write_file\"}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        let mut response_rx = Box::pin(response_rx);
        assert!(
            response_rx.as_mut().now_or_never().is_none(),
            "non-matching grant must leave approval unresolved"
        );
        assert!(
            mode.overlay_state.pending_approval.is_some(),
            "non-matching grant must still open the approval overlay"
        );
        assert_eq!(
            mode.current_task.active_grants.get(&Capability::ApplyPatch),
            Some(&ApprovalScope::Session),
            "non-matching grant must remain intact"
        );
    }

    // -- PM-01 (app side): build_runtime_with_resume ---------------------------

    #[test]
    fn test_build_runtime_with_resume_restores_task() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = TaskState::new("task-startup-resume".to_string());
        state
            .active_grants
            .insert(Capability::Network, ApprovalScope::Session);
        state.status = crate::runtime::TaskStatus::Running;

        let config = Config {
            model_token: None,
            model_name: "mock-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            working_dir: temp.path().to_path_buf(),
            model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
            model_protocol: crate::runtime::ModelProtocol::MessagesV1,
            tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            model_headers: reqwest::header::HeaderMap::new(),
            notes_path: None,
            hooks: Vec::new(),
        };

        let (runtime, _ctx) = build_runtime_with_resume(config, state)
            .expect("build_runtime_with_resume should succeed");

        assert_eq!(runtime.mode.current_task.id, "task-startup-resume");
        assert_eq!(
            runtime
                .mode
                .current_task
                .active_grants
                .get(&Capability::Network),
            Some(&ApprovalScope::Session)
        );
        assert!(
            runtime
                .mode
                .history_lines()
                .iter()
                .any(|l| l.contains("[resumed: task-startup-resume status=Running]")),
            "expected resume banner in history"
        );
    }

    // -- PC-01: /model --------------------------------------------------------

    #[tokio::test]
    async fn test_model_shows_current_name() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        mode.on_user_input("/model".to_string(), &mut ctx);
        assert!(
            mode.history_lines().iter().any(|l| l.contains("[model]")),
            "bare /model must echo current model"
        );
    }

    #[tokio::test]
    async fn test_model_switches_name() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let old = mode.model_name.clone();
        mode.on_user_input("/model local/qwen3-8b".to_string(), &mut ctx);
        assert_eq!(mode.model_name, "local/qwen3-8b");
        assert_eq!(ctx.test_model_name().await, "local/qwen3-8b");
        assert!(mode
            .history_lines()
            .iter()
            .any(|l| l.contains(&old) && l.contains("local/qwen3-8b")));
    }

    #[tokio::test]
    async fn test_model_rejects_local_on_api_backend() {
        let mut ctx = setup_ctx();
        let mut config = Config::default_for_tui();
        config.model_backend = crate::runtime::ModelBackendKind::ApiServer;
        config.model_name = "remote-model".to_string();
        let mut mode = TuiMode::new_with_config(None, config);
        // local/ prefix on an ApiServer session must be rejected.
        mode.on_user_input("/model local/phi-3".to_string(), &mut ctx);
        assert_ne!(
            mode.model_name, "local/phi-3",
            "must reject local/ model on api-server backend"
        );
        assert!(mode.history_lines().iter().any(|l| l.contains("rejected")));
        assert_eq!(ctx.test_model_name().await, "mock-model");
    }

    #[tokio::test]
    async fn test_model_rejects_remote_on_local_backend() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let original = mode.model_name.clone();
        mode.on_user_input("/model remote-model".to_string(), &mut ctx);
        assert_eq!(mode.model_name, original);
        assert_eq!(ctx.test_model_name().await, "mock-model");
        assert!(mode.history_lines().iter().any(|l| l.contains("rejected")));
    }

    #[tokio::test]
    async fn test_model_does_not_start_turn() {
        let mut ctx = setup_ctx();
        let mut mode = TuiMode::new();
        let initial_messages = ctx.test_message_count().await;

        mode.on_user_input("/model".to_string(), &mut ctx);
        assert!(!mode.is_turn_in_progress(), "/model must not start a turn");

        mode.on_user_input("/model local/phi-3".to_string(), &mut ctx);
        assert!(
            !mode.is_turn_in_progress(),
            "/model <n> must not start a turn"
        );
        assert_eq!(ctx.test_message_count().await, initial_messages);
    }

    // -- PK-07: /diff ---------------------------------------------------------

    #[tokio::test]
    async fn test_tui_diff_renders_working_tree_diff() {
        let mut ctx = setup_ctx();
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        std::fs::write(temp.path().join("a.txt"), "hello\n").unwrap();
        git_success(temp.path(), &["add", "a.txt"]);
        git_success(temp.path(), &["commit", "-m", "init"]);
        std::fs::write(temp.path().join("a.txt"), "world\n").unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        mode.on_user_input("/diff".to_string(), &mut ctx);

        let has_diff = mode
            .history_lines()
            .iter()
            .any(|l| l.contains("diff --git") || l.contains("a.txt"));
        assert!(has_diff, "expected git diff output in history");
    }

    #[tokio::test]
    async fn test_tui_diff_staged_flag() {
        let mut ctx = setup_ctx();
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        std::fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
        git_success(temp.path(), &["add", "tracked.txt"]);
        git_success(temp.path(), &["commit", "-m", "init"]);

        std::fs::write(temp.path().join("tracked.txt"), "staged\n").unwrap();
        git_success(temp.path(), &["add", "tracked.txt"]);
        std::fs::write(temp.path().join("tracked.txt"), "unstaged\n").unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        mode.on_user_input("/diff --staged".to_string(), &mut ctx);

        let history = mode.history_lines().join("\n");
        assert!(history.contains("tracked.txt"));
        assert!(history.contains("+staged"));
        assert!(!history.contains("+unstaged"));
    }

    #[tokio::test]
    async fn test_tui_diff_non_git_repo() {
        let mut ctx = setup_ctx();
        let temp = tempfile::tempdir().unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        mode.on_user_input("/diff".to_string(), &mut ctx);

        assert!(mode
            .history_lines()
            .iter()
            .any(|l| l == "[diff] not a git repository"));
    }

    #[tokio::test]
    async fn test_tui_diff_clean_working_tree() {
        let mut ctx = setup_ctx();
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        std::fs::write(temp.path().join("clean.txt"), "clean\n").unwrap();
        git_success(temp.path(), &["add", "clean.txt"]);
        git_success(temp.path(), &["commit", "-m", "init"]);

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        mode.on_user_input("/diff".to_string(), &mut ctx);

        assert!(mode
            .history_lines()
            .iter()
            .any(|l| l == "[diff] working tree is clean"));
    }

    #[tokio::test]
    async fn test_tui_diff_truncates_at_max_lines() {
        let mut ctx = setup_ctx();
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        let path = temp.path().join("large.txt");
        std::fs::write(&path, "seed\n").unwrap();
        git_success(temp.path(), &["add", "large.txt"]);
        git_success(temp.path(), &["commit", "-m", "init"]);

        let large_body = (0..260)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, large_body).unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        mode.on_user_input("/diff".to_string(), &mut ctx);

        assert!(mode
            .history_lines()
            .iter()
            .any(|line| line == "[diff truncated \u{2014} showing first 200 lines]"));
    }

    #[tokio::test]
    async fn test_tui_diff_does_not_start_model_turn() {
        let mut ctx = setup_ctx();
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        std::fs::write(temp.path().join("tracked.txt"), "clean\n").unwrap();
        git_success(temp.path(), &["add", "tracked.txt"]);
        git_success(temp.path(), &["commit", "-m", "init"]);

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let initial_messages = ctx.test_message_count().await;
        mode.on_user_input("/diff".to_string(), &mut ctx);

        assert!(
            !mode.is_turn_in_progress(),
            "/diff must not start a model turn"
        );
        assert_eq!(ctx.test_message_count().await, initial_messages);
    }

    fn init_git_repo(path: &std::path::Path) {
        git_success(path, &["init"]);
        git_success(path, &["config", "user.name", "test"]);
        git_success(path, &["config", "user.email", "t@t"]);
    }

    fn git_success(path: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: stdout={} stderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_tui_edit_command_starts_edit_loop() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/edit fix the parser bug".to_string(), &mut ctx);
        assert!(
            mode.active_edit_loop.is_some(),
            "/edit must set active_edit_loop"
        );
        assert!(
            mode.is_turn_in_progress(),
            "/edit must mark turn_in_progress"
        );
    }

    #[test]
    fn test_tui_edit_command_preserves_prior_history_line() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.history_state
            .lines
            .push("prior assistant line".to_string());

        mode.on_user_input("/edit fix the parser bug".to_string(), &mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("new output".to_string()), &mut ctx);

        assert_eq!(mode.history_state.lines[0], "prior assistant line");
        assert!(
            mode.history_state
                .lines
                .iter()
                .any(|line| line.contains("new output")),
            "stream output must target the fresh placeholder line"
        );
    }

    #[test]
    fn test_tui_fix_without_prior_loop_emits_guidance() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/fix".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[no recent validation failure in this session")),
            "expected guidance message when no prior loop exists"
        );
        assert!(!mode.is_turn_in_progress());
    }

    #[test]
    fn test_tui_fix_during_active_edit_emits_reentrancy_guard() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.active_edit_loop = Some(EditLoop::new("task-existing".to_string()));
        mode.history_state.turn_in_progress = true;
        mode.on_user_input("/fix".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[edit loop already active")),
            "expected reentrancy guard message"
        );
    }

    #[test]
    fn test_tui_second_edit_command_blocked_while_loop_active() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.active_edit_loop = Some(EditLoop::new("task-existing".to_string()));
        mode.history_state.turn_in_progress = true;
        mode.on_user_input("/edit add more tests".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[edit loop already active")),
            "second /edit while loop active must emit reentrancy guard"
        );
    }

    #[test]
    fn test_slash_command_returns_none_for_non_slash_input() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("hello world".to_string(), &mut ctx);
        assert!(
            mode.is_turn_in_progress(),
            "non-slash input must dispatch a model turn"
        );
    }

    #[test]
    fn test_slash_command_does_not_call_start_turn_directly() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/edit refactor the parser".to_string(), &mut ctx);
        assert_eq!(
            mode.last_turn_input.as_deref(),
            Some("refactor the parser"),
            "/edit must pass bare instruction (not the full slash command) to start_turn"
        );
    }

    #[test]
    fn test_tui_edit_empty_instruction_emits_usage() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/edit".to_string(), &mut ctx);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains("[edit] usage: /edit <instruction>")),
            "expected usage hint when /edit called without instruction"
        );
        assert!(!mode.is_turn_in_progress());
        assert!(mode.active_edit_loop.is_none());
    }

    #[test]
    fn test_tui_edit_loop_completion_clears_busy_state() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.on_user_input("/edit refactor the parser".to_string(), &mut ctx);

        mode.on_model_update(
            UiUpdate::EditLoopComplete {
                outcome: EditLoopOutcome::MaxTurnsReached { last_error: None },
                last_validation_result: None,
            },
            &mut ctx,
        );

        assert!(!mode.is_turn_in_progress());
        assert!(mode.history_state.active_assistant_index.is_none());
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains("[edit loop reached max turns]")),
            "expected loop completion summary"
        );
    }

    #[test]
    fn test_tui_new_clears_active_edit_loop_field() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

        let mut mode = TuiMode::new();
        mode.active_edit_loop = Some(EditLoop::new("task-before-new".to_string()));
        let mut ctx = setup_ctx();
        mode.on_user_input("/new".to_string(), &mut ctx);

        assert!(
            mode.active_edit_loop.is_none(),
            "/new must clear active_edit_loop field"
        );
        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_tui_clear_clears_active_edit_loop_field() {
        let mut mode = TuiMode::new();
        mode.active_edit_loop = Some(EditLoop::new("task-before-clear".to_string()));
        let mut ctx = setup_ctx();
        mode.on_user_input("/clear".to_string(), &mut ctx);

        assert!(
            mode.active_edit_loop.is_none(),
            "/clear must clear active_edit_loop field"
        );
    }

    #[tokio::test]
    async fn test_tui_explain_does_not_invoke_edit_loop() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx_with_responses(vec![vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Explained\"},\"finish_reason\":\"stop\"}]}".to_string(),
        ]]);

        mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);

        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                if ctx.test_message_count().await > 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("/explain must start a single model turn");

        assert!(
            mode.active_edit_loop.is_none(),
            "/explain must not invoke EditLoop"
        );
        assert!(
            mode.last_turn_input.as_deref().is_some_and(|prompt| {
                prompt.contains("Explain the relevant code for the request below.")
                    && prompt.contains("Request:\nexplain src/app.rs")
            }),
            "/explain must render the explain template prompt"
        );
    }

    #[tokio::test]
    async fn test_tui_explain_silently_denies_tool_approval_requests() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);

        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "apply_patch".to_string(),
                input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        assert!(
            !response_rx.await.expect("response should resolve"),
            "/explain must silently deny approval-requiring tool calls"
        );
        assert!(
            mode.overlay_state.pending_approval.is_none(),
            "/explain must not surface the approval overlay"
        );
        assert!(
            mode.history_lines()
                .iter()
                .all(|line| !line.contains("[tool approval requested:")),
            "/explain denial should stay silent in transcript output"
        );
    }

    #[tokio::test]
    async fn test_read_only_turn_flag_clears_after_turn_completion() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);
        assert!(
            mode.read_only_turn_active,
            "/explain must mark the active turn as read-only"
        );

        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
        assert!(
            !mode.read_only_turn_active,
            "turn completion must clear the read-only turn flag"
        );

        let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name: "apply_patch".to_string(),
                input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        let mut response_rx = Box::pin(response_rx);
        assert!(
            response_rx.as_mut().now_or_never().is_none(),
            "normal turns must keep approval unresolved until operator input"
        );
        assert!(
            mode.overlay_state.pending_approval.is_some(),
            "normal turns must restore the approval overlay"
        );
    }

    #[test]
    fn test_tui_run_command_invokes_validation_suite_only() {
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_user_input(successful_run_input(), &mut ctx);

        assert!(
            !mode.is_turn_in_progress(),
            "/run must not start a model turn"
        );
        assert!(
            mode.active_edit_loop.is_none(),
            "/run must not seed or invoke EditLoop"
        );
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains("[run]")),
            "expected /run transcript output"
        );
    }

    #[test]
    fn test_at_path_injects_file_contents() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("note.txt"), "hello from file\n").unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let mut ctx = setup_ctx();

        mode.on_user_input("summarize @note.txt".to_string(), &mut ctx);

        let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
        assert!(turn_input.contains("[file: note.txt]"));
        assert!(turn_input.contains("hello from file"));
        assert!(mode.is_turn_in_progress());
    }

    #[test]
    fn test_at_path_directory_renders_listing() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "pub fn hi() {}\n").unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let mut ctx = setup_ctx();

        mode.on_user_input("review @src".to_string(), &mut ctx);

        let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
        assert!(turn_input.contains("[dir: src]"));
        assert!(turn_input.contains("src/lib.rs"));
    }

    #[test]
    fn test_at_path_missing_file_is_annotated() {
        let temp = tempfile::tempdir().unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let mut ctx = setup_ctx();

        mode.on_user_input("inspect @missing.txt".to_string(), &mut ctx);

        let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
        assert!(turn_input.contains("[file: missing.txt \u{2014} not found]"));
    }

    #[test]
    fn test_at_path_outside_workspace_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_path = outside.path().join("secret.txt");
        std::fs::write(&outside_path, "secret").unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let mut ctx = setup_ctx();

        mode.on_user_input(
            format!("inspect @{}", outside_path.to_string_lossy()),
            &mut ctx,
        );

        let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
        assert!(turn_input.contains("[file: "));
        assert!(
            turn_input.contains("outside workspace root")
                || turn_input.contains("escapes workspace root")
                || turn_input.contains("absolute or platform-specific path not allowed"),
            "expected outside-workspace annotation, got: {turn_input}"
        );
    }

    #[test]
    fn test_at_path_multiple_tokens_resolved_in_order() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("one.txt"), "first").unwrap();
        std::fs::write(temp.path().join("two.txt"), "second").unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let mut ctx = setup_ctx();

        mode.on_user_input("compare @one.txt with @two.txt".to_string(), &mut ctx);

        let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
        let first_idx = turn_input.find("[file: one.txt]").unwrap();
        let second_idx = turn_input.find("[file: two.txt]").unwrap();
        assert!(first_idx < second_idx);
        assert!(turn_input.contains("first"));
        assert!(turn_input.contains("second"));
    }

    #[test]
    fn test_at_path_not_expanded_inside_slash_command_args() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("note.txt"), "hello from file\n").unwrap();

        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let mut ctx = setup_ctx();

        mode.on_user_input("/explain @note.txt".to_string(), &mut ctx);

        let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
        assert!(!turn_input.contains("[file: note.txt]"));
        assert!(!turn_input.contains("hello from file"));
    }

    #[test]
    fn test_bang_prefix_requires_run_command_approval() {
        let temp = tempfile::tempdir().unwrap();
        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let mut ctx = setup_ctx();

        mode.on_user_input(successful_bang_input(), &mut ctx);

        assert!(mode.overlay_state.pending_approval.is_some());
        assert!(!mode.is_turn_in_progress());
        assert!(mode
            .history_lines()
            .iter()
            .any(|line| { line.contains("[tool approval requested:") }));
    }

    #[tokio::test]
    async fn test_bang_prefix_runs_without_model_turn_after_approval() {
        let temp = tempfile::tempdir().unwrap();
        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let (mut ctx, mut rx) = setup_ctx_with_updates();
        let initial_messages = ctx.test_message_count().await;

        mode.on_user_input(successful_bang_input(), &mut ctx);
        assert!(mode.overlay_state.pending_approval.is_some());
        assert!(!mode.is_turn_in_progress());

        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(mode.is_turn_in_progress());

        drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

        assert!(mode.overlay_state.pending_approval.is_none());
        assert!(!mode.is_turn_in_progress());
        assert_eq!(ctx.test_message_count().await, initial_messages);
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains("stdout: inline-shell")),
            "expected inline shell stdout in transcript"
        );
        assert!(
            mode.history_lines().iter().any(|line| line == "[exit: 0]"),
            "expected inline shell exit status"
        );
    }

    #[tokio::test]
    async fn test_bang_prefix_routes_through_sandbox() {
        let temp = tempfile::tempdir().unwrap();
        let wrapped = Arc::new(AtomicBool::new(false));
        let result = run_shell_command_with_runner(
            DefaultCommandRunner::new(),
            RecordingSandbox {
                wrapped: Arc::clone(&wrapped),
            },
            "echo sandbox-hit".to_string(),
            temp.path().to_path_buf(),
        )
        .await
        .unwrap();

        assert!(wrapped.load(Ordering::SeqCst));
        assert!(result.stdout.contains("sandbox-hit"));
    }

    #[tokio::test]
    async fn test_bang_prefix_cancellation_completes_turn() {
        let temp = tempfile::tempdir().unwrap();
        let mut mode = TuiMode::new();
        mode.working_dir = temp.path().to_path_buf();
        let (mut ctx, mut rx) = setup_ctx_with_updates();
        let input = if cfg!(windows) {
            "!ping -n 6 127.0.0.1 > nul".to_string()
        } else {
            "!sleep 5".to_string()
        };

        mode.on_user_input(input, &mut ctx);
        mode.on_user_input("1".to_string(), &mut ctx);
        assert!(mode.is_turn_in_progress());

        mode.on_interrupt(&mut ctx);
        drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

        assert!(!mode.is_turn_in_progress());
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line == "[shell] cancelled"),
            "expected cancellation feedback for inline shell commands"
        );
    }

    #[tokio::test]
    async fn test_tui_context_renders_without_model_turn() {
        let _env_lock = crate::test_support::ENV_LOCK.lock().await;
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        let initial_messages = ctx.test_message_count().await;

        mode.on_user_input("/context".to_string(), &mut ctx);

        assert!(
            !mode.is_turn_in_progress(),
            "/context must not start a model turn"
        );
        assert_eq!(
            ctx.test_message_count().await,
            initial_messages,
            "/context must not call ctx.start_turn"
        );
        assert!(
            mode.history_lines().iter().any(|line| line == "[context]"),
            "expected context header"
        );
    }

    #[test]
    fn test_tui_context_shows_tilde_token_estimate() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_user_input("/context".to_string(), &mut ctx);

        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.trim_start().starts_with("tokens") && line.contains('~')),
            "token estimate line must include '~'"
        );
    }

    #[test]
    fn test_tui_context_shows_active_grants_count() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        mode.current_task
            .active_grants
            .insert(Capability::RunCommand, ApprovalScope::Session);

        mode.on_user_input("/context".to_string(), &mut ctx);

        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains("1 active grant(s)")),
            "expected active grants count in /context output"
        );
    }

    #[test]
    fn test_tui_context_shows_active_profile_name() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        let mut profile =
            ModelProfile::default_for_backend(crate::runtime::ModelBackendKind::ApiServer);
        profile.name = "qwen-coder".to_string();
        mode.active_edit_loop =
            Some(EditLoop::new("task-profile".to_string()).with_profile(profile));

        mode.on_user_input("/context".to_string(), &mut ctx);

        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains("profile") && line.contains("qwen-coder")),
            "expected active profile name in /context output"
        );
    }

    #[test]
    fn test_tui_commands_renders_all_registered_commands() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();

        mode.on_user_input("/commands".to_string(), &mut ctx);

        assert!(
            mode.history_lines().iter().any(|line| line == "[commands]"),
            "expected commands header"
        );
        for spec in SLASH_COMMANDS {
            assert!(
                mode.history_lines()
                    .iter()
                    .any(|line| line.contains(spec.display) && line.contains(spec.description)),
                "expected '{}' to appear in /commands output",
                spec.display
            );
        }
    }

    #[test]
    fn test_tui_help_is_alias_for_commands() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let mut commands_mode = TuiMode::new();
        let mut help_mode = TuiMode::new();
        let mut ctx = setup_ctx();

        commands_mode.on_user_input("/commands".to_string(), &mut ctx);
        help_mode.on_user_input("/help".to_string(), &mut ctx);

        assert_eq!(
            &commands_mode.history_lines()[2..],
            &help_mode.history_lines()[2..],
            "/help must render the same command directory as /commands"
        );
    }

    #[tokio::test]
    async fn test_commands_output_does_not_call_start_turn() {
        let _env_lock = crate::test_support::ENV_LOCK.lock().await;
        let mut mode = TuiMode::new();
        let mut ctx = setup_ctx();
        let initial_messages = ctx.test_message_count().await;

        mode.on_user_input("/commands".to_string(), &mut ctx);

        assert!(
            !mode.is_turn_in_progress(),
            "/commands must not start a model turn"
        );
        assert_eq!(
            ctx.test_message_count().await,
            initial_messages,
            "/commands must not call ctx.start_turn"
        );
    }

    #[test]
    fn test_missing_command_description_is_compile_error() {
        assert!(
            !SLASH_COMMANDS.is_empty(),
            "slash command registry must not be empty"
        );
        for spec in SLASH_COMMANDS {
            assert!(
                !spec.description.is_empty(),
                "command '{}' must have a description",
                spec.display
            );
        }
    }
}
