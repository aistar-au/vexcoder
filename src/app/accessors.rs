use super::*;
use crate::ui::editor::file_mention_range;
use std::collections::BTreeSet;
use std::path::Path;

impl TuiMode {
    pub(super) fn mode_status_label(&self) -> &'static str {
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
    fn cached_file_prompt_entries(&self) -> Vec<String> {
        if let Some(entries) = self.file_prompt_entries.borrow().as_ref() {
            return entries.clone();
        }

        let operator = ToolOperator::new(self.working_dir.clone());
        let Ok(paths) = operator.find_files("**/*") else {
            return Vec::new();
        };

        let mut entries = paths
            .into_iter()
            .map(|path| operator.to_workspace_relative_display(&path))
            .collect::<Vec<_>>();
        entries.sort();
        entries.dedup();
        *self.file_prompt_entries.borrow_mut() = Some(entries.clone());
        entries
    }

    pub(super) fn invalidate_file_prompt_entries(&self) {
        *self.file_prompt_entries.borrow_mut() = None;
    }

    pub(super) fn approval_status_label(&self) -> &'static str {
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

        // Sum session tokens from all completed turns so the draw layer can
        // surface a context-window indicator without knowing RuntimeContext.
        let total_tokens: u64 = self
            .current_task
            .turns
            .iter()
            .map(|t| t.tokens.input + t.tokens.output)
            .sum();

        format!(
            "mode:{} approval:{} history:{} repo:{} inst:{} tokens:{}",
            self.mode_status_label(),
            self.approval_status_label(),
            history_rows,
            self.repo_label,
            self.instructions_path.as_deref().unwrap_or("none"),
            total_tokens,
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

    pub(super) fn patch_overlay_active(&self) -> bool {
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

    pub fn prompt_hint_for_input(&self, input: &str, cursor: usize) -> String {
        let base = if self.command_session_active() {
            "Prompt\nsubmit: / commands  @ files  ! shell  Ctrl+C cancels".to_string()
        } else {
            "Prompt\nsubmit: / commands  @ files  ! shell".to_string()
        };

        if self.command_session_active() || self.overlay_active() {
            return base;
        }

        let trimmed = input.trim_start();
        if trimmed.starts_with('/') {
            let suggestions = self.slash_prompt_suggestions(trimmed);
            if !suggestions.is_empty() {
                let mut lines = vec!["Prompt".to_string(), "mode: slash".to_string()];
                lines.extend(suggestions);
                return lines.join("\n");
            }
            return "Prompt\nmode: slash".to_string();
        }

        if let Some(range) = file_mention_range(input, cursor) {
            if let Some(prefix) = input[range].strip_prefix('@') {
                let mut lines = vec!["Prompt".to_string(), "mode: file mention".to_string()];
                let suggestions = self.file_prompt_matches(prefix);
                if suggestions.is_empty() {
                    if prefix.is_empty() {
                        lines.push("[file] no files available".to_string());
                    } else {
                        lines.push(format!("[file] no matches for {prefix}"));
                    }
                } else {
                    lines.extend(suggestions.into_iter().map(|path| format!("[file] {path}")));
                }
                return lines.join("\n");
            }
        }

        base
    }

    pub fn composer_is_focused(&self) -> bool {
        !self.overlay_active() && !self.command_session_active()
    }

    pub fn file_prompt_matches(&self, prefix: &str) -> Vec<String> {
        let mut entries = BTreeSet::new();
        for display in self.cached_file_prompt_entries() {
            let basename = Path::new(&display)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(display.as_str());
            if prefix.is_empty() || display.starts_with(prefix) || basename.starts_with(prefix) {
                entries.insert(display);
                if entries.len() >= 8 {
                    break;
                }
            }
        }

        entries.into_iter().collect()
    }

    pub fn set_history_content_width(&self, width: usize) {
        self.history_content_width.set(width.max(1));
    }

    /// Total number of timeline entries available for selection.
    /// Mirrors the entry count produced by `task_timeline_entries()`.
    pub(super) fn timeline_entry_count(&self) -> usize {
        if !self.command_sessions.is_empty() {
            return self.command_sessions.len().max(1);
        }

        // Use current turn data if available, otherwise fall back to last turn.
        let (input_text, tool_count) = if self.history_state.turn_in_progress
            || !self.current_turn_input.trim().is_empty()
            || !self.current_turn_tool_invocations.is_empty()
            || !self.pending_turn_tool_calls.is_empty()
        {
            (
                &self.current_turn_input,
                self.current_turn_tool_invocations.len() + self.pending_turn_tool_calls.len(),
            )
        } else {
            (
                &self.last_turn_input_display,
                self.last_turn_tool_invocations.len(),
            )
        };

        let mut count = 0;
        if !input_text.trim().is_empty() {
            count += 1;
        }
        count += tool_count;
        count.max(1)
    }
}

impl TuiMode {
    fn slash_prompt_suggestions(&self, token: &str) -> Vec<String> {
        let partial = token.split_whitespace().next().unwrap_or(token);
        let mut rows = SLASH_COMMANDS
            .iter()
            .filter(|spec| {
                spec.display.starts_with(partial)
                    || spec
                        .display
                        .split_whitespace()
                        .next()
                        .is_some_and(|command| command == partial)
            })
            .take(6)
            .map(|spec| {
                format!(
                    "[slash] {} · {}",
                    spec.display,
                    slash_command_mode_summary(spec.id)
                )
            })
            .collect::<Vec<_>>();

        rows.extend(
            self.custom_commands
                .iter()
                .filter(|command| format!("/{}", command.name).starts_with(partial))
                .take(3)
                .map(|command| {
                    format!(
                        "[slash] {} · custom · {}",
                        command.display(),
                        command.description
                    )
                }),
        );
        rows
    }
}

fn slash_command_mode_summary(id: SlashCommandId) -> &'static str {
    match id {
        SlashCommandId::Plan => "read-only · no patch",
        SlashCommandId::Init => "writes .vex + AGENTS in current workspace",
        SlashCommandId::Edit | SlashCommandId::Fix => "edit loop · may patch",
        SlashCommandId::Explain | SlashCommandId::Review => "read-only semantic turn",
        SlashCommandId::Run | SlashCommandId::Test => "local validation only",
        SlashCommandId::Permissions | SlashCommandId::Allow | SlashCommandId::Deny => {
            "session permissions"
        }
        SlashCommandId::Model => "session model selection",
        SlashCommandId::Commands | SlashCommandId::Help => "show command directory",
        _ => "session command",
    }
}
