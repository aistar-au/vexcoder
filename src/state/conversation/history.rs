use super::ConversationManager;
use crate::config::CompactionConfig;
use crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES;
use crate::state::conversation::tools::routing::missing_read_only_location_prompt;
use crate::tool_preview::{
    ReadFileRollupSummary, ReadFileSummaryMessageStyle, ToolPreviewStyle,
    format_read_file_rollup_message, preview_tool_input, read_file_path,
};
use crate::types::{ApiMessage, Content, ContentBlock};
use anyhow::Result;
use std::time::Duration;

const LOCAL_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 1_200;
const LOCAL_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 2_500;
const LOCAL_DEFAULT_MAX_API_MESSAGES: usize = 14;
const LOCAL_DEFAULT_TOOL_TIMEOUT_SECS: u64 = 20;
const LOCAL_IMPLICIT_COMPACTION_THRESHOLD_PERCENT: u8 = 60;
const LOCAL_IMPLICIT_COMPACTION_KEEP_RECENT_TURNS: usize = 1;
const LOCAL_IMPLICIT_COMPACTION_SUMMARY_MAX_TOKENS: usize = 384;
const REMOTE_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 3_000;
const REMOTE_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 6_000;
const REMOTE_DEFAULT_MAX_API_MESSAGES: usize = 32;
const REMOTE_DEFAULT_TOOL_TIMEOUT_SECS: u64 = 60;

#[derive(Clone, Copy)]
pub(super) struct HistoryLimits {
    pub(super) max_assistant_history_chars: usize,
    pub(super) max_tool_result_history_chars: usize,
    pub(super) max_api_messages: usize,
}

impl ConversationManager {
    #[cfg(test)]
    pub(super) fn prune_message_history(&mut self, max_api_messages: usize) {
        if self.api_messages.len() <= max_api_messages {
            return;
        }

        let len = self.api_messages.len();
        let target_keep_start = len.saturating_sub(max_api_messages);
        let mut keep_start = target_keep_start;

        while keep_start < len {
            let message = &self.api_messages[keep_start];
            if message.role == "user" && !message_contains_tool_result(message) {
                break;
            }
            keep_start += 1;
        }

        if keep_start >= len {
            keep_start = target_keep_start;
        }

        if keep_start > 0 {
            self.api_messages.drain(0..keep_start);
        }
    }

    pub(super) fn prune_message_history_preserving(
        &mut self,
        max_api_messages: usize,
        preserve_index: usize,
    ) -> usize {
        if self.api_messages.is_empty() {
            return 0;
        }
        if self.api_messages.len() <= max_api_messages {
            return preserve_index.min(self.api_messages.len().saturating_sub(1));
        }

        let len = self.api_messages.len();
        let target_keep_start = len.saturating_sub(max_api_messages);
        let preserve_distance = target_keep_start.saturating_sub(preserve_index);
        let keep_preserve_anchor = preserve_index < target_keep_start && preserve_distance <= 2;
        let mut keep_start = if keep_preserve_anchor {
            preserve_index
        } else {
            target_keep_start
        };
        let fallback_keep_start = keep_start;

        while keep_start < len {
            if keep_preserve_anchor && keep_start == preserve_index {
                break;
            }
            let message = &self.api_messages[keep_start];
            if message.role == "user" && !message_contains_tool_result(message) {
                break;
            }
            keep_start += 1;
        }

        // The anchor heuristic above can leave keep_start below target_keep_start, which
        // would keep more than max_api_messages and cause repeated context-overflow retries
        // in send_message_with_policy. Clamp back to honour the contract.
        keep_start = keep_start.max(target_keep_start);

        if keep_start >= len {
            keep_start = fallback_keep_start.min(len.saturating_sub(1));
        }

        if keep_start > 0 {
            self.api_messages.drain(0..keep_start);
            preserve_index.saturating_sub(keep_start)
        } else {
            preserve_index
        }
    }

    #[cfg(test)]
    pub(super) fn compact_for_context_overflow(&mut self) {
        let _ = self.compact_for_context_overflow_inner(4, None);
    }

    pub(super) fn compact_for_context_overflow_preserving(
        &mut self,
        keep_messages: usize,
        preserve_index: usize,
    ) -> usize {
        self.compact_for_context_overflow_inner(keep_messages, Some(preserve_index))
    }

