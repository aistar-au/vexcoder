use super::{ConversationManager, ConversationStreamUpdate, ToolApprovalRequest, TurnToolPolicy};
use crate::config::{HookEvent, HookOnFail};
use crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES;
use crate::runtime::{
    CommandRequest, CommandRunner, DefaultCommandRunner, PassthroughSandbox, SandboxDriver,
};
use crate::tool_preview::{preview_tool_input, ToolPreviewStyle};
use crate::tools::{ToolOperator, WriteFileOutcome};
use crate::types::ContentBlock;
use crate::util::parse_bool_flag;
use anyhow::{bail, Context, Result};
#[cfg(test)]
use std::collections::HashMap;
#[cfg(all(test, not(windows)))]
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

#[cfg(all(test, not(windows)))]
static HOOK_WARNINGS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn emit_hook_warning(message: String) {
    eprintln!("{message}");
    #[cfg(all(test, not(windows)))]
    HOOK_WARNINGS.lock().unwrap().push(message);
}

#[cfg(all(test, not(windows)))]
pub(super) fn take_hook_warnings() -> Vec<String> {
    std::mem::take(&mut *HOOK_WARNINGS.lock().unwrap())
}

impl ConversationManager {
    pub(super) async fn request_tool_approval(
        &self,
        name: &str,
        input: &serde_json::Value,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> bool {
        let Some(tx) = stream_delta_tx else {
            return true;
        };

        let (response_tx, response_rx) = oneshot::channel();
        let request = ToolApprovalRequest {
            tool_name: name.to_string(),
            input_preview: tool_input_preview(name, input),
            response_tx,
        };

        if tx
            .send(ConversationStreamUpdate::ToolApprovalRequest(request))
            .is_err()
        {
            return false;
        }

        response_rx.await.unwrap_or(false)
    }

    // The streamless shim keeps direct unit tests on the hook path small while
    // production call sites route through the update-aware variant below.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) async fn execute_tool_with_timeout(
        &self,
        name: &str,
        input: &serde_json::Value,
        tool_timeout: Duration,
    ) -> Result<String> {
        self.execute_tool_with_timeout_with_updates(name, input, tool_timeout, None)
            .await
    }

