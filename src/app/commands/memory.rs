use super::super::*;

impl TuiMode {
    pub(crate) fn handle_memory_display(&mut self) {
        let content = self
            .resolved_existing_notes_path()
            .and_then(|path| crate::runtime::memory_candidates::load_or_migrate(&path).ok());
        match content {
            Some(store) if !store.candidates.is_empty() => {
                let accepted: Vec<_> = store
                    .candidates
                    .iter()
                    .filter(|candidate| {
                        candidate.status
                            == crate::runtime::memory_candidates::CandidateStatus::Accepted
                    })
                    .collect();
                let pending: Vec<_> = store
                    .candidates
                    .iter()
                    .filter(|candidate| {
                        candidate.status
                            == crate::runtime::memory_candidates::CandidateStatus::Pending
                    })
                    .collect();
                if !accepted.is_empty() {
                    self.push_history_line("[memory] accepted".to_string());
                    for candidate in accepted {
                        self.push_history_line(format!("  - {}", candidate.body));
                    }
                }
                if !pending.is_empty() {
                    self.push_history_line("[memory] pending (use /memory accept <n>)".to_string());
                    for (index, candidate) in pending.iter().enumerate() {
                        self.push_history_line(format!("  {}. {}", index + 1, candidate.body));
                    }
                }
            }
            _ => {
                self.push_history_line("[memory] no notes".to_string());
            }
        }
    }
    pub(crate) fn handle_memory_add(&mut self, note: String) {
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
        match crate::runtime::memory_candidates::add_user_note(&path, &note) {
            Ok(()) => {
                self.task_doc.session_notes.push(SessionNote {
                    content: note,
                    created_at_turn: self.task_doc.completed_turns.len(),
                });
                self.persist_task_document();
                self.push_history_line("[memory: note added]".to_string());
            }
            Err(e) => {
                self.push_history_line(format!("[memory] error writing: {e}"));
            }
        }
    }
    pub(crate) fn handle_memory_accept(&mut self, selector: &str) {
        let selector = selector.trim();
        if selector.is_empty() {
            self.push_history_line("[memory] usage: /memory accept <n|topic>".to_string());
            return;
        }
        let path = self
            .resolved_existing_notes_path()
            .or_else(|| self.resolved_notes_path());
        let Some(path) = path else {
            self.push_history_line("[memory] error resolving notes path".to_string());
            return;
        };
        match crate::runtime::memory_candidates::accept_pending(&path, selector) {
            Ok(Some(body)) => {
                self.push_history_line(format!("[memory] accepted: {body}"));
            }
            Ok(None) => {
                self.push_history_line("[memory] no matching pending candidate".to_string());
            }
            Err(e) => {
                self.push_history_line(format!("[memory] error accepting: {e}"));
            }
        }
    }
    pub(crate) fn handle_memory_clear_input(&mut self, input: &str) {
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
                if let Err(e) = crate::runtime::memory_candidates::clear_store(&path) {
                    self.push_history_line(format!("[memory] error clearing: {e}"));
                    return;
                }
                self.task_doc.session_notes.clear();
                self.persist_task_document();
                self.push_history_line("[memory: cleared]".to_string());
            }
            _ => {
                self.push_history_line("[memory: cancelled]".to_string());
            }
        }
    }

    pub(crate) fn handle_reindex_command(&mut self, ctx: &RuntimeContext) {
        if !self.search_config.enabled {
            self.push_history_line(
                "[search] /reindex unavailable: search is disabled by [search].enabled=false"
                    .to_string(),
            );
            return;
        }

        let starting_batch = self
            .task_doc
            .active_pulse
            .as_ref()
            .is_none_or(|t| t.command_sessions.is_empty());
        if starting_batch {
            self.begin_turn_capture("/reindex".to_string());
        }

        let session_id = self.begin_command_session("/reindex".to_string());
        let working_dir = self.working_dir.clone();
        let exclude = self.search_config.exclude.clone();
        let max_file_size = self.search_config.max_file_size;
        let ctx = ctx.clone();

        tokio::spawn(async move {
            ctx.emit_transcript_line(format_command_session_started("/reindex", None));
            match tokio::task::spawn_blocking(move || {
                crate::state::force_full_reindex_with_config(&working_dir, &exclude, max_file_size)
            })
            .await
            {
                Ok(chunk_count) => {
                    ctx.emit_transcript_line(format!(
                        "[search] index rebuilt: {} chunks indexed",
                        chunk_count
                    ));
                }
                Err(error) => {
                    ctx.emit_transcript_line(format!("[search] reindex failed: {error}"));
                }
            }
            ctx.emit_command_session_finished(session_id);
            ctx.emit_turn_complete();
        });
    }

    pub(crate) fn handle_memory_auto_on(&mut self) {
        self.auto_memory_enabled = true;
        self.push_history_line("[memory] auto extraction enabled".to_string());
    }

    pub(crate) fn handle_memory_auto_off(&mut self) {
        self.auto_memory_enabled = false;
        self.push_history_line("[memory] auto extraction disabled".to_string());
    }

    pub(crate) fn handle_memory_auto_clear(&mut self) {
        let path = self
            .resolved_existing_notes_path()
            .or_else(|| self.resolved_notes_path());
        let Some(path) = path else {
            self.push_history_line("[memory] no notes file to clear auto entries from".to_string());
            return;
        };
        match crate::runtime::memory_candidates::remove_feedback_candidates(&path) {
            Ok(removed) if removed > 0 => {
                self.task_doc
                    .session_notes
                    .retain(|n| !crate::auto_memory::is_auto_note_line(&n.content));
                self.persist_task_document();
                self.push_history_line(format!("[memory] removed {removed} auto note(s)"));
            }
            Ok(_) => {
                self.push_history_line("[memory] no auto notes found".to_string());
            }
            Err(e) => {
                self.push_history_line(format!("[memory] error removing auto notes: {e}"));
            }
        }
    }
}
