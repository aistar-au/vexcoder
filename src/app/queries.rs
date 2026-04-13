use super::*;
use crate::ui::editor::file_mention_range;
use std::collections::BinaryHeap;
use std::path::Path;

const MAX_PROMPT_HINT_FILE_MATCHES: usize = 12;
// Keep a wider ranked heap than the visible hint window so the top 12 entries
// stay stable regardless of filesystem walk order.
const MAX_FILE_PROMPT_MATCH_CANDIDATES: usize = 100;

/// Directories excluded from the @ file picker.  `.github/` and `.agents/`
/// are intentionally kept because the server works with workflow and agent
/// configuration files inside them.
const PICKER_EXCLUDED_PREFIXES: &[&str] = &[".git/", "target/", "node_modules/"];

fn is_picker_excluded_path(entry: &str) -> bool {
    PICKER_EXCLUDED_PREFIXES
        .iter()
        .any(|prefix| entry.starts_with(prefix))
}

impl TuiMode {
    pub(super) fn mode_status_label(&self) -> &'static str {
        if self.overlay_active() {
            "overlay"
        } else if self.command_session_active() {
            "command-session"
        } else if self.pending_quit {
            "quit-arm"
        } else if self
            .task_doc
            .active_turn
            .as_ref()
            .is_some_and(|t| t.cancel_pending)
        {
            "cancelling"
        } else if self.task_doc.active_turn.is_some() {
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
        let Ok(paths) = operator.find_files("**") else {
            return Vec::new();
        };

        let mut entries: Vec<String> = paths
            .into_iter()
            .map(|path| operator.to_workspace_relative_display(&path))
            .filter(|entry| !is_picker_excluded_path(entry))
            .collect();

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

    pub(super) fn history_row_count(&self) -> usize {
        let rows = crate::app::transcript_projection::project_transcript_rows(
            &self.task_doc,
            &self.pre_session_notices,
        );
        let strings: Vec<String> = rows.iter().map(|r| r.to_history_string()).collect();
        crate::ui::render::history_visual_line_count(&strings, self.display_column_width.get())
    }

    pub(super) fn total_session_tokens(&self) -> u64 {
        self.task_doc
            .completed_turns
            .iter()
            .map(|t| t.tokens.input + t.tokens.output)
            .sum()
    }

    pub(super) fn tokens_sent_total(&self) -> u64 {
        self.task_doc
            .completed_turns
            .iter()
            .map(|t| t.tokens.input)
            .sum()
    }

    pub(super) fn tokens_received_total(&self) -> u64 {
        self.task_doc
            .completed_turns
            .iter()
            .map(|t| t.tokens.output)
            .sum()
    }

    pub fn status_line(&self) -> String {
        format!(
            "mode:{} approval:{} history:{} repo:{} inst:{} tokens:{}",
            self.mode_status_label(),
            self.approval_status_label(),
            self.history_row_count(),
            self.repo_label,
            self.instructions_path.as_deref().unwrap_or("none"),
            self.total_session_tokens(),
        )
    }

    pub fn current_task_id(&self) -> String {
        self.task_doc.info.id.clone()
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

    pub fn history_lines(&self) -> Vec<String> {
        crate::app::transcript_projection::project_transcript_rows(
            &self.task_doc,
            &self.pre_session_notices,
        )
        .iter()
        .map(|r| r.to_history_string())
        .collect()
    }

    /// Returns `None`; the active-assistant-index concept has been removed.
    /// Kept for compatibility with call sites being migrated.
    pub fn active_assistant_index(&self) -> Option<usize> {
        None
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
        self.task_doc
            .active_turn
            .as_ref()
            .is_some_and(|t| !t.command_sessions.is_empty())
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
                let (suggestions, total_matches) = self.file_prompt_matches_with_total(prefix);
                if suggestions.is_empty() {
                    if prefix.is_empty() {
                        lines.push("[file] no files available".to_string());
                    } else {
                        lines.push(file_picker_no_match_summary(prefix, total_matches));
                    }
                } else {
                    lines.push(file_picker_match_summary(
                        prefix,
                        suggestions.len(),
                        total_matches,
                    ));
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

    pub(crate) fn file_prompt_matches_with_total(&self, prefix: &str) -> (Vec<String>, usize) {
        let needle = prefix.trim();

        if needle.contains('/') || needle.contains('\\') {
            let needle_normalized = needle.replace('\\', "/");
            let (dir_prefix, name_filter) = match needle_normalized.rfind('/') {
                Some(pos) => (&needle_normalized[..=pos], &needle_normalized[pos + 1..]),
                None => ("", needle_normalized.as_str()),
            };
            let total_matches = self.directory_filtered_children(dir_prefix, "").len();
            let matches = if name_filter.is_empty() {
                self.directory_filtered_children(dir_prefix, "")
            } else {
                self.directory_filtered_children(dir_prefix, name_filter)
            };
            return (matches, total_matches);
        }

        let all_entries = self.cached_file_prompt_entries();
        let total_count = all_entries.len();
        let needle_lower = needle.to_ascii_lowercase();
        let mut scored = BinaryHeap::new();

        for display in all_entries {
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

        let matches: Vec<String> = ranked.into_iter().map(|(_, _, display)| display).collect();
        (matches, total_count)
    }

    pub fn file_prompt_matches(&self, prefix: &str) -> Vec<String> {
        self.file_prompt_matches_with_total(prefix).0
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
                entry.len() // file entry: full path
            };
            let child_entry = &entry[..child_end];

            // Filter on the entry's own name.
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

    pub fn set_display_column_width(&self, width: usize) {
        self.display_column_width.set(width.max(1));
    }

    /// Total number of timeline entries available for selection.
    pub(super) fn timeline_entry_count(&self) -> usize {
        let (has_input, tool_count, command_sessions_len) =
            if let Some(active) = self.task_doc.active_turn.as_ref() {
                let tools = active
                    .entries
                    .iter()
                    .filter(|e| matches!(e, crate::runtime::TurnEntry::ToolCall { .. }))
                    .count();
                (
                    !active.input.trim().is_empty(),
                    tools,
                    active.command_sessions.len(),
                )
            } else if let Some(last) = self.task_doc.completed_turns.last() {
                let tools = last
                    .entries
                    .iter()
                    .filter(|e| matches!(e, crate::runtime::TurnEntry::ToolCall { .. }))
                    .count();
                (!last.input.trim().is_empty(), tools, 0)
            } else {
                (false, 0, 0)
            };

        let mut count = 0;
        if has_input {
            count += 1;
        }
        count += tool_count;
        count += command_sessions_len;
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
    let segment_prefix_len = display_lower
        .split('/')
        .filter(|segment| segment.starts_with(needle))
        .map(str::len)
        .min();

    if basename_lower == needle {
        Some((0, basename_lower.len()))
    } else if display_lower == needle {
        Some((1, display_lower.len()))
    } else if basename_lower.starts_with(needle) {
        Some((2, basename_lower.len()))
    } else if let Some(segment_len) = segment_prefix_len {
        Some((3, segment_len))
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
                        "{} · {} · {}",
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
                    label: format!("{} · custom · {}", command.display(), command.description),
                }),
        );
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_excludes_git_internals() {
        assert!(is_picker_excluded_path(".git/HEAD"));
        assert!(is_picker_excluded_path(".git/config"));
        assert!(is_picker_excluded_path(".git/refs/heads/main"));
    }

    #[test]
    fn picker_excludes_target_dir() {
        assert!(is_picker_excluded_path("target/debug/.cargo-lock"));
        assert!(is_picker_excluded_path("target/.rustc_info.json"));
    }

    #[test]
    fn picker_keeps_github_and_agents() {
        assert!(!is_picker_excluded_path(".github/workflows/ci.yml"));
        assert!(!is_picker_excluded_path(".github/agents/profile.md"));
        assert!(!is_picker_excluded_path(".agents/skill.md"));
        assert!(!is_picker_excluded_path(".vex/config.toml"));
    }

    #[test]
    fn picker_keeps_source_files() {
        assert!(!is_picker_excluded_path("src/main.rs"));
        assert!(!is_picker_excluded_path("Cargo.toml"));
        assert!(!is_picker_excluded_path("adr/ADR-README.md"));
    }
}
