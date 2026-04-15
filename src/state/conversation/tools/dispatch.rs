use super::config::{
    read_file_max_lines, write_file_diff_preferred_above_lines, write_file_max_lines,
};
use super::index::{CODEBASE_INDEX, build_codebase_index};
use super::*;
use crate::config::SearchConfig;
use crate::tools::search;
use crate::tools::{ToolOperator, WriteFileOutcome, glob_files, list_dir};
use anyhow::{Result, bail};
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Arc;
use std::sync::Mutex;

#[cfg(test)]
pub(crate) fn execute_tool_blocking_with_operator(
    tool_operator: &ToolOperator,
    name: &str,
    input: &serde_json::Value,
    search_config: &SearchConfig,
    mock_tool_operator_responses: Option<Arc<Mutex<HashMap<String, String>>>>,
) -> Result<String> {
    if let Some(responses_arc) = mock_tool_operator_responses {
        let responses = responses_arc.lock().unwrap();
        if name == "read_file" {
            let path = required_tool_string(input, name, "path")?;
            if let Some(content) = responses.get(path) {
                return Ok(content.clone());
            }
            return Err(anyhow::anyhow!(
                "Mock tool 'read_file' not configured for path: {}",
                path
            ));
        }
    }

    execute_tool_dispatch_with_search_config(tool_operator, name, input, search_config)
}

#[cfg(not(test))]
pub(crate) fn execute_tool_blocking_with_operator(
    tool_operator: &ToolOperator,
    name: &str,
    input: &serde_json::Value,
    search_config: &SearchConfig,
) -> Result<String> {
    execute_tool_dispatch_with_search_config(tool_operator, name, input, search_config)
}

#[cfg(test)]
pub(crate) fn execute_tool_dispatch(
    tool_operator: &ToolOperator,
    name: &str,
    input: &serde_json::Value,
) -> Result<String> {
    execute_tool_dispatch_with_search_config(tool_operator, name, input, &SearchConfig::default())
}

