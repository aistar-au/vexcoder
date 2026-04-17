use super::ConversationManager;
use crate::config::CompactionConfig;
use crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES;
use crate::state::conversation::tools::dispatch::missing_read_only_location_prompt;
use crate::tool_preview::{
    ReadFileRollupSummary, ReadFileSummaryMessageStyle, ToolPreviewStyle,
    format_read_file_rollup_message, preview_tool_input, read_file_path,
};
use crate::types::{ApiMessage, Content, ContentBlock};
use anyhow::Result;
use std::time::Duration;

/// Fixed summarization prompt used when building a heuristic compaction summary.
/// The prompt is intentionally not user-configurable to prevent prompt injection
/// via config (PM-01 constraint).
const COMPACTION_SUMMARY_PROMPT: &str = "\
You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary \
for the next turn. Include: current progress and key decisions made, important \
constraints or user preferences, what remains to be done (clear next steps), \
and any critical data or references needed to continue. Be concise and structured.";

const LOCAL_DEFAULT_MAX_ASSISTANT_HISTORY_CHARS: usize = 1_200;
const LOCAL_DEFAULT_MAX_TOOL_RESULT_HISTORY_CHARS: usize = 2_500;
const LOCAL_DEFAULT_MAX_API_MESSAGES: usize = 14;
const LOCAL_DEFAULT_TOOL_TIMEOUT_SECS: u64 = 20;
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
        let mut keep_start = len.saturating_sub(max_api_messages);

        // MessagesV1 requires history to begin with a user message.
        // Additionally, a leading user tool_result is invalid without its preceding assistant tool_use.
        while keep_start < len {
            let message = &self.api_messages[keep_start];
            if message.role == "user" && !message_contains_tool_result(message) {
                break;
            }
            keep_start += 1;
        }

        if keep_start >= len {
            self.api_messages.clear();
            return;
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

        if keep_start >= len {
            self.api_messages.clear();
            return 0;
        }

        if keep_start > 0 {
            self.api_messages.drain(0..keep_start);
            preserve_index.saturating_sub(keep_start)
        } else {
            preserve_index
        }
    }

    /// Aggressively compact conversation history after a context-overflow
    /// error from the server.  Keeps only the last 4 messages (the current
    /// user message plus the most recent context), ensuring the history
    /// starts with a plain user message (no tool_result).
    pub(super) fn compact_for_context_overflow(&mut self) {
        const KEEP_MESSAGES: usize = 4;
        if self.api_messages.len() <= KEEP_MESSAGES {
            return;
        }
        let len = self.api_messages.len();
        let mut keep_start = len.saturating_sub(KEEP_MESSAGES);

        // MessagesV1 requires the first message to be a plain user message.
        while keep_start < len {
            let msg = &self.api_messages[keep_start];
            if msg.role == "user" && !message_contains_tool_result(msg) {
                break;
            }
            keep_start += 1;
        }

        if keep_start > 0 && keep_start < len {
            self.api_messages.drain(0..keep_start);
        }
    }

    /// Estimate the total token count of the current message history using
    /// a byte-based heuristic (4 bytes per token).
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

    /// Check whether proactive compaction should trigger based on the
    /// configured threshold and an estimated context window size.
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

    /// Run proactive compaction: replace all messages before the most recent
    /// `keep_recent_turns` turns with a single user message containing
    /// `summary_text`. Returns the number of messages removed.
    ///
    /// If `summary_text` is empty, falls back to the existing
    /// keep-recent pruning without a summary prefix.
    ///
    /// Preserves the MessagesV1 invariant that history starts with a
    /// plain user message.
    pub(super) fn compact_with_summary(
        &mut self,
        keep_recent_turns: usize,
        summary_text: &str,
    ) -> usize {
        let len = self.api_messages.len();
        if len == 0 {
            return 0;
        }

        // Find the boundary: count user messages from the end to find the
        // start of the most recent `keep_recent_turns` turns.
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

        // Fold the summary into the first preserved user message so the
        // compacted history still starts with a plain user message and does
        // not create consecutive user-role entries.
        if !summary_text.is_empty()
            && let Some(first_message) = self.api_messages.first_mut()
            && first_message.role == "user"
            && !message_contains_tool_result(first_message)
            && let Content::Text(text) = &mut first_message.content
        {
            *text = format!("[conversation summary] {summary_text}\n\n{text}");
        }

        removed
    }

    /// Run proactive compaction if the threshold is exceeded. Builds a
    /// heuristic summary from the evicted messages (no LLM call) and calls
    /// `compact_with_summary`. Returns `Some((messages_before, messages_after, summary))`
    /// when compaction was performed, or `None` if it was not needed.
    pub(super) fn run_proactive_compaction(
        &mut self,
        context_window_tokens: usize,
    ) -> Option<(usize, usize, String)> {
        if !self.should_compact_proactively(&self.compaction_config, context_window_tokens) {
            return None;
        }
        let messages_before = self.api_messages.len();
        let keep = self.compaction_config.keep_recent_turns;
        let summary = build_heuristic_summary(
            &self.api_messages,
            keep,
            self.compaction_config.summary_max_tokens,
        );
        let removed = self.compact_with_summary(keep, &summary);
        if removed == 0 {
            return None;
        }
        let messages_after = self.api_messages.len();
        Some((messages_before, messages_after, summary))
    }

    /// Condense tool results in messages older than `keep_turns` recent
    /// message pairs. Each affected tool result keeps its first 5 lines plus
    /// a `(N more lines)` indicator.
    pub(super) fn condense_old_tool_results(&mut self, keep_turns: usize) {
        let len = self.api_messages.len();
        if len == 0 {
            return;
        }
        // Count backwards to find the boundary. Each "turn" is roughly a
        // user message followed by an assistant message, but the exact
        // interleaving varies. We count user-role messages from the end.
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
        // Condense tool results in messages before the boundary.
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
                    // Text-protocol tool results are embedded as
                    // "tool_result <name>:\n<content>". Condense the content
                    // portion after each header.
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
            // read_file_path returns None if the "path" key is absent or non-string.
            // The fallback "<missing>" is a display-layer decision kept here, not baked into the helper.
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
        Content::Text(_) => false,
    }
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