    fn compact_for_context_overflow_inner(
        &mut self,
        keep_messages: usize,
        preserve_index: Option<usize>,
    ) -> usize {
        if self.api_messages.is_empty() {
            return 0;
        }

        let keep_messages = keep_messages.max(1);
        self.condense_all_tool_results_for_overflow();
        if self.api_messages.len() <= keep_messages {
            return preserve_index
                .unwrap_or(0)
                .min(self.api_messages.len().saturating_sub(1));
        }
        let len = self.api_messages.len();
        let target_keep_start = len.saturating_sub(keep_messages);
        let preserve_index = preserve_index.map(|index| index.min(len.saturating_sub(1)));
        let keep_preserve_anchor = preserve_index.is_some_and(|index| index < target_keep_start);
        let mut keep_start = if keep_preserve_anchor {
            preserve_index.expect("preserve index must exist when preserving anchor")
        } else {
            target_keep_start
        };
        let fallback_keep_start = keep_start;

        while keep_start < len {
            if keep_preserve_anchor && preserve_index == Some(keep_start) {
                break;
            }
            let msg = &self.api_messages[keep_start];
            if msg.role == "user" && !message_contains_tool_result(msg) {
                break;
            }
            keep_start += 1;
        }

        if keep_start >= len {
            keep_start = fallback_keep_start.min(len.saturating_sub(1));
        }

        if keep_start > 0 && keep_start < len {
            self.api_messages.drain(0..keep_start);
            preserve_index.unwrap_or(0).saturating_sub(keep_start)
        } else {
            preserve_index.unwrap_or(0)
        }
    }

    fn condense_all_tool_results_for_overflow(&mut self) {
        for message in &mut self.api_messages {
            if message.role != "user" {
                continue;
            }
            match &mut message.content {
                Content::Blocks(blocks) => {
                    for block in blocks.iter_mut() {
                        if let ContentBlock::ToolResult { content, .. } = block {
                            *content = truncate_to_lines(content, CONDENSED_TOOL_RESULT_LINES);
                        }
                    }
                }
                Content::Text(text) => {
                    if text_protocol_tool_result_message(text) {
                        *text = condense_text_protocol_tool_results(text);
                    }
                }
            }
        }
    }

