use super::*;

impl TuiMode {
    pub(super) fn reset_conversation_window(&mut self, ctx: &RuntimeContext) {
        ctx.clear_conversation();
        self.history_state.lines.clear();
        self.history_state.turn_in_progress = false;
        self.history_state.cancel_pending = false;
        self.command_sessions.clear();
        self.history_state.active_assistant_index = None;
        self.history_state.scroll_offset = 0;
        self.history_state.auto_follow = true;
        self.transcript_scroll_offset = 0;
        self.inspector_scroll_offset = 0;
        self.active_stream_blocks.clear();
        self.last_assembled_context = None;
        self.read_only_turn_active = false;
        self.turn_completion_pending = false;
        self.reset_turn_capture();
        self.reset_last_turn_display();
    }

    pub(super) fn apply_resumed_task(&mut self, state: TaskState, ctx: &RuntimeContext) {
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

    pub(super) fn reset_turn_capture(&mut self) {
        self.current_turn_input.clear();
        self.current_turn_response.clear();
        self.current_turn_changed_files.clear();
        self.current_turn_command_history.clear();
        self.current_turn_tool_invocations.clear();
        self.pending_turn_tool_calls.clear();
        self.last_completed_tool_header = None;
        self.duplicate_tool_count = 1;
        self.overlay_state.approved_tool_steps.clear();
        self.selected_timeline_index = 0;
        self.timeline_follow_mode = true;
        self.inspector_scroll_offset = 0;
        self.turn_started_at = None;
        self.ttft = None;
        self.current_turn_prompt_progress = None;
        self.current_turn_timings = None;
        self.turn_completion_pending = false;
        self.plan_turn_active = false;
    }

    /// Append a `[ttft: …s | ↑:…s (N tok) | ↓:…s (N tok) | total: …s]`
    /// timing and token-count summary to the transcript
    /// after a turn finishes so the operator can see latency at a glance.
    fn append_turn_timing_line(&mut self) {
        let total = match self.last_turn_duration {
            Some(d) => d,
            None => return,
        };
        let mut parts = Vec::new();
        if let Some(ttft) = self.last_turn_ttft {
            parts.push(format!("ttft:{:.1}s", ttft.as_secs_f64()));
        }
        if let Some(timings) = self.last_turn_timings.as_ref() {
            if let Some(prompt_ms) = timings.prompt_ms {
                let tok_suffix = timings
                    .prompt_n
                    .map(|n| format!(" ({n} tok)"))
                    .unwrap_or_default();
                parts.push(format!("\u{2191}:{:.1}s{tok_suffix}", prompt_ms / 1000.0));
            }
            if let Some(predicted_ms) = timings.predicted_ms {
                let tok_suffix = timings
                    .predicted_n
                    .map(|n| format!(" ({n} tok)"))
                    .unwrap_or_default();
                parts.push(format!(
                    "\u{2193}:{:.1}s{tok_suffix}",
                    predicted_ms / 1000.0
                ));
            }
        }
        parts.push(format!("total:{:.1}s", total.as_secs_f64()));
        self.push_history_line(format!("[{}]", parts.join(" | ")));
    }

    pub(super) fn reset_last_turn_display(&mut self) {
        self.last_turn_tool_invocations.clear();
        self.last_turn_response.clear();
        self.last_turn_input_display.clear();
        self.last_turn_duration = None;
        self.last_turn_timings = None;
        self.last_error_message = None;
    }

    pub(super) fn persist_current_task_state(&mut self) {
        let dir = TaskState::state_dir();
        if let Err(error) = self.current_task.save(&dir) {
            // Persistence errors are non-fatal.  The in-memory task state
            // remains authoritative and callers already update status before
            // calling this method.  Log to stderr so operators and crash-resume
            // diagnostics can observe lost transitions (ADR-030 Invariant 5).
            eprintln!("[state] save failed: {error}");
        }
    }

    pub(super) fn set_task_status(&mut self, status: TaskStatus) {
        self.current_task.status = status;
        self.persist_current_task_state();
    }

    /// Reload the session-task list from the persisted state file so that
    /// in-memory state stays consistent after a facade call that writes
    /// session tasks to disk without going through `current_task` directly.
    pub(super) fn sync_session_tasks_from_disk(&mut self) {
        let state_dir = TaskState::state_dir_from(&self.working_dir);
        if let Ok(saved) = TaskState::load(&state_dir, &self.current_task.id) {
            self.current_task.session_tasks = saved.session_tasks;
        }
    }

    pub(super) fn begin_turn_capture(&mut self, input: String) {
        self.reset_turn_capture();
        self.current_turn_input = input;
        self.set_task_status(TaskStatus::Running);
        self.transcript_scroll_offset = 0;
        self.turn_started_at = Some(Instant::now());
        self.last_error_message = None;
    }

    pub(super) fn begin_command_session(&mut self, command: String) -> u64 {
        let session_id = self.next_command_session_id;
        self.begin_command_session_with_id(session_id, command);
        session_id
    }

    pub(super) fn begin_command_session_with_id(&mut self, session_id: u64, command: String) {
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
        self.set_task_status(TaskStatus::Running);
    }

    pub(super) fn complete_turn_if_idle(&mut self, ctx: &RuntimeContext) -> bool {
        if !self.turn_completion_pending || !self.command_sessions.is_empty() {
            return false;
        }

        self.last_error_message = None;
        self.resolve_pending_approval(false, ctx);
        self.resolve_pending_patch_approval(false);
        self.active_stream_blocks.clear();
        self.commit_completed_turn(ctx);
        self.append_turn_timing_line();
        self.maybe_extract_auto_memory();
        self.history_state.cancel_pending = false;
        self.history_state.turn_in_progress = false;
        self.history_state.active_assistant_index = None;
        self.read_only_turn_active = false;
        self.turn_completion_pending = false;
        if self.history_state.auto_follow {
            self.set_scroll_to_bottom();
        } else {
            self.clamp_scroll_offset();
        }
        self.transcript_scroll_offset = 0;
        self.inspector_scroll_offset = 0;
        true
    }

    pub(super) fn commit_completed_turn(&mut self, ctx: &RuntimeContext) {
        if self.current_turn_input.trim().is_empty()
            && self.current_turn_response.trim().is_empty()
            && self.current_turn_changed_files.is_empty()
            && self.current_turn_command_history.is_empty()
            && self.current_turn_tool_invocations.is_empty()
        {
            self.current_task.status = TaskStatus::Completed;
            self.persist_current_task_state();
            self.reset_turn_capture();
            return;
        }

        // Persist plan response to TaskState if this was a /plan turn (ADR-029).
        if self.plan_turn_active && !self.current_turn_response.trim().is_empty() {
            self.current_task.plan = Some(self.current_turn_response.clone());
        }

        // Preserve turn data for persistent display after the turn completes.
        self.last_turn_tool_invocations = self.current_turn_tool_invocations.clone();
        self.last_turn_response = self.current_turn_response.clone();
        self.last_turn_input_display = self.current_turn_input.clone();
        self.last_turn_duration = self.turn_started_at.map(|started| started.elapsed());
        self.last_turn_ttft = self.ttft;
        self.last_turn_timings = self.current_turn_timings.clone();
        self.last_error_message = None;

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
        let turn_tokens = ctx.session_tokens_snapshot().last_turn();
        // Accumulate cache usage from the turn into task-level totals (ADR-029).
        self.current_task.cache_usage.total_cache_creation_tokens = self
            .current_task
            .cache_usage
            .total_cache_creation_tokens
            .saturating_add(turn_tokens.cache_creation_input_tokens);
        self.current_task.cache_usage.total_cache_read_tokens = self
            .current_task
            .cache_usage
            .total_cache_read_tokens
            .saturating_add(turn_tokens.cache_read_input_tokens);
        self.current_task.turns.push(TurnEvidenceState {
            input: std::mem::take(&mut self.current_turn_input),
            response: std::mem::take(&mut self.current_turn_response),
            changed_files,
            command_history,
            tool_invocations: std::mem::take(&mut self.current_turn_tool_invocations),
            tokens: turn_tokens,
        });

        self.persist_current_task_state();
        self.transcript_scroll_offset = 0;
        self.inspector_scroll_offset = 0;
        self.reset_turn_capture();
    }

    pub(super) fn summarize_usage_line_suffix(estimated: bool) -> &'static str {
        if estimated {
            " (estimated)"
        } else {
            ""
        }
    }