pub(crate) fn execute_tool_dispatch_with_search_config(
    tool_operator: &ToolOperator,
    name: &str,
    input: &serde_json::Value,
    search_config: &SearchConfig,
) -> Result<String> {
    let get_bool =
        |key: &str, default: bool| input.get(key).and_then(|v| v.as_bool()).unwrap_or(default);
    let get_usize = |key: &str, default: usize| {
        input
            .get(key)
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(default)
    };

    match name {
        "read_file" => {
            let path =
                required_tool_string_any(input, name, "path", &["path", "file_path", "file"])?;
            let offset = input
                .get("offset")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let limit = input
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            // Auto-cap reads to preserve context budget. The model can use
            // offset/limit to navigate within large files.
            let auto_limit = read_file_max_lines();
            let effective_limit = limit.or(Some(auto_limit));
            tool_operator.read_file_range(path, offset, effective_limit)
        }
        "write_file" => {
            let path =
                required_tool_string_any(input, name, "path", &["path", "file_path", "file"])?;
            let content = first_tool_string(input, &["content", "text"]).unwrap_or("");
            let (chars, lines) = text_stats(content);

            // Phase 3 hard guard: reject writes above VEX_WRITE_FILE_MAX_LINES.
            let max_lines = write_file_max_lines();
            if lines > max_lines {
                bail!(
                    "write_file rejected: {lines} lines exceeds the {max_lines}-line limit. \
                     Use apply_patch or edit_file for large files."
                );
            }

            let result = match tool_operator.write_file(path, content)? {
                WriteFileOutcome::Written => {
                    format!("Wrote {path} ({chars} chars, {lines} lines).")
                }
                WriteFileOutcome::Pending(pending) => {
                    format!("Pending patch for {path}.\n{}", pending.diff)
                }
            };

            // Phase 3 soft guard: warn when file exceeds diff-preferred threshold.
            let diff_threshold = write_file_diff_preferred_above_lines();
            let warning = if lines > diff_threshold {
                format!(
                    "\nWarning: file has {lines} lines (>{diff_threshold}). \
                     Prefer apply_patch or edit_file for large-file edits."
                )
            } else {
                String::new()
            };

            refresh_codebase_index(path, tool_operator.working_dir(), search_config);
            Ok(format!("{result}{warning}"))
        }
        "apply_patch" => {
            let path =
                required_tool_string_any(input, name, "path", &["path", "file_path", "file"])?;
            let content = required_tool_string_any_preserve(
                input,
                name,
                "content",
                &["content", "text", "new_content"],
            )?;
            let old_content = tool_operator.read_file_if_exists(path)?.unwrap_or_default();
            let pending = tool_operator.propose_patch(path, &old_content, content)?;
            tool_operator.apply_patch(pending)?;
            let (chars, lines) = text_stats(content);
            refresh_codebase_index(path, tool_operator.working_dir(), search_config);
            Ok(format!(
                "Applied patch to {path} ({chars} chars, {lines} lines)."
            ))
        }
        "edit_file" => {
            let path = required_tool_string_any(
                input,
                name,
                "path",
                &["path", "file_path", "file", "filename"],
            )?;
            let old_str = required_tool_string_any_preserve(
                input,
                name,
                "old_str",
                &["old_str", "old_text", "old_string", "find", "search"],
            )?;
            let new_str = first_tool_string(
                input,
                &[
                    "new_str",
                    "new_text",
                    "new_string",
                    "replace",
                    "replace_with",
                    "replacement",
                ],
            )
            .unwrap_or("");
            let (old_chars, old_lines) = text_stats(old_str);
            let (new_chars, new_lines) = text_stats(new_str);
            let summary = if old_lines > 0 && new_lines == 0 {
                format!(
                    "Deleted snippet in {path} ({old_chars} chars/{old_lines} lines -> {new_chars} chars/{new_lines} lines)."
                )
            } else if old_lines == 0 && new_lines > 0 {
                format!(
                    "Inserted snippet in {path} ({old_chars} chars/{old_lines} lines -> {new_chars} chars/{new_lines} lines)."
                )
            } else {
                format!(
                    "Updated snippet in {path} ({old_chars} chars/{old_lines} lines -> {new_chars} chars/{new_lines} lines)."
                )
            };
            tool_operator.edit_file(path, old_str, new_str)?;
            refresh_codebase_index(path, tool_operator.working_dir(), search_config);
            Ok(summary)
        }
        "rename_file" => {
            let old_path = required_tool_string_any(
                input,
                name,
                "old_path",
                &["old_path", "from", "source_path"],
            )?;
            let new_path = required_tool_string_any(
                input,
                name,
                "new_path",
                &["new_path", "to", "target_path"],
            )?;
            let result = tool_operator.rename_file(old_path, new_path)?;
            refresh_codebase_index(old_path, tool_operator.working_dir(), search_config);
            refresh_codebase_index(new_path, tool_operator.working_dir(), search_config);
            Ok(result)
        }
        "list_files" | "list_directory" => tool_operator.list_files(
            first_tool_string(input, &["path", "dir", "directory", "root"]),
            first_tool_usize(input, &["max_entries", "max_results", "limit"]).unwrap_or(100),
        ),
        "list_dir" => list_dir(
            tool_operator,
            first_tool_string(input, &["path", "dir", "directory"]),
            first_tool_usize(input, &["max_entries", "max_results", "limit"]).unwrap_or(200),
        ),
        "glob_files" => {
            let pattern =
                required_tool_string_any(input, name, "pattern", &["pattern", "glob", "query"])?;
            glob_files(
                tool_operator,
                pattern,
                first_tool_usize(input, &["max_results", "limit", "max_entries"]).unwrap_or(50),
            )
        }
        "search_files" | "search" => {
            let query = required_tool_string_any(
                input,
                name,
                "query",
                &["query", "pattern", "text", "search", "needle"],
            )?;
            tool_operator.search_files(
                query,
                first_tool_string(input, &["path", "dir", "directory", "root"]),
                first_tool_usize(input, &["max_results", "limit", "max_entries"]).unwrap_or(30),
            )
        }
        "git_status" => tool_operator.git_status(
            get_bool("short", true),
            input.get("path").and_then(|v| v.as_str()),
        ),
        "git_diff" => tool_operator.git_diff(
            get_bool("cached", false),
            input.get("path").and_then(|v| v.as_str()),
        ),
        "git_log" => tool_operator.git_log(get_usize("max_count", 10)),
        "git_show" => tool_operator.git_show(required_tool_string(input, name, "revision")?),
        "git_add" => tool_operator.git_add(required_tool_string_any(
            input,
            name,
            "path",
            &["path", "file_path", "file"],
        )?),
        "git_commit" => tool_operator.git_commit(required_tool_string_any(
            input,
            name,
            "message",
            &["message", "msg", "commit_message"],
        )?),
        "search_content" => {
            let query = required_tool_string(input, name, "query")?;
            let path_glob = input.get("path_glob").and_then(|v| v.as_str());
            let matches = tool_operator.search_content(query, path_glob)?;
            Ok(matches
                .iter()
                .map(|m| {
                    format!(
                        "{}:{}: {}",
                        tool_operator.to_workspace_relative_display(&m.file),
                        m.line_number,
                        m.line_text
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "find_files" => {
            let name_glob = required_tool_string(input, name, "name_glob")?;
            let files = tool_operator.find_files(name_glob)?;
            Ok(files
                .iter()
                .map(|p| tool_operator.to_workspace_relative_display(p))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "codebase_search" => {
            if !search_config.enabled {
                bail!("codebase_search is disabled by [search].enabled=false");
            }
            let query = required_tool_string(input, name, "query")?;
            let max_results = input
                .get("max_results")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let idx_mutex = CODEBASE_INDEX.get_or_init(|| {
                let chunks = build_codebase_index(tool_operator.working_dir(), search_config);
                Mutex::new(chunks)
            });
            let idx = idx_mutex
                .lock()
                .map_err(|_| anyhow::anyhow!("codebase index lock poisoned"))?;
            let results = search::codebase_search(query, &idx, max_results);
            Ok(search::format_search_results(query, &results))
        }
        // run_command is schema-registered for model use (ADR-042 D5 amendment).
        // Execution is async-only through execute_run_command_tool, which applies
        // the full approval overlay (ADR-042 D6). If a call reaches this blocking
        // dispatcher it means the async path was bypassed — reject it.
        "run_command" => bail!("run_command must execute through the async runtime command runner"),
        _ => bail!("Unknown tool: {name}"),
    }
}

/// Read a required non-empty string field from tool input by a single key.
///
/// Delegates to [`required_tool_string_any`] with a single-key slice.
pub(crate) fn required_tool_string<'a>(
    input: &'a serde_json::Value,
    tool: &str,
    key: &str,
) -> Result<&'a str> {
    required_tool_string_any(input, tool, key, &[key])
}

pub(crate) fn first_tool_string<'a>(
    input: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(|v| v.as_str()))
}

pub(crate) fn first_tool_usize(input: &serde_json::Value, keys: &[&str]) -> Option<usize> {
    keys.iter().find_map(|key| {
        input.get(*key).and_then(|value| match value {
            serde_json::Value::Number(number) => number
                .as_u64()
                .and_then(|value| usize::try_from(value).ok()),
            serde_json::Value::String(text) => text.trim().parse::<usize>().ok(),
            _ => None,
        })
    })
}

pub(crate) fn required_tool_string_any<'a>(
    input: &'a serde_json::Value,
    tool: &str,
    canonical_key: &str,
    keys: &[&str],
) -> Result<&'a str> {
    let value = first_tool_string(input, keys).map(str::trim).unwrap_or("");
    if value.is_empty() {
        bail!("{tool} requires a non-empty '{canonical_key}' string argument");
    }
    Ok(value)
}

pub(crate) fn required_tool_string_any_preserve<'a>(
    input: &'a serde_json::Value,
    tool: &str,
    canonical_key: &str,
    keys: &[&str],
) -> Result<&'a str> {
    let value = first_tool_string(input, keys).unwrap_or("");
    if value.is_empty() {
        bail!("{tool} requires a non-empty '{canonical_key}' string argument");
    }
    Ok(value)
}

