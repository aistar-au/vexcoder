use super::*;
use crate::runtime::session_task::now_millis;

impl TuiMode {
    pub(super) fn reset_conversation_window(&mut self, ctx: &RuntimeContext) {
        ctx.clear_conversation();
        self.pre_session_notices.clear();
        if let Some(t) = self.task_doc.active_turn.as_mut() {
            t.command_sessions.clear();
        }
        self.transcript_scroll_offset = 0;
        self.inspector_scroll_offset = 0;
        self.last_assembled_context = None;
        self.read_only_turn_active = false;
        self.turn_completion_pending = false;
        self.reset_turn_capture();
    }

    pub(super) fn apply_resumed_task(&mut self, state: TaskState, ctx: &RuntimeContext) {
        let restored_id = state.id.clone();
        let status = format!("{:?}", state.status);
        let task_doc = self.task_doc_condenser.restore_from_snapshot(state);
        self.task_doc = task_doc;
        if let Some(path_str) = self.task_doc.info.instructions_path.clone() {
            self.instructions_path = Some(path_str);
        } else {
            self.task_doc.info.instructions_path = self.instructions_path.clone();
        }
        self.active_edit_loop = None;
        ctx.reset_session_tokens();
        self.reset_conversation_window(ctx);
        self.push_document_notice(
            format!("[resumed: {restored_id} status={status}]"),
            crate::runtime::NoticeSeverity::Info,
        );
    }