    pub(super) fn estimate_history_tokens(&self) -> usize {
        self.api_messages
            .iter()
            .map(|msg| match &msg.content {
                Content::Text(t) => t.len(),
                Content::Blocks(blocks) => blocks
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text { text, .. } => text.len(),
                        ContentBlock::ToolUse { input, .. } => input.to_string().len(),
                        ContentBlock::ToolResult { content, .. } => content.len(),
                        _ => 0,
                    })
                    .sum(),
            })
            .sum::<usize>()
            / 4
    }

    pub(super) fn should_compact_proactively(
        &self,
        config: &CompactionConfig,
        context_window_tokens: usize,
    ) -> bool {
        if !config.enabled || context_window_tokens == 0 {
            return false;
        }
        let threshold = context_window_tokens * (config.threshold_percent as usize) / 100;
        self.estimate_history_tokens() > threshold
    }

    fn effective_proactive_compaction_config(
        &self,
        requires_tool_evidence: bool,
    ) -> CompactionConfig {
        if self.compaction_config.enabled
            || !self.client.is_local_endpoint()
            || !requires_tool_evidence
        {
            return self.compaction_config.clone();
        }

        // Local endpoints with small context windows degrade quickly when a
        // read-heavy prior turn is carried forward verbatim into the next file-
        // inspection request. In that lane, prefer a one-turn checkpoint so the
        // current prompt remains dominant and the older turn is preserved only as
        // a concise handoff summary.
        CompactionConfig {
            enabled: true,
            threshold_percent: LOCAL_IMPLICIT_COMPACTION_THRESHOLD_PERCENT,
            keep_recent_turns: LOCAL_IMPLICIT_COMPACTION_KEEP_RECENT_TURNS,
            summary_max_tokens: LOCAL_IMPLICIT_COMPACTION_SUMMARY_MAX_TOKENS,
        }
    }

    fn should_force_local_handoff_compaction(&self, requires_tool_evidence: bool) -> bool {
        !self.compaction_config.enabled
            && self.client.is_local_endpoint()
            && requires_tool_evidence
            && self.api_messages.len() >= 2
    }

    pub(super) fn compact_with_summary(
        &mut self,
        keep_recent_turns: usize,
        summary_text: &str,
    ) -> usize {
        let len = self.api_messages.len();
        if len == 0 {
            return 0;
        }

        let mut user_count = 0usize;
        let mut boundary = 0;
        for i in (0..len).rev() {
            if self.api_messages[i].role == "user"
                && !message_contains_tool_result(&self.api_messages[i])
            {
                user_count += 1;
                if user_count >= keep_recent_turns {
                    boundary = i;
                    break;
                }
            }
        }

        if boundary == 0 {
            return 0;
        }

        let removed = boundary;
        self.api_messages.drain(0..boundary);

        if !summary_text.is_empty()
            && let Some(first_message) = self.api_messages.first_mut()
            && first_message.role == "user"
            && !message_contains_tool_result(first_message)
            && let Content::Text(text) = &mut first_message.content
        {
            *text = format!(
                "[conversation summary] {summary_text}\n\nUse the summary only as background context. Treat the current request below as authoritative and do not continue earlier file targets unless the current request asks for them.\n\n[current request]\n{text}"
            );
        }

        removed
    }

    pub(super) fn run_proactive_compaction(
        &mut self,
        context_window_tokens: usize,
        requires_tool_evidence: bool,
    ) -> Option<(usize, usize, String)> {
        let config = self.effective_proactive_compaction_config(requires_tool_evidence);
        let force_local_handoff =
            self.should_force_local_handoff_compaction(requires_tool_evidence);
        if !force_local_handoff && !self.should_compact_proactively(&config, context_window_tokens)
        {
            return None;
        }
        let messages_before = self.api_messages.len();
        let keep = config.keep_recent_turns;
        let summary = build_heuristic_summary(&self.api_messages, keep, config.summary_max_tokens);
        let removed = self.compact_with_summary(keep, &summary);
        if removed == 0 {
            return None;
        }
        let messages_after = self.api_messages.len();
        Some((messages_before, messages_after, summary))
    }

    pub(super) fn condense_old_tool_results(&mut self, keep_turns: usize) {
        let len = self.api_messages.len();
        if len == 0 {
            return;
        }

        let mut user_count = 0usize;
        let mut boundary = len;
        for i in (0..len).rev() {
            if self.api_messages[i].role == "user" {
                user_count += 1;
                if user_count >= keep_turns {
                    boundary = i;
                    break;
                }
            }
        }
        if boundary >= len || boundary == 0 {
            return;
        }

        for message in &mut self.api_messages[..boundary] {
            if message.role != "user" {
                continue;
            }
            match &mut message.content {
                Content::Blocks(blocks) => {
                    for block in blocks.iter_mut() {
                        if let ContentBlock::ToolResult { content, .. } = block {
                            *content = truncate_to_lines(content, CONDENSED_TOOL_RESULT_LINES);
                        }
                    }
                }
                Content::Text(text) => {
                    if text.contains("tool_result ") || text.contains("tool_error ") {
                        *text = condense_text_protocol_tool_results(text);
                    }
                }
            }
        }
    }

    pub(super) fn format_tool_result_for_history(
        &mut self,
        name: &str,
        input: &serde_json::Value,
        result: &Result<String>,
    ) -> String {
        let Ok(output) = result else {
            let error = result
                .as_ref()
                .err()
                .map_or_else(|| "Unknown tool error".to_string(), ToString::to_string);
            return format_tool_error_for_model_context(name, input, &error);
        };

        if name == "read_file" {
            let path = read_file_path(input).unwrap_or_else(|| "<missing>".to_string());
            let summary = self.read_file_history_cache.summarize(&path, output);
            return self.format_read_file_result_for_model_context(&path, output, summary);
        }

        if should_enrich_tool_result(name, output) {
            return format_generic_tool_result_for_model_context(name, input, output);
        }

        output.clone()
    }

    pub(super) fn format_read_file_result_for_model_context(
        &self,
        path: &str,
        output: &str,
        summary: ReadFileRollupSummary,
    ) -> String {
        match summary {
            ReadFileRollupSummary::Unchanged { .. } => {
                format_read_file_rollup_message(path, summary, ReadFileSummaryMessageStyle::History)
            }
            ReadFileRollupSummary::FirstRead { .. } | ReadFileRollupSummary::Changed { .. } => {
                let summary_message = match summary {
                    ReadFileRollupSummary::FirstRead { chars, lines } => format!(
                        "Read {path}: {chars} chars, {lines} lines. Snapshot included below for model context."
                    ),
                    ReadFileRollupSummary::Changed {
                        before_chars,
                        before_lines,
                        after_chars,
                        after_lines,
                    } => format!(
                        "Read {path}: content changed ({before_chars} chars/{before_lines} lines -> {after_chars} chars/{after_lines} lines). Snapshot included below for model context."
                    ),
                    ReadFileRollupSummary::Unchanged { .. } => unreachable!(),
                };
                format!(
                    "{summary_message}\nContent for model context:\n--- {path} ---\n{output}\n--- end {path} ---"
                )
            }
        }
    }
}

