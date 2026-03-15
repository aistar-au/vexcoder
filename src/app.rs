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
mod inline;
mod layout;
mod overlay;
mod scroll;
mod shell;
#[cfg(test)]
mod tests;
mod turn;
mod turn_start;
pub(crate) mod util;

use self::overlay::summarize_tool_approval_context;
#[cfg(test)]
use self::overlay::{
    overlay_event_to_user_input, parse_approval_selection, render_pass_order, RenderPass,
};
#[cfg(test)]
use self::scroll::{input_rows_for_buffer, RenderGuard};
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
