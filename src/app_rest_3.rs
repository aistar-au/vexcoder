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
    instruction_manifest: Vec<InstructionSource>,
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