    pub(super) async fn execute_tool_with_timeout_with_updates(
        &self,
        name: &str,
        input: &serde_json::Value,
        tool_timeout: Duration,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> Result<String> {
        let tool_name = name.to_string();
        self.run_hooks(HookEvent::PreTool, &tool_name, input, stream_delta_tx)
            .await?;

        let tool_result = if name == "run_command" {
            execute_run_command_tool(&self.tool_operator, input, tool_timeout).await
        } else {
            let task_name = tool_name.clone();
            let task_input = input.clone();
            let task_executor = self.tool_operator.clone();
            #[cfg(test)]
            let task_mock_responses = self.mock_tool_operator_responses.clone();

            let mut task = tokio::task::spawn_blocking(move || {
                #[cfg(test)]
                {
                    execute_tool_blocking_with_operator(
                        &task_executor,
                        &task_name,
                        &task_input,
                        task_mock_responses,
                    )
                }
                #[cfg(not(test))]
                {
                    execute_tool_blocking_with_operator(&task_executor, &task_name, &task_input)
                }
            });

            match tokio::time::timeout(tool_timeout, &mut task).await {
                Ok(join_result) => match join_result {
                    Ok(result) => result,
                    Err(join_error) => Err(anyhow::anyhow!(
                        "Tool execution task failed for {tool_name}: {join_error}"
                    )),
                },
                Err(_) => {
                    task.abort();
                    Err(anyhow::anyhow!(
                        "Tool execution timed out after {}s for {tool_name}",
                        tool_timeout.as_secs()
                    ))
                }
            }
        };

        if tool_result.is_ok() {
            self.run_hooks(HookEvent::PostTool, &tool_name, input, stream_delta_tx)
                .await?;
        }

        tool_result
    }

    async fn run_hooks(
        &self,
        event: HookEvent,
        tool_name: &str,
        input: &serde_json::Value,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> Result<()> {
        if self.hooks.is_empty() {
            return Ok(());
        }

        let primary_path = input
            .get("path")
            .or_else(|| input.get("file_path"))
            .or_else(|| input.get("file"))
            .or_else(|| input.get("filename"))
            .or_else(|| input.get("old_path"))
            .or_else(|| input.get("from"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let runner = DefaultCommandRunner::new();
        let sandbox = PassthroughSandbox;

        for hook in &self.hooks {
            if hook.event != event || hook.tool != tool_name {
                continue;
            }

            let args = hook
                .args
                .iter()
                .map(|arg| {
                    arg.replace("{{tool}}", tool_name)
                        .replace("{{path}}", primary_path)
                })
                .collect::<Vec<_>>();

            let approval_input = serde_json::json!({
                "command": hook.command,
                "args": args,
                "tool": tool_name,
            });
            let approved = if stream_delta_tx.is_some() {
                self.request_tool_approval("run_command", &approval_input, stream_delta_tx)
                    .await
            } else {
                false
            };
            if !approved {
                emit_hook_warning(format!(
                    "[hooks] warning: skipping hook '{}' for tool '{}' due to missing RunCommand approval",
                    hook.command,
                    tool_name
                ));
                continue;
            }

            let wrapped_req = sandbox.wrap(CommandRequest {
                program: hook.command.clone(),
                args,
                working_dir: None,
            })?;
            match runner.run_one_shot(wrapped_req).await {
                Ok(result) if result.exit_code == 0 => {}
                Ok(result) => {
                    let msg = format!(
                        "hook '{}' failed with exit code {}",
                        hook.command, result.exit_code
                    );
                    match hook.on_fail {
                        HookOnFail::Abort => bail!(msg),
                        HookOnFail::Warn => emit_hook_warning(format!("[hooks] warning: {msg}")),
                        HookOnFail::Ignore => {}
                    }
                }
                Err(error) => {
                    let msg = format!("hook '{}' failed to execute", hook.command);
                    match hook.on_fail {
                        HookOnFail::Abort => return Err(error).context(msg),
                        HookOnFail::Warn => {
                            emit_hook_warning(format!("[hooks] warning: {msg}: {error}"))
                        }
                        HookOnFail::Ignore => {}
                    }
                }
            }
        }

        Ok(())
    }
}

async fn execute_run_command_tool(
    tool_operator: &ToolOperator,
    input: &serde_json::Value,
    tool_timeout: Duration,
) -> Result<String> {
    let program = required_tool_string_any(
        input,
        "run_command",
        "command",
        &["command", "program", "cmd"],
    )?;
    let args: Vec<String> = input
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let request = PassthroughSandbox.wrap(CommandRequest {
        program: program.to_string(),
        args,
        working_dir: Some(tool_operator.working_dir().to_path_buf()),
    })?;
    let runner = DefaultCommandRunner::new();
    let output = tokio::time::timeout(tool_timeout, runner.run_one_shot(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Tool execution timed out after {}s for run_command",
                tool_timeout.as_secs()
            )
        })??;

    let mut result = format!("exit_code: {}\n", output.exit_code);
    if !output.stdout.is_empty() {
        result.push_str(&format!("stdout:\n{}", output.stdout));
    }
    if !output.stderr.is_empty() {
        result.push_str(&format!("stderr:\n{}", output.stderr));
    }
    Ok(result)
}

#[cfg(test)]
pub(super) fn execute_tool_blocking_with_operator(
    tool_operator: &ToolOperator,
    name: &str,
    input: &serde_json::Value,
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

    execute_tool_dispatch(tool_operator, name, input)
}

#[cfg(not(test))]
pub(super) fn execute_tool_blocking_with_operator(
    tool_operator: &ToolOperator,
    name: &str,
    input: &serde_json::Value,
) -> Result<String> {
    execute_tool_dispatch(tool_operator, name, input)
}

pub(super) fn execute_tool_dispatch(
    tool_operator: &ToolOperator,
    name: &str,
    input: &serde_json::Value,
) -> Result<String> {
    let get_str = |key: &str| input.get(key).and_then(|v| v.as_str()).unwrap_or("");
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
            tool_operator.read_file(path)
        }
        "write_file" => {
            let path =
                required_tool_string_any(input, name, "path", &["path", "file_path", "file"])?;
            let content = first_tool_string(input, &["content", "text"]).unwrap_or("");
            let (chars, lines) = text_stats(content);
            match tool_operator.write_file(path, content)? {
                WriteFileOutcome::Written => {
                    Ok(format!("Wrote {path} ({chars} chars, {lines} lines)."))
                }
                WriteFileOutcome::Pending(pending) => {
                    Ok(format!("Pending patch for {path}.\n{}", pending.diff))
                }
            }
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
            tool_operator
                .edit_file(path, old_str, new_str)
                .map(|_| summary)
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
            tool_operator.rename_file(old_path, new_path)
        }
        "list_files" | "list_directory" => tool_operator.list_files(
            input.get("path").and_then(|v| v.as_str()),
            get_usize("max_entries", 100),
        ),
        "search_files" | "search" => tool_operator.search_files(
            get_str("query"),
            input.get("path").and_then(|v| v.as_str()),
            get_usize("max_results", 30),
        ),
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
        "run_command" => bail!("run_command must execute through the runtime command runner"),
        _ => bail!("Unknown tool: {name}"),
    }
}

pub(super) fn required_tool_string<'a>(
    input: &'a serde_json::Value,
    tool: &str,
    key: &str,
) -> Result<&'a str> {
    let value = input
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if value.is_empty() {
        bail!("{tool} requires a non-empty '{key}' string argument");
    }
    Ok(value)
}