    pub(super) fn reset_turn_capture(&mut self) {
        self.task_doc.active_turn = None;
        self.stream_uses_structured_final_output = false;
        self.overlay_state.approved_tool_steps.clear();
        self.selected_timeline_index = 0;
        self.timeline_follow_mode = true;
        self.inspector_scroll_offset = 0;
        self.turn_started_at = None;
        self.ttft = None;
        self.turn_completion_pending = false;
        self.plan_turn_active = false;
    }

    
    pub(super) fn append_turn_timing_line(&mut self) {
        let total = match self.last_turn_duration {
            Some(d) => d,
            None => return,
        };
        let mut parts = Vec::new();
        if let Some(ttft) = self.last_turn_ttft {
            parts.push(format!("ttft:{:.1}s", ttft.as_secs_f64()));
        }
        let timings = self
            .task_doc
            .completed_turns
            .last()
            .and_then(|t| t.timings.as_ref());
        if let Some(t) = timings {
            if let Some(prompt_ms) = t.prompt_ms {
                let tok_suffix = t
                    .prompt_n
                    .map(|n| format!(" ({n} tok)"))
                    .unwrap_or_default();
                parts.push(format!("\u{2191}:{:.1}s{tok_suffix}", prompt_ms / 1000.0));
            }
            if let Some(predicted_ms) = t.predicted_ms {
                let tok_suffix = t
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
        self.push_document_notice(
            format!("[{}]", parts.join(" | ")),
            crate::runtime::NoticeSeverity::Info,
        );
    }

    
    pub(super) fn persist_task_document(&mut self) {
        let snapshot = self.task_doc_condenser.persistable_snapshot(&self.task_doc);
        let dir = TaskState::state_dir_from(&self.working_dir);
        if let Err(error) = snapshot.save(&dir) {
            eprintln!("[state] save failed: {error}");
        }
    }

    pub(super) fn set_task_status(&mut self, status: TaskStatus) {
        self.task_doc.info.status = status;
        self.persist_task_document();
    }

    
    pub(super) fn sync_session_tasks_from_disk(&mut self) {
        let state_dir = TaskState::state_dir_from(&self.working_dir);
        if let Ok(saved) = TaskState::load(&state_dir, &self.task_doc.info.id) {
            self.task_doc.session_tasks = saved.session_tasks;
        }
    }

    pub(super) fn begin_turn_capture(&mut self, input: String) {
        self.begin_turn_capture_with_policy(input, crate::state::TurnToolPolicy::Default);
    }

    pub(super) fn begin_turn_capture_with_policy(
        &mut self,
        input: String,
        tool_policy: crate::state::TurnToolPolicy,
    ) {
        self.reset_turn_capture();
        self.task_doc_condenser
            .begin_turn(&mut self.task_doc, input, now_millis(), tool_policy);
        self.set_task_status(TaskStatus::Running);
        self.transcript_scroll_offset = 0;
        self.turn_started_at = Some(Instant::now());
        self.last_error_message = None;
    }

    pub(super) fn begin_command_session(&mut self, command: String) -> u64 {
        let session_id = self.task_doc.info.next_step_id;
        self.task_doc.info.next_step_id += 1;
        self.begin_command_session_with_id(session_id, command);
        session_id
    }

    pub(super) fn begin_command_session_with_id(&mut self, session_id: u64, command: String) {
        if let Some(t) = self.task_doc.active_turn.as_mut() {
            t.command_sessions.entry(session_id).or_insert_with(|| {
                crate::runtime::task_document::CommandSessionDocument {
                    session_id,
                    command,
                    pid: None,
                    status: "running".to_string(),
                    output_tail: vec![],
                }
            });
        }
        self.set_task_status(TaskStatus::Running);
    }

    pub(super) fn complete_turn_if_idle(&mut self, ctx: &RuntimeContext) -> bool {
        let sessions_empty = self
            .task_doc
            .active_turn
            .as_ref()
            .is_none_or(|t| t.command_sessions.is_empty());
        if !self.turn_completion_pending || !sessions_empty {
            return false;
        }
        self.last_error_message = None;
        self.resolve_pending_approval(false, ctx);
        self.resolve_pending_patch_approval(false);
        self.commit_completed_turn(ctx);
        self.append_turn_timing_line();
        self.maybe_extract_auto_memory();
        if let Some(active) = self.task_doc.active_turn.as_mut() {
            active.cancel_pending = false;
        }
        self.read_only_turn_active = false;
        self.turn_completion_pending = false;
        self.transcript_scroll_offset = 0;
        self.inspector_scroll_offset = 0;
        true
    }

    pub(super) fn commit_completed_turn(&mut self, ctx: &RuntimeContext) {
        let has_content = self.task_doc.active_turn.as_ref().is_some_and(|active| {
            !active.input.trim().is_empty()
                || active
                    .entries
                    .iter()
                    .any(|e| !matches!(e, crate::runtime::TurnEntry::SystemNotice { .. }))
        });

        if self.plan_turn_active
            && let Some(active) = self.task_doc.active_turn.as_ref()
        {
            let plan_text = active.entries.iter().rev().find_map(|e| {
                if let crate::runtime::TurnEntry::AssistantBlock { block, .. } = e
                    && block.phase == crate::runtime::AssistantPhase::Final
                    && !block.content.trim().is_empty()
                {
                    return Some(block.content.clone());
                }
                None
            });
            if let Some(plan) = plan_text {
                
                self.task_doc
                    .session_notes
                    .push(crate::runtime::task_state::SessionNote {
                        content: format!("[plan] {plan}"),
                        created_at_turn: self.task_doc.completed_turns.len(),
                    });
            }
        }

        let turn_tokens = ctx.session_tokens_rollup().last_turn();

        if !has_content {
            self.task_doc.info.status = TaskStatus::Completed;
            self.persist_task_document();
            self.reset_turn_capture();
            return;
        }

        self.last_turn_duration = self.turn_started_at.map(|started| started.elapsed());
        self.last_turn_ttft = self.ttft;
        self.last_error_message = None;

        self.task_doc_condenser.finish_turn(
            &mut self.task_doc,
            TurnOutcome::Completed,
            turn_tokens,
            now_millis(),
        );

        self.persist_task_document();
        self.transcript_scroll_offset = 0;
        self.inspector_scroll_offset = 0;
        self.reset_turn_capture();
    }

    pub(super) fn summarize_usage_line_suffix(estimated: bool) -> &'static str {
        if estimated { " (estimated)" } else { "" }
    }

    fn maybe_extract_auto_memory(&mut self) {
        if !self.auto_memory_enabled {
            return;
        }
        let last_turn = match self.task_doc.completed_turns.last() {
            Some(t) => t,
            None => return,
        };
        let input = last_turn.input.clone();
        let response =
            crate::app::transcript_projection::extract_assistant_response(&last_turn.entries);
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
        let Some(path) = path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match crate::auto_memory::append_auto_notes(&formatted_notes, &path) {
            Ok(()) => {
                let turn_index = self.task_doc.completed_turns.len();
                for note in &formatted_notes {
                    self.task_doc
                        .session_notes
                        .push(crate::runtime::task_state::SessionNote {
                            content: note.clone(),
                            created_at_turn: turn_index,
                        });
                }
                self.persist_task_document();
                self.push_document_notice(
                    format!("[memory] auto: {} note(s) saved", formatted_notes.len()),
                    crate::runtime::NoticeSeverity::Info,
                );
            }
            Err(e) => {
                self.push_document_notice(
                    format!("[memory] auto extraction error: {e}"),
                    crate::runtime::NoticeSeverity::Warning,
                );
            }
        }
    }
}
