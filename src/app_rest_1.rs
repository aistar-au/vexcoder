#[cfg(test)]
use self::scroll::{RenderGuard, input_rows_for_buffer};
use self::util::{
    builtin_slash_command_names, capability_for_tool_name, format_inline_block,
    format_inline_reference, kebab_to_scope, list_recent_task_entries, new_task_id,
    parse_generate_tests_args, parse_review_args, resolve_repo_label, run_validation_suite_capture,
    sanitize_task_label, scope_to_label, shell_command_request,
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