pub(super) fn first_tool_string<'a>(
    input: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(|v| v.as_str()))
}

pub(super) fn required_tool_string_any<'a>(
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

pub(super) fn required_tool_string_any_preserve<'a>(
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

pub(super) fn missing_mutating_location_prompt(
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

pub(super) fn is_read_only_user_request(input: &str) -> bool {
    const READ_ONLY_HINTS: [&str; 25] = [
        "show",
        "read",
        "list",
        "count",
        "how many",
        "read-only",
        "read only",
        "readonly",
        "review only",
        "no changes",
        "without changes",
        "do not change",
        "don't change",
        "do not modify",
        "don't modify",
        "what is in",
        "what's in",
        "whats in",
        "content of",
        "status",
        "diff",
        "log",
        "cat",
        "display",
        "print",
    ];
    const MUTATING_HINTS: [&str; 18] = [
        "write",
        "edit",
        "update",
        "create",
        "add",
        "delete",
        "remove",
        "rename",
        "move",
        "commit",
        "stage",
        "patch",
        "apply",
        "implement",
        "refactor",
        "fix",
        "push",
        "rebase",
    ];

    let normalized = input.to_ascii_lowercase();
    let has_read_only_hint = READ_ONLY_HINTS
        .iter()
        .any(|hint| contains_hint(&normalized, hint));
    let has_mutating_hint = MUTATING_HINTS
        .iter()
        .any(|hint| contains_hint(&normalized, hint));

    has_read_only_hint && !has_mutating_hint
}

fn contains_hint(normalized: &str, hint: &str) -> bool {
    normalized.match_indices(hint).any(|(start, _)| {
        let end = start + hint.len();
        let previous = normalized[..start].chars().next_back();
        let next = normalized[end..].chars().next();

        previous.is_none_or(|value| !value.is_ascii_alphanumeric())
            && next.is_none_or(|value| !value.is_ascii_alphanumeric())
    })
}

pub(super) fn mutating_tool_read_only_conflict_prompt(
    user_input: &str,
    tool_name: &str,
) -> Option<String> {
    if !tool_requires_confirmation(tool_name) || !is_read_only_user_request(user_input) {
        return None;
    }

    Some(format!(
        "Blocked mutating tool call `{tool_name}` because this request appears read-only. Use read-only tools (`read_file`, `search_files`, `list_files`, `git_status`, `git_diff`, `git_log`, `git_show`) and answer from those results. No file changes were made."
    ))
}

pub(super) fn tests_only_mutation_conflict_prompt(
    policy: TurnToolPolicy,
    tool_name: &str,
    input: &serde_json::Value,
) -> Option<String> {
    if policy != TurnToolPolicy::TestsOnlyMutations {
        return None;
    }

    let target_paths = mutating_tool_target_paths(tool_name, input);
    if target_paths.is_empty() || target_paths.iter().all(|path| is_test_target_path(path)) {
        return None;
    }

    let blocked_path = target_paths
        .into_iter()
        .find(|path| !is_test_target_path(path))
        .unwrap_or_default();
    Some(format!(
        "Dropped non-test patch target `{blocked_path}` for /generate-tests. This command may only modify test files or paths under `test/` or `tests/`. Use `/edit` for source-file changes."
    ))
}

fn mutating_tool_target_paths(tool_name: &str, input: &serde_json::Value) -> Vec<String> {
    match tool_name {
        "write_file" | "edit_file" => {
            first_tool_string(input, &["path", "file_path", "file", "filename"])
                .into_iter()
                .map(ToString::to_string)
                .collect()
        }
        "apply_patch" => first_tool_string(input, &["path", "file_path", "file", "filename"])
            .map(ToString::to_string)
            .into_iter()
            .chain(
                first_tool_string(input, &["content", "patch", "diff"])
                    .and_then(extract_apply_patch_target_path),
            )
            .collect(),
        "rename_file" => [
            first_tool_string(input, &["old_path", "from", "source_path"]),
            first_tool_string(input, &["new_path", "to", "target_path"]),
        ]
        .into_iter()
        .flatten()
        .map(ToString::to_string)
        .collect(),
        _ => Vec::new(),
    }
}

fn extract_apply_patch_target_path(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let path = line
            .strip_prefix("+++ b/")
            .or_else(|| line.strip_prefix("--- a/"))
            .or_else(|| {
                line.strip_prefix("diff --git a/")
                    .and_then(|rest| rest.split_once(" b/").map(|(_, path)| path))
            })?;
        let normalized = path.trim();
        if normalized.is_empty() || normalized == "/dev/null" {
            None
        } else {
            Some(normalized.to_string())
        }
    })
}