pub(crate) fn missing_mutating_location_prompt(
    name: &str,
    input: &serde_json::Value,
) -> Option<String> {
    let missing =
        |keys: &[&str]| first_tool_string(input, keys).is_none_or(|value| value.trim().is_empty());

    match name {
        "write_file" => {
            if missing(&["path", "file_path", "file", "filename"]) {
                Some("I need the target file path before creating a file. Please provide an explicit path like `src/calculator.rs`. No file changes were made.".to_string())
            } else {
                None
            }
        }
        "apply_patch" => {
            if missing(&["path", "file_path", "file", "filename"]) {
                Some("I need the target file path before applying a patch. Please provide an explicit path like `src/calculator.rs`. No file changes were made.".to_string())
            } else {
                None
            }
        }
        "edit_file" => {
            if missing(&["path", "file_path", "file", "filename"]) {
                Some("I need the target file path before editing a file. Please provide an explicit path like `src/calculator.rs`. No file changes were made.".to_string())
            } else {
                None
            }
        }
        "rename_file" => {
            if missing(&["old_path", "from", "source_path"])
                || missing(&["new_path", "to", "target_path"])
            {
                Some("I need both source and destination file paths before renaming. Please provide `old_path` and `new_path`. No file changes were made.".to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn missing_read_only_location_prompt(
    name: &str,
    input: &serde_json::Value,
) -> Option<String> {
    let missing =
        |keys: &[&str]| first_tool_string(input, keys).is_none_or(|value| value.trim().is_empty());

    match name {
        "read_file" => {
            if missing(&["path", "file_path", "file", "filename"]) {
                Some("I need an explicit file path before reading a file. Please provide a non-empty `path` such as `src/main.rs` or `adr/ADR-README.md`. If the user referenced a file with `@`, its content is already in the conversation — look for the `[file: ...]` block instead of calling read_file again. Do not retry without a concrete path. No file changes were made.".to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}