pub(super) fn message_contains_tool_result(message: &ApiMessage) -> bool {
    match &message.content {
        Content::Blocks(blocks) => blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolResult { .. })),
        Content::Text(text) => text_protocol_tool_result_message(text),
    }
}

fn text_protocol_tool_result_message(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("tool_result ") || trimmed.starts_with("tool_error ")
    })
}

pub(super) fn resolve_history_limits(is_local_endpoint: bool) -> HistoryLimits {
    let defaults = if is_local_endpoint {
        HistoryLimits {
            max_assistant_history_chars: LOCAL_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS,
            max_tool_result_history_chars: LOCAL_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS,
            max_api_messages: LOCAL_DEFAULT_MAX_API_MESSAGES,
        }
    } else {
        HistoryLimits {
            max_assistant_history_chars: REMOTE_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS,
            max_tool_result_history_chars: REMOTE_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS,
            max_api_messages: REMOTE_DEFAULT_MAX_API_MESSAGES,
        }
    };

    HistoryLimits {
        max_assistant_history_chars: env_override_usize(
            "VEX_MAX_ASSISTANT_HISTORY_CHARS",
            defaults.max_assistant_history_chars,
            200,
            20_000,
        ),
        max_tool_result_history_chars: env_override_usize(
            "VEX_MAX_TOOL_RESULT_HISTORY_CHARS",
            defaults.max_tool_result_history_chars,
            200,
            40_000,
        ),
        max_api_messages: env_override_usize(
            "VEX_MAX_API_MESSAGES",
            defaults.max_api_messages,
            4,
            128,
        ),
    }
}

pub(super) fn resolve_tool_timeout(is_local_endpoint: bool) -> Duration {
    let default_secs = if is_local_endpoint {
        LOCAL_DEFAULT_TOOL_TIMEOUT_SECS
    } else {
        REMOTE_DEFAULT_TOOL_TIMEOUT_SECS
    };

    let secs = std::env::var("VEX_TOOL_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default_secs)
        .clamp(2, 300);
    Duration::from_secs(secs)
}

pub(super) fn resolve_max_tool_rounds(is_local_endpoint: bool) -> usize {
    let default_rounds = if is_local_endpoint { 12 } else { 24 };
    std::env::var("VEX_MAX_TOOL_ROUNDS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default_rounds)
        .clamp(2, 64)
}

pub(super) fn env_override_usize(key: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default)
}

const DEFAULT_HISTORY_KEEP_TURNS: usize = 10;
const CONDENSED_TOOL_RESULT_LINES: usize = 5;
const ENRICHED_TOOL_RESULT_PREVIEW_LINES: usize = 5;
const ENRICHED_TOOL_RESULT_INLINE_CHARS: usize = 240;
const ENRICHED_TOOL_RESULT_PREVIEW_LINE_CHARS: usize = 160;

pub(super) fn resolve_history_keep_turns() -> usize {
    env_override_usize("VEX_HISTORY_KEEP_TURNS", DEFAULT_HISTORY_KEEP_TURNS, 2, 64)
}

pub(super) fn truncate_to_lines(text: &str, max_lines: usize) -> String {
    if text
        .lines()
        .last()
        .is_some_and(|l| l.ends_with("more lines)"))
    {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }
    let remaining = lines.len() - max_lines;
    let mut out: String = lines[..max_lines].join("\n");
    out.push_str(&format!("\n({remaining} more lines)"));
    out
}

pub(super) fn truncate_for_history(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }

    let total = chars.len();
    let indicator = format!(
        "\n...[{} chars omitted]...\n",
        total.saturating_sub(max_chars)
    );
    let indicator_len = indicator.chars().count();
    if indicator_len >= max_chars {
        return chars.into_iter().take(max_chars).collect();
    }

    let available = max_chars - indicator_len;
    let keep_head = available / 2;
    let keep_suffix = available - keep_head;

    let head: String = chars.iter().take(keep_head).collect();
    let suffix: String = chars
        .iter()
        .skip(total.saturating_sub(keep_suffix))
        .collect();
    format!("{head}{indicator}{suffix}")
}

