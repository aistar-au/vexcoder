use serde_json::{json, Value};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolSummary {
    pub(crate) name: String,
    pub(crate) description: String,
}

pub(crate) fn builtin_tool_summaries() -> Vec<ToolSummary> {
    let definitions = tool_definitions();
    let Some(entries) = definitions.as_array() else {
        return Vec::new();
    };

    let mut summaries = entries
        .iter()
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?.to_string();
            let description = entry.get("description")?.as_str()?.to_string();
            Some(ToolSummary { name, description })
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.name.cmp(&right.name));
    summaries
}

pub(super) fn tool_definitions() -> serde_json::Value {
    static TOOL_DEFINITIONS: OnceLock<Value> = OnceLock::new();

    TOOL_DEFINITIONS
        .get_or_init(|| {
            json!([
                {
                    "name": "read_file",
                    "description": "Read file content from an explicit non-empty path. For repository overviews, use list_files or codebase_search first. For large files, use offset and limit to read specific line ranges instead of loading the entire file.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Non-empty file path relative to workspace root" },
                            "offset": { "type": "integer", "description": "Starting line number (1-based). Omit to start from line 1." },
                            "limit": { "type": "integer", "description": "Maximum number of lines to return. Omit to read all remaining lines." }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "write_file",
                    "description": "Write file content. Files that exceed the diff-preferred threshold trigger a warning to prefer apply_patch or edit_file; files that exceed the max line limit are rejected outright. Use apply_patch or edit_file for large-file edits.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["path", "content"]
                    }
                },
                {
                    "name": "apply_patch",
                    "description": "Apply the provided full-file content as a patch to an existing workspace path.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["path", "content"]
                    }
                },
                {
                    "name": "edit_file",
                    "description": "Edit existing file by replacing one exact, unique snippet (old_str -> new_str). Do not send entire-file replacements via this tool.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "old_str": { "type": "string" },
                            "new_str": { "type": "string" }
                        },
                        "required": ["path", "old_str", "new_str"]
                    }
                },
                {
                    "name": "rename_file",
                    "description": "Rename or move a file within the workspace.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "old_path": { "type": "string" },
                            "new_path": { "type": "string" }
                        },
                        "required": ["old_path", "new_path"]
                    }
                },
                {
                    "name": "list_files",
                    "description": "List files and directories under a path. Omit path to list the workspace root; prefer this for repo overviews before targeted file reads.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "max_entries": { "type": "integer", "minimum": 1, "maximum": 2000 }
                        }
                    }
                },
                {
                    "name": "list_directory",
                    "description": "Alias for list_files. List files and directories under a path, or omit path to list the workspace root.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "max_entries": { "type": "integer", "minimum": 1, "maximum": 2000 }
                        }
                    }
                },
                {
                    "name": "list_dir",
                    "description": "List immediate contents of a workspace directory. Not recursive. Respects .gitignore.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "max_entries": { "type": "integer", "minimum": 1, "maximum": 500 }
                        }
                    }
                },
                {
                    "name": "glob_files",
                    "description": "Return workspace-relative file paths matching a glob pattern. Use * for single-segment wildcards and ** for cross-directory matches. Respects .gitignore.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "pattern": { "type": "string" },
                            "max_results": { "type": "integer", "minimum": 1, "maximum": 200 }
                        },
                        "required": ["pattern"]
                    }
                },
                {
                    "name": "search_files",
                    "description": "Search text across files and return matching lines. Respects .gitignore.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "path": { "type": "string" },
                            "max_results": { "type": "integer", "minimum": 1, "maximum": 200 }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "search",
                    "description": "Alias for search_files. Search text across files and return matching lines.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "path": { "type": "string" },
                            "max_results": { "type": "integer", "minimum": 1, "maximum": 200 }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "git_status",
                    "description": "Show git repository status.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "short": { "type": "boolean" },
                            "path": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "git_diff",
                    "description": "Show git diff for working tree or staged changes.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "cached": { "type": "boolean" },
                            "path": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "git_log",
                    "description": "Show recent git commit history.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "max_count": { "type": "integer", "minimum": 1, "maximum": 100 }
                        }
                    }
                },
                {
                    "name": "git_show",
                    "description": "Show details for a git revision.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "revision": { "type": "string" }
                        },
                        "required": ["revision"]
                    }
                },
                {
                    "name": "git_add",
                    "description": "Stage a file or directory for commit.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "git_commit",
                    "description": "Create a commit with the provided message.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "message": { "type": "string" }
                        },
                        "required": ["message"]
                    }
                },
                {
                    "name": "search_content",
                    "description": "Search file contents for a text query within the workspace.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "path_glob": { "type": "string" }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "find_files",
                    "description": "Find files by name pattern within the workspace.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "name_glob": { "type": "string" }
                        },
                        "required": ["name_glob"]
                    }
                },
                {
                    "name": "codebase_search",
                    "description": "Search the codebase for functions, types, and code patterns by name or keyword. Returns ranked code snippets with file paths and line numbers, and when embeddings are configured it also performs semantic reranking backed by a persisted index under .vex/index/. Prefer this over read_file for exploring unfamiliar code or producing a quick repo overview.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Natural language or identifier search query" },
                            "max_results": { "type": "integer", "description": "Maximum results to return (default 10)", "minimum": 1, "maximum": 50 }
                        },
                        "required": ["query"]
                    }
                }
            ])
        })
        .clone()
}

pub(super) fn tool_definitions_with_extra(extra: &[Value]) -> serde_json::Value {
    if extra.is_empty() {
        return tool_definitions();
    }
    let mut definitions = tool_definitions().as_array().cloned().unwrap_or_default();
    definitions.extend(extra.iter().cloned());
    Value::Array(definitions)
}

pub(super) fn tool_definitions_chat_compat_with_extra(extra: &[Value]) -> Value {
    let base = tool_definitions_with_extra(extra);
    let converted = base
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.get("name").cloned().unwrap_or_else(|| json!("")),
                            "description": tool.get("description").cloned().unwrap_or_else(|| json!("")),
                            "parameters": tool
                                .get("input_schema")
                                .cloned()
                                .unwrap_or_else(|| json!({ "type": "object" })),
                        }
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Value::Array(converted)
}