/// Number of recent user messages to keep at full fidelity.  Tool results
/// in messages older than this threshold are condensed to their first 5
/// lines.  Configurable via `VEX_HISTORY_KEEP_TURNS`.
const DEFAULT_HISTORY_KEEP_TURNS: usize = 10;
const CONDENSED_TOOL_RESULT_LINES: usize = 5;
const ENRICHED_TOOL_RESULT_PREVIEW_LINES: usize = 5;
const ENRICHED_TOOL_RESULT_INLINE_CHARS: usize = 240;
const ENRICHED_TOOL_RESULT_PREVIEW_LINE_CHARS: usize = 160;

pub(super) fn resolve_history_keep_turns() -> usize {
    env_override_usize("VEX_HISTORY_KEEP_TURNS", DEFAULT_HISTORY_KEEP_TURNS, 2, 64)
}

/// Truncate a tool result to its first `max_lines` lines, appending a
/// `(N more lines)` indicator when content is trimmed.  Idempotent: if the
/// last line already matches the indicator pattern, the text is returned
/// unchanged.
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

/// Condense text-protocol tool results. Each result block starts with a
/// header line like `tool_result read_file:` followed by content lines.
/// We keep the header and the first `CONDENSED_TOOL_RESULT_LINES` content
/// lines, appending a `(N more lines)` indicator for the rest.
/// Idempotent: existing `(N more lines)` indicators are preserved as-is.
fn condense_text_protocol_tool_results(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut content_lines_since_header = 0usize;
    let mut total_remaining = 0usize;
    let mut in_tool_result = false;

    for line in text.lines() {
        let is_header = line.starts_with("tool_result ") || line.starts_with("tool_error ");
        // Idempotency: treat existing indicators as pass-through.
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
        "write_file" | "apply_patch" | "edit_file" | "rename_file" => {
            Some("Re-read the target path and retry with a streamlined file edit or patch.")
        }
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

/// Build a heuristic summary from the messages that will be evicted
/// (everything before the most recent `keep_recent_turns` turns).
/// Extracts the first line of each user message and the first line of
/// each assistant response, capped at `max_tokens * 4` bytes.
fn build_heuristic_summary(
    messages: &[ApiMessage],
    keep_recent_turns: usize,
    max_tokens: usize,
) -> String {
    let len = messages.len();
    if len == 0 {
        return String::new();
    }

    // Find the boundary the same way compact_with_summary does.
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
        let role = &msg.role;
        let first_line = match &msg.content {
            Content::Text(t) => t.lines().next().unwrap_or("").to_string(),
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

    format!("{}\n{}", COMPACTION_SUMMARY_PROMPT, parts.join("\n"))
}
