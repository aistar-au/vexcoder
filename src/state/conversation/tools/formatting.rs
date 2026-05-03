use crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES;
use crate::tool_preview::{ToolPreviewStyle, preview_tool_input};
use crate::types::ContentBlock;
use crate::util::parse_bool_flag;

pub(crate) fn text_stats(text: &str) -> (usize, usize) {
    (
        text.chars().count(),
        text.lines().count().max(usize::from(!text.is_empty())),
    )
}

pub(crate) fn default_tool_approval_enabled(is_local_endpoint: bool) -> bool {
    !is_local_endpoint
}

pub(crate) fn tool_approval_enabled(is_local_endpoint: bool) -> bool {
    std::env::var("VEX_TOOL_CONFIRM")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(default_tool_approval_enabled(is_local_endpoint))
}

pub(crate) fn tool_input_preview(tool_name: &str, input: &serde_json::Value) -> String {
    preview_tool_input(
        tool_name,
        input,
        ToolPreviewStyle::Compact,
        DEFAULT_EDIT_DIFF_CONTEXT_LINES,
    )
}

pub(crate) fn render_tool_calls_for_text_protocol(blocks: &[ContentBlock]) -> String {
    let mut out = String::new();
    for block in blocks {
        if let ContentBlock::ToolUse { name, input, .. } = block {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("<function={name}>\n"));

            if let Some(obj) = input.as_object() {
                let mut keys: Vec<_> = obj.keys().collect();
                keys.sort_unstable();
                for key in keys {
                    let value = obj
                        .get(key)
                        .map(json_value_to_text_protocol_value)
                        .unwrap_or_default();
                    out.push_str(&format!("<parameter={key}>\n{value}\n</parameter>\n"));
                }
            }

            out.push_str("</function>");
        }
    }
    out
}

fn json_value_to_text_protocol_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

pub(crate) fn render_loop_limit_guard_message(
    last_assistant_text: &str,
    max_rounds: usize,
) -> String {
    render_loop_guard_message(
        last_assistant_text,
        format!("Stopped after {max_rounds} tool rounds to prevent an infinite loop."),
    )
}

pub(crate) fn render_repeated_tool_guard_message(last_assistant_text: &str) -> String {
    render_loop_guard_message(
        last_assistant_text,
        "Repeated identical read/search tool calls detected; stopped to prevent an infinite loop."
            .to_string(),
    )
}

pub(crate) fn render_repeated_mutating_tool_guard_message(last_assistant_text: &str) -> String {
    render_loop_guard_message(
        last_assistant_text,
        "Repeated identical mutating tool calls detected; stopped to prevent an infinite loop. Verify edit_file arguments include path, old_str, and new_str.".to_string(),
    )
}

pub(crate) fn render_tool_denied_message(tool_name: &str) -> String {
    if tool_requires_confirmation(tool_name) {
        format!("Stopped: approval denied for {tool_name}. No file changes were made.")
    } else {
        format!("Stopped: approval denied for {tool_name}. No tool actions were performed.")
    }
}

pub(crate) fn render_missing_tool_evidence_guard_message(last_assistant_text: &str) -> String {
    render_loop_guard_message(
        last_assistant_text,
        "Model did not call any tool call required to answer this request with workspace evidence."
            .to_string(),
    )
}

pub(crate) fn render_loop_guard_message(last_assistant_text: &str, reason: String) -> String {
    let summary = if last_assistant_text.trim().is_empty() {
        "No final assistant answer was produced.".to_string()
    } else {
        last_assistant_text.to_string()
    };
    format!("{summary}\n\n[loop guard] {reason}")
}

pub(crate) fn is_read_only_tool_name(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "search"
            | "search_files"
            | "search_content"
            | "find_files"
            | "codebase_search"
            | "list_files"
            | "list_directory"
            | "list_dir"
            | "glob_files"
            | "git_status"
            | "git_diff"
            | "git_log"
            | "git_show"
    )
}

pub(crate) fn is_read_only_tool_round(blocks: &[ContentBlock]) -> bool {
    blocks.iter().all(|block| {
        matches!(
            block,
            ContentBlock::ToolUse { name, .. } if is_read_only_tool_name(name)
        )
    })
}

pub(crate) fn tool_can_run_in_parallel(name: &str) -> bool {
    is_read_only_tool_name(name)
}

pub(crate) fn is_parallel_safe_tool_round(blocks: &[ContentBlock]) -> bool {
    !blocks.is_empty()
        && blocks.iter().all(|block| {
            matches!(
                block,
                ContentBlock::ToolUse { name, .. } if tool_can_run_in_parallel(name)
            )
        })
}

pub(crate) fn should_parallelize_tool_round(
    blocks: &[ContentBlock],
    require_tool_approval: bool,
) -> bool {
    !require_tool_approval && blocks.len() > 1 && is_parallel_safe_tool_round(blocks)
}

pub(crate) fn is_mutating_tool_round(blocks: &[ContentBlock]) -> bool {
    blocks.iter().any(|block| {
        matches!(
            block,
            ContentBlock::ToolUse { name, .. } if tool_requires_confirmation(name)
        )
    })
}

pub(crate) fn tool_requires_confirmation(name: &str) -> bool {
    if name.starts_with("mcp.") {
        return true;
    }
    matches!(
        name,
        "write_file"
            | "apply_patch"
            | "edit_file"
            | "rename_file"
            | "git_add"
            | "git_commit"
            | "run_command"
            | "run_shell_command"
            | "bash"
            | "call_command"
            | "call_bash"
    )
}

pub(crate) fn tool_round_signature(blocks: &[ContentBlock]) -> Vec<String> {
    let mut signature = Vec::new();
    for block in blocks {
        if let ContentBlock::ToolUse { name, input, .. } = block {
            let payload = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
            signature.push(format!("{name}:{payload}"));
        }
    }
    signature
}

pub(crate) fn builtin_supported_git_tools_response(input: &str) -> Option<String> {
    let normalized = input.to_ascii_lowercase();
    let asks_git_capabilities = (normalized.contains("git tool")
        || normalized.contains("git tools")
        || normalized.contains("git command")
        || normalized.contains("git commands"))
        && (normalized.contains("what")
            || normalized.contains("which")
            || normalized.contains("can you")
            || normalized.contains("available"));
    if !asks_git_capabilities {
        return None;
    }

    Some(
        "Built-in git tools available here: git_status, git_diff, git_log, git_show, git_add, git_commit."
            .to_string(),
    )
}