fn is_test_target_path(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/").to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }

    if normalized
        .split('/')
        .any(|segment| segment == "test" || segment == "tests" || segment == "__tests__")
    {
        return true;
    }

    let file_name = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    matches!(file_name, "test.rs" | "tests.rs")
        || file_name.starts_with("test_")
        || file_name.starts_with("spec_")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.ends_with("_test.rs")
        || file_name.ends_with("_tests.rs")
        || file_name.ends_with("_test.py")
        || file_name.ends_with("_test.go")
        || file_name.ends_with("_test.js")
        || file_name.ends_with("_test.jsx")
        || file_name.ends_with("_test.ts")
        || file_name.ends_with("_test.tsx")
        || file_name.ends_with("_spec.rb")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tests_only_policy_allows_rust_tests_module_paths() {
        let input = json!({
            "path": "src/state/conversation/tests.rs",
            "content": "#[test]\nfn keeps_guard_happy() {}\n",
        });

        assert_eq!(
            tests_only_mutation_conflict_prompt(
                TurnToolPolicy::TestsOnlyMutations,
                "write_file",
                &input,
            ),
            None
        );
        assert!(is_test_target_path("src/state/conversation/tests.rs"));
        assert!(is_test_target_path("src/runtime/test.rs"));
    }

    #[test]
    fn test_tests_only_policy_blocks_apply_patch_source_paths() {
        let input = json!({
            "path": "src/lib.rs",
            "content": "pub fn answer() -> i32 { 42 }\n",
        });

        let message = tests_only_mutation_conflict_prompt(
            TurnToolPolicy::TestsOnlyMutations,
            "apply_patch",
            &input,
        )
        .expect("apply_patch against src/ should be rejected");
        assert!(message.contains("src/lib.rs"));
    }

    #[test]
    fn test_tests_only_policy_parses_apply_patch_diff_headers_when_path_missing() {
        let input = json!({
            "content": "\
        diff --git a/src/lib.rs b/src/lib.rs\n\
        --- a/src/lib.rs\n\
        +++ b/src/lib.rs\n\
        @@ -1 +1 @@\n\
        -old\n\
        +new\n"
        });

        let message = tests_only_mutation_conflict_prompt(
            TurnToolPolicy::TestsOnlyMutations,
            "apply_patch",
            &input,
        )
        .expect("diff headers should still identify the blocked source file");
        assert!(message.contains("src/lib.rs"));
    }
}