fn condense_text_protocol_tool_results(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut content_lines_since_header = 0usize;
    let mut total_remaining = 0usize;
    let mut in_tool_result = false;

    for line in text.lines() {
        let is_header = line.starts_with("tool_result ") || line.starts_with("tool_error ");

        if line.ends_with("more lines)") && line.starts_with('(') {
            if in_tool_result {
                out.push('\n');
                out.push_str(line);
            } else {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(line);
            }
            continue;
        }
        if is_header {
            if in_tool_result && total_remaining > 0 {
                out.push_str(&format!("\n({total_remaining} more lines)"));
                total_remaining = 0;
            }
            in_tool_result = true;
            content_lines_since_header = 0;
            out.push('\n');
            out.push_str(line);
            continue;
        }
        if in_tool_result {
            content_lines_since_header += 1;
            if content_lines_since_header <= CONDENSED_TOOL_RESULT_LINES {
                out.push('\n');
                out.push_str(line);
            } else {
                total_remaining += 1;
            }
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    if in_tool_result && total_remaining > 0 {
        out.push_str(&format!("\n({total_remaining} more lines)"));
    }
    if out.starts_with('\n') && !text.starts_with('\n') {
        out.remove(0);
    }
    out
}

fn should_enrich_tool_result(name: &str, output: &str) -> bool {
    if output.chars().count() > ENRICHED_TOOL_RESULT_INLINE_CHARS || output.contains('\n') {
        return true;
    }

    matches!(
        name,
        "list_files"
            | "list_directory"
            | "list_dir"
            | "glob_files"
            | "find_files"
            | "search_files"
            | "search"
            | "search_content"
            | "codebase_search"
            | "git_status"
            | "git_diff"
            | "git_log"
            | "git_show"
    )
}

fn format_generic_tool_result_for_model_context(
    name: &str,
    input: &serde_json::Value,
    output: &str,
) -> String {
    let request = preview_tool_input(
        name,
        input,
        ToolPreviewStyle::Structured,
        DEFAULT_EDIT_DIFF_CONTEXT_LINES,
    );
    let (chars, lines) = crate::state::conversation::tools::text_stats(output);

    let mut rendered = format!("Tool {name} completed.");
    if !request.trim().is_empty() {
        rendered.push_str("\nRequest:\n");
        rendered.push_str(&indent_block(&request, "  "));
    }
    rendered.push_str(&format!("\nResult: {chars} chars, {lines} lines."));

    if !output.trim().is_empty() {
        rendered.push_str("\nOutput preview:\n");
        rendered.push_str(&indent_block(&preview_tool_output(output), "  "));
    }

    rendered
}

fn format_tool_error_for_model_context(
    name: &str,
    input: &serde_json::Value,
    error: &str,
) -> String {
    let preserve_full_error = missing_read_only_location_prompt(name, input).is_some();
    let normalized_error = preferred_tool_error_message(name, input, error);
    let error_body = if preserve_full_error {
        normalized_error.clone()
    } else {
        preview_tool_output(&normalized_error)
    };
    let request = preview_tool_input(
        name,
        input,
        ToolPreviewStyle::Structured,
        DEFAULT_EDIT_DIFF_CONTEXT_LINES,
    );
    let mut rendered = format!("Tool {name} failed.");
    if !request.trim().is_empty() {
        rendered.push_str("\nRequest:\n");
        rendered.push_str(&indent_block(&request, "  "));
    }
    rendered.push_str("\nError:\n");
    rendered.push_str(&indent_block(&error_body, "  "));

    if let Some(hint) = tool_error_hint(name) {
        rendered.push_str("\nSuggested next step:\n  ");
        rendered.push_str(hint);
    }

    rendered
}

fn preferred_tool_error_message(name: &str, input: &serde_json::Value, error: &str) -> String {
    if let Some(prompt) = missing_read_only_location_prompt(name, input) {
        return prompt;
    }

    error.to_string()
}

fn tool_error_hint(name: &str) -> Option<&'static str> {
    match name {
        "read_file" | "list_files" | "list_directory" | "list_dir" | "glob_files" => Some(
            "Locate the workspace-relative target first, then retry with an explicit path or pattern.",
        ),
        "search_files" | "search" | "search_content" | "codebase_search" => Some(
            "Retry with a focused query or a concrete path scope so the next search result is easier to use.",
        ),
        "write_file" | "apply_patch" | "edit_file" | "rename_file" => Some(
            "Re-read the target path and retry with a streamlined file edit or patch that preserves local style; for Rust, keep the diff rustfmt-consistent.",
        ),
        "git_status" | "git_diff" | "git_log" | "git_show" => {
            Some("Retry with a focused git target if the repository state is larger than expected.")
        }
        _ => None,
    }
}

fn preview_tool_output(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.is_empty() {
        return "<empty>".to_string();
    }

    let mut rendered = lines
        .iter()
        .take(ENRICHED_TOOL_RESULT_PREVIEW_LINES)
        .map(|line| truncate_preview_line(line, ENRICHED_TOOL_RESULT_PREVIEW_LINE_CHARS))
        .collect::<Vec<_>>()
        .join("\n");

    if lines.len() > ENRICHED_TOOL_RESULT_PREVIEW_LINES {
        rendered.push_str(&format!(
            "\n... ({} more lines)",
            lines.len() - ENRICHED_TOOL_RESULT_PREVIEW_LINES
        ));
    }

    rendered
}

fn truncate_preview_line(line: &str, max_chars: usize) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return line.to_string();
    }

    let shortened = chars.into_iter().take(max_chars).collect::<String>();
    format!("{shortened}...")
}

