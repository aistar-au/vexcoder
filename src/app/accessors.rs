use super::*;
use crate::ui::editor::file_mention_range;
use std::collections::BinaryHeap;
use std::path::Path;

const MAX_PROMPT_HINT_FILE_MATCHES: usize = 12;
const MAX_FILE_PROMPT_MATCH_CANDIDATES: usize = 100;

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

        // Derive directory entries from file paths so the picker shows both
        // files and directories (ADR-032 §7).
        let mut dir_set = std::collections::BTreeSet::new();
        for entry in &entries {
            let mut path = Path::new(entry);
            while let Some(parent) = path.parent() {
                let s = parent.to_str().unwrap_or("");
                if s.is_empty() {
                    break;
                }
                dir_set.insert(format!("{s}/"));
                path = parent;
            }
        }
        entries.extend(dir_set);

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
                    lines.push(format!("[file] {} match(es)", suggestions.len()));
                    lines.extend(
                        suggestions
                            .into_iter()
                            .take(MAX_PROMPT_HINT_FILE_MATCHES)
                            .map(|path| format!("[file] {path}")),
                    );
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
        let needle = prefix.trim();

        // Directory navigation mode: when prefix contains `/`, show immediate
        // children of the directory path. This enables hierarchical drill-down:
        //   @         → fuzzy match all entries
        //   @src/     → list immediate children of src/
        //   @src/ui/  → list immediate children of src/ui/
        //   @src/ui/e → filter children of src/ui/ matching "e"
        if needle.contains('/') {
            let (dir_prefix, name_filter) = match needle.rfind('/') {
                Some(pos) => (&needle[..=pos], &needle[pos + 1..]),
                None => ("", needle),
            };
            return self.directory_filtered_children(dir_prefix, name_filter);
        }

        // Fuzzy matching for simple (no-slash) prefixes.
        let needle_lower = needle.to_ascii_lowercase();
        let mut scored = BinaryHeap::new();

        for display in self.cached_file_prompt_entries() {
            let Some((rank, match_len)) = score_file_prompt_entry(&display, &needle_lower) else {
                continue;
            };

            scored.push((rank, match_len, display));
            if scored.len() > MAX_FILE_PROMPT_MATCH_CANDIDATES {
                scored.pop();
            }
        }

        let mut ranked = scored.into_vec();
        ranked.sort_by(
            |(left_rank, left_len, left_path), (right_rank, right_len, right_path)| {
                left_rank
                    .cmp(right_rank)
                    .then_with(|| left_len.cmp(right_len))
                    .then_with(|| left_path.cmp(right_path))
            },
        );

        ranked.into_iter().map(|(_, _, display)| display).collect()
    }

    /// List immediate children of `dir_prefix`, optionally filtered by `name_filter`.
    ///
    /// For `dir_prefix = "src/"` and `name_filter = ""`:
    ///   returns `["src/app/", "src/ui/", "src/lib.rs", ...]`
    ///
    /// For `dir_prefix = "src/ui/"` and `name_filter = "ed"`:
    ///   returns `["src/ui/editor.rs"]`
    fn directory_filtered_children(&self, dir_prefix: &str, name_filter: &str) -> Vec<String> {
        let filter_lower = name_filter.to_ascii_lowercase();
        let dir_lower = dir_prefix.to_ascii_lowercase();
        let mut children = std::collections::BTreeSet::new();

        for entry in self.cached_file_prompt_entries() {
            let entry_lower = entry.to_ascii_lowercase();
            let Some(rest) = entry_lower.strip_prefix(dir_lower.as_str()) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }

            // Extract the immediate child: first path segment after the prefix.
            let child_end = if let Some(slash_pos) = rest.find('/') {
                dir_prefix.len() + slash_pos + 1 // include trailing /
            } else {
                entry.len() // file child: full path
            };
            let child_entry = &entry[..child_end];

            // Filter on the child's own name.
            let child_name_lower = child_entry[dir_prefix.len()..].to_ascii_lowercase();
            if !filter_lower.is_empty()
                && !child_name_lower.starts_with(&filter_lower)
                && !child_name_lower.contains(&filter_lower)
            {
                continue;
            }

            children.insert(child_entry.to_string());
        }

        children.into_iter().collect()
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

fn score_file_prompt_entry(display: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return Some((0, 0));
    }

    let display_lower = display.to_ascii_lowercase();
    let basename = Path::new(display)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(display);
    let basename_lower = basename.to_ascii_lowercase();
    let segment_prefix = display_lower
        .split('/')
        .any(|segment| segment.starts_with(needle));

    if basename_lower == needle {
        Some((0, basename_lower.len()))
    } else if display_lower == needle {
        Some((1, display_lower.len()))
    } else if basename_lower.starts_with(needle) {
        Some((2, basename_lower.len()))
    } else if segment_prefix {
        Some((3, display_lower.len()))
    } else if display_lower.starts_with(needle) {
        Some((4, display_lower.len()))
    } else if basename_lower.contains(needle) {
        Some((5, basename_lower.len()))
    } else if display_lower.contains(needle) {
        Some((6, display_lower.len()))
    } else {
        None
    }
}

impl TuiMode {
    fn slash_prompt_suggestions(&self, token: &str) -> Vec<String> {
        self.slash_picker_matches(token)
            .into_iter()
            .map(|m| m.label)
            .collect()
    }

    pub fn slash_picker_matches(&self, token: &str) -> Vec<SlashPickerMatch> {
        let partial = token.split_whitespace().next().unwrap_or(token);
        let mut rows: Vec<SlashPickerMatch> = SLASH_COMMANDS
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
                let command_word = spec
                    .display
                    .split_whitespace()
                    .next()
                    .unwrap_or(spec.display);
                SlashPickerMatch {
                    command: format!("{command_word} "),
                    label: format!(
                        "[slash] {} · {} · {}",
                        spec.display,
                        slash_command_menu_group(spec.id),
                        slash_command_mode_summary(spec.id)
                    ),
                }
            })
            .collect();

        rows.extend(
            self.custom_commands
                .iter()
                .filter(|command| format!("/{}", command.name).starts_with(partial))
                .take(3)
                .map(|command| SlashPickerMatch {
                    command: format!("/{} ", command.name),
                    label: format!(
                        "[slash] {} · custom · {}",
                        command.display(),
                        command.description
                    ),
                }),
        );
        rows
    }
}