pub(super) fn text_stats(text: &str) -> (usize, usize) {
    (
        text.chars().count(),
        text.lines().count().max(usize::from(!text.is_empty())),
    )
}

pub(super) fn default_tool_approval_enabled(is_local_endpoint: bool) -> bool {
    !is_local_endpoint
}

pub(super) fn tool_approval_enabled(is_local_endpoint: bool) -> bool {
    std::env::var("VEX_TOOL_CONFIRM")
        .ok()
        .and_then(parse_bool_flag)
        .unwrap_or(default_tool_approval_enabled(is_local_endpoint))
}

pub(super) fn tool_input_preview(tool_name: &str, input: &serde_json::Value) -> String {
    preview_tool_input(
        tool_name,
        input,
        ToolPreviewStyle::Compact,
        DEFAULT_EDIT_DIFF_CONTEXT_LINES,
    )
}

#[derive(Debug, Clone)]
pub(super) struct TaggedToolCall {
    pub(super) name: String,
    pub(super) input: serde_json::Value,
}

pub(super) fn parse_tagged_tool_calls(text: &str) -> Vec<TaggedToolCall> {
    let mut calls = Vec::new();
    let mut cursor = 0usize;

    while let Some(function_rel) = text[cursor..].find("<function=") {
        let function_start = cursor + function_rel;
        let name_start = function_start + "<function=".len();
        let Some(name_end_rel) = text[name_start..].find('>') else {
            break;
        };
        let name_end = name_start + name_end_rel;
        let function_name = text[name_start..name_end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        let body_start = name_end + 1;
        let (body_end, next_cursor) = find_function_body_bounds(text, body_start);
        let body = &text[body_start..body_end];

        let input = parse_tagged_parameters(body);

        if !function_name.is_empty() {
            calls.push(TaggedToolCall {
                name: function_name,
                input: serde_json::Value::Object(input),
            });
        }

        cursor = next_cursor.max(function_start + 1);
    }

    calls
}

pub(super) fn find_function_body_bounds(text: &str, body_start: usize) -> (usize, usize) {
    let function_close = text[body_start..]
        .find("</function>")
        .map(|rel| body_start + rel);
    let next_function = text[body_start..]
        .find("<function=")
        .map(|rel| body_start + rel);

    match (function_close, next_function) {
        (Some(close), Some(next)) if next < close => (next, next),
        (Some(close), _) => (close, close + "</function>".len()),
        (None, Some(next)) => (next, next),
        (None, None) => (text.len(), text.len()),
    }
}

pub(super) fn parse_tagged_parameters(body: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut input = serde_json::Map::new();
    let mut parameter_cursor = 0usize;

    while let Some(parameter_rel) = body[parameter_cursor..].find("<parameter=") {
        let parameter_start = parameter_cursor + parameter_rel;
        let key_start = parameter_start + "<parameter=".len();
        let Some(key_end_rel) = body[key_start..].find('>') else {
            break;
        };
        let key_end = key_start + key_end_rel;
        let key = body[key_start..key_end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        let value_start = key_end + 1;
        let parameter_close = body[value_start..]
            .find("</parameter>")
            .map(|rel| value_start + rel);
        let next_parameter = body[value_start..]
            .find("<parameter=")
            .map(|rel| value_start + rel);

        let (value_end, next_cursor) = match (parameter_close, next_parameter) {
            (Some(close), Some(next)) if next < close => (next, next),
            (Some(close), _) => (close, close + "</parameter>".len()),
            (None, Some(next)) => (next, next),
            (None, None) => (body.len(), body.len()),
        };

        let value = normalize_tagged_parameter_value(&body[value_start..value_end]);
        if !key.is_empty() {
            input.insert(key, serde_json::Value::String(value));
        }

        parameter_cursor = next_cursor.max(parameter_start + 1);
    }

    input
}

pub(super) fn render_tool_calls_for_text_protocol(blocks: &[ContentBlock]) -> String {
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

pub(super) fn json_value_to_text_protocol_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

pub(super) fn normalize_tagged_parameter_value(raw: &str) -> String {
    let mut value = raw.replace("\r\n", "\n");
    if value.starts_with('\n') {
        value.remove(0);
    }
    if value.ends_with('\n') {
        value.pop();
    }
    value
}

pub(super) fn render_loop_limit_guard_message(
    last_assistant_text: &str,
    max_rounds: usize,
) -> String {
    render_loop_guard_message(
        last_assistant_text,
        format!("Stopped after {max_rounds} tool rounds to prevent an infinite loop."),
    )
}

pub(super) fn render_repeated_tool_guard_message(last_assistant_text: &str) -> String {
    render_loop_guard_message(
        last_assistant_text,
        "Repeated identical read/search tool calls detected; stopped to prevent an infinite loop."
            .to_string(),
    )
}

pub(super) fn render_repeated_mutating_tool_guard_message(last_assistant_text: &str) -> String {
    render_loop_guard_message(
        last_assistant_text,
        "Repeated identical mutating tool calls detected; stopped to prevent an infinite loop. Verify edit_file arguments include path, old_str, and new_str.".to_string(),
    )
}

pub(super) fn render_tool_denied_message(tool_name: &str) -> String {
    if tool_requires_confirmation(tool_name) {
        format!("Stopped: approval denied for {tool_name}. No file changes were made.")
    } else {
        format!("Stopped: approval denied for {tool_name}. No tool actions were performed.")
    }
}

pub(super) fn render_missing_tool_evidence_guard_message(last_assistant_text: &str) -> String {
    render_loop_guard_message(
        last_assistant_text,
        "Model did not execute any tool call required to answer this request with workspace evidence."
            .to_string(),
    )
}

pub(super) fn render_loop_guard_message(last_assistant_text: &str, reason: String) -> String {
    let summary = if last_assistant_text.trim().is_empty() {
        "No final assistant answer was produced.".to_string()
    } else {
        last_assistant_text.to_string()
    };
    format!("{summary}\n\n[loop guard] {reason}")
}

pub(super) fn is_read_only_tool_name(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "search"
            | "search_files"
            | "list_files"
            | "list_directory"
            | "git_status"
            | "git_diff"
            | "git_log"
            | "git_show"
    )
}

pub(super) fn is_read_only_tool_round(blocks: &[ContentBlock]) -> bool {
    blocks.iter().all(|block| {
        matches!(
            block,
            ContentBlock::ToolUse { name, .. } if is_read_only_tool_name(name)
        )
    })
}

pub(super) fn is_mutating_tool_round(blocks: &[ContentBlock]) -> bool {
    blocks.iter().any(|block| {
        matches!(
            block,
            ContentBlock::ToolUse { name, .. } if tool_requires_confirmation(name)
        )
    })
}

pub(super) fn tool_requires_confirmation(name: &str) -> bool {
    matches!(
        name,
        "write_file"
            | "apply_patch"
            | "edit_file"
            | "rename_file"
            | "git_add"
            | "git_commit"
            | "run_command"
    )
}

pub(super) fn tool_round_signature(blocks: &[ContentBlock]) -> Vec<String> {
    let mut signature = Vec::new();
    for block in blocks {
        if let ContentBlock::ToolUse { name, input, .. } = block {
            let payload = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
            signature.push(format!("{name}:{payload}"));
        }
    }
    signature
}

pub(super) fn builtin_supported_git_tools_response(input: &str) -> Option<String> {
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
