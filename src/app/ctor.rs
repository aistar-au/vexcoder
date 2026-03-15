use super::*;

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
}
