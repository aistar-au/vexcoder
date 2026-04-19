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
        let task_doc_condenser = TaskDocumentCondenser::new();
        let initial_meta = crate::runtime::TaskInfo {
            id: new_task_id(),
            status: TaskStatus::Ready,
            parent_task_id: None,
            agent_id: None,
            worktree_path: None,
            branch_name: None,
            instructions_path: None,
            model_name: config.model_name.clone(),
            model_backend: config.model_backend,
            model_url: config.model_url.clone(),
            started_at_ms: None,
            updated_at_ms: crate::runtime::session_task::now_millis(),
            last_heartbeat_ms: None,
            active_grants: std::collections::HashMap::new(),
            next_step_id: 0,
        };
        let task_doc = task_doc_condenser.begin_task(initial_meta);
        Self {
            overlay_state: OverlayState::default(),
            repo_label: resolve_repo_label(),
            git_branch: util::resolve_git_branch(),
            instructions_path: None,
            mcp_rollup: None,
            display_column_width: Cell::new(DISPLAY_COLUMN_WIDTH_FALLBACK),
            pending_quit: false,
            quit_requested: false,
            notes_path,
            model_name: config.model_name.clone(),
            model_backend: config.model_backend,
            model_profile: config.model_profile.clone(),
            working_dir: config.working_dir.clone(),
            model_url: config.model_url.clone(),
            search_config: config.search.clone(),
            sandbox: ConfiguredSandbox::default(),
            file_prompt_entries: RefCell::new(None),
            custom_commands,
            last_assembled_context: None,
            task_doc,
            task_doc_condenser,
            pre_session_notices: Vec::new(),
            stream_uses_structured_final_output: false,
            read_only_turn_active: false,
            active_edit_loop: None,
            selected_timeline_index: 0,
            timeline_follow_mode: true,
            transcript_scroll_offset: 0,
            inspector_scroll_offset: 0,
            turn_started_at: None,
            ttft: None,
            last_turn_ttft: None,
            last_turn_duration: None,
            last_error_message: None,
            turn_completion_pending: false,
            plan_turn_active: false,
            auto_memory_enabled: config.auto_memory.enabled,
            auto_memory_max_notes: config.auto_memory.max_notes_per_turn,
            #[cfg(test)]
            last_turn_input: None,
        }
    }
}