    fn maybe_extract_auto_memory(&mut self) {
        if !self.auto_memory_enabled {
            return;
        }
        let input = self.last_turn_input_display.clone();
        let response = self.last_turn_response.clone();
        if input.trim().is_empty() && response.trim().is_empty() {
            return;
        }
        let notes = crate::auto_memory::extract_notes_from_turn(
            &input,
            &response,
            self.auto_memory_max_notes,
        );
        if notes.is_empty() {
            return;
        }
        let formatted_notes = crate::auto_memory::format_auto_notes(
            &notes,
            crate::auto_memory::current_timestamp_seconds(),
        );
        let path = self
            .resolved_existing_notes_path()
            .or_else(|| self.resolved_notes_path());
        let Some(path) = path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match crate::auto_memory::append_auto_notes(&formatted_notes, &path) {
            Ok(()) => {
                let turn_index = self.current_task.turns.len();
                for note in &formatted_notes {
                    self.current_task.session_notes.push(SessionNote {
                        content: note.clone(),
                        created_at_turn: turn_index,
                    });
                }
                self.persist_current_task_state();
                self.push_history_line(format!(
                    "[memory] auto: {} note(s) saved",
                    formatted_notes.len()
                ));
            }
            Err(e) => {
                self.push_history_line(format!("[memory] auto extraction error: {e}"));
            }
        }
    }
}