fn indent_block(text: &str, indent: &str) -> String {
    text.lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_heuristic_summary(
    messages: &[ApiMessage],
    keep_recent_turns: usize,
    max_tokens: usize,
) -> String {
    let len = messages.len();
    if len == 0 {
        return String::new();
    }

    let mut user_count = 0usize;
    let mut boundary = 0;
    for i in (0..len).rev() {
        if messages[i].role == "user" && !message_contains_tool_result(&messages[i]) {
            user_count += 1;
            if user_count >= keep_recent_turns {
                boundary = i;
                break;
            }
        }
    }
    if boundary == 0 {
        return String::new();
    }

    let max_bytes = max_tokens * 4;
    let mut parts: Vec<String> = Vec::new();
    let mut total_bytes = 0usize;

    for msg in &messages[..boundary] {
        if msg.role != "user" || message_contains_tool_result(msg) {
            continue;
        }

        let role = &msg.role;
        let first_line = match &msg.content {
            Content::Text(t) => summary_user_text_line(t).unwrap_or_default(),
            Content::Blocks(blocks) => blocks
                .iter()
                .find_map(|b| match b {
                    ContentBlock::Text { text, .. } => text.lines().next().map(String::from),
                    ContentBlock::ToolUse { name, .. } => Some(format!("[tool: {name}]")),
                    ContentBlock::ToolResult { content, .. } => {
                        content.lines().next().map(String::from)
                    }
                    _ => None,
                })
                .unwrap_or_default(),
        };
        if first_line.is_empty() {
            continue;
        }
        let entry = format!("- {role}: {first_line}");
        total_bytes += entry.len();
        if total_bytes > max_bytes {
            break;
        }
        parts.push(entry);
    }

    if parts.is_empty() {
        return String::new();
    }

    parts.join("\n")
}

fn summary_user_text_line(text: &str) -> Option<String> {
    if let Some((_, current_request)) = text.split_once("[current request]\n") {
        return current_request
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_string);
    }

    text.lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with("[conversation summary]")
                && !line.starts_with("Use the summary only as background context.")
                && !line.starts_with("Treat the current request below as authoritative")
        })
        .map(str::to_string)
}
