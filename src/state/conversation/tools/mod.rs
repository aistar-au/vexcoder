use super::{ConversationManager, ConversationStreamUpdate, ToolApprovalRequest, TurnToolPolicy};
use crate::config::{HookEvent, HookOnFail, SearchConfig};
use crate::mcp::McpRegistry;
use crate::runtime::{
    format_command_session_cancelled, format_command_session_exit, format_command_session_output,
    format_command_session_started, CommandRequest, CommandRunner, ConfiguredSandbox,
    DefaultCommandRunner, SandboxDriver,
};
use crate::tools::embed::EmbeddingConfig;
use crate::tools::search;
use crate::tools::semantic;
use crate::tools::ToolOperator;
use anyhow::{bail, Context, Result};
use reqwest;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(all(test, not(windows)))]
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

// Imports from extracted submodules
use self::config::{append_capped, max_command_output_bytes};
use self::index::{build_codebase_index, refresh_codebase_index, CODEBASE_INDEX};

#[cfg(all(test, not(windows)))]
static HOOK_WARNINGS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static RUN_COMMAND_SESSION_IDS: AtomicU64 = AtomicU64::new(1 << 63);

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
    #[cfg(test)]
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
        self.run_http_hooks(HookEvent::PreTool, &tool_name, input, tool_timeout)
            .await?;

        let tool_result = if name == "run_command" {
            execute_run_command_tool(
                &self.tool_operator,
                &self.sandbox,
                input,
                tool_timeout,
                stream_delta_tx,
            )
            .await
        } else if name == "codebase_search" {
            execute_codebase_search_tool(&self.tool_operator, &self.search_config, input).await
        } else if name.starts_with("mcp.") {
            execute_mcp_tool(self.mcp_registry.as_ref(), name, input, tool_timeout).await
        } else {
            let task_name = tool_name.clone();
            let task_input = input.clone();
            let task_executor = self.tool_operator.clone();
            let task_search_config = self.search_config.clone();
            #[cfg(test)]
            let task_mock_responses = self.mock_tool_operator_responses.clone();

            let mut task = tokio::task::spawn_blocking(move || {
                #[cfg(test)]
                {
                    execute_tool_blocking_with_operator(
                        &task_executor,
                        &task_name,
                        &task_input,
                        &task_search_config,
                        task_mock_responses,
                    )
                }
                #[cfg(not(test))]
                {
                    execute_tool_blocking_with_operator(
                        &task_executor,
                        &task_name,
                        &task_input,
                        &task_search_config,
                    )
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
            self.run_http_hooks(HookEvent::PostTool, &tool_name, input, tool_timeout)
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
        let sandbox = self.sandbox.clone();

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

    async fn run_http_hooks(
        &self,
        event: HookEvent,
        tool_name: &str,
        input: &serde_json::Value,
        tool_timeout: Duration,
    ) -> Result<()> {
        if self.http_hooks.is_empty() {
            return Ok(());
        }

        let primary_path = input
            .get("path")
            .or_else(|| input.get("file_path"))
            .or_else(|| input.get("file"))
            .or_else(|| input.get("filename"))
            .or_else(|| input.get("old_path"))
            .or_else(|| input.get("from"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let timestamp_ms = crate::runtime::session_task::now_millis();
        let hook_timeout = tool_timeout.min(Duration::from_secs(5));
        let client = reqwest::Client::builder().timeout(hook_timeout).build()?;

        for hook in &self.http_hooks {
            if hook.event != event || hook.tool != tool_name {
                continue;
            }

            let payload = serde_json::json!({
                "event": hook.event,
                "tool": tool_name,
                "path": primary_path,
                "timestamp_ms": timestamp_ms,
            });

            match client.post(&hook.url).json(&payload).send().await {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    let msg = format!(
                        "http_hook '{}' responded with status {}",
                        hook.url,
                        resp.status()
                    );
                    match hook.on_fail {
                        HookOnFail::Abort => bail!(msg),
                        HookOnFail::Warn => {
                            emit_hook_warning(format!("[http_hooks] warning: {msg}"))
                        }
                        HookOnFail::Ignore => {}
                    }
                }
                Err(e) => {
                    let msg = if e.is_timeout() {
                        format!(
                            "http_hook '{}' timed out after {}ms",
                            hook.url,
                            hook_timeout.as_millis()
                        )
                    } else {
                        format!("http_hook '{}' failed to send: {e}", hook.url)
                    };
                    match hook.on_fail {
                        HookOnFail::Abort => bail!(msg),
                        HookOnFail::Warn => {
                            emit_hook_warning(format!("[http_hooks] warning: {msg}"))
                        }
                        HookOnFail::Ignore => {}
                    }
                }
            }
        }

        Ok(())
    }
}

async fn execute_codebase_search_tool(
    tool_operator: &ToolOperator,
    search_config: &SearchConfig,
    input: &serde_json::Value,
) -> Result<String> {
    if !search_config.enabled {
        bail!("codebase_search is disabled by [search].enabled=false");
    }

    let query = required_tool_string(input, "codebase_search", "query")?;
    let max_results = input
        .get("max_results")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize);
    let idx_mutex = CODEBASE_INDEX.get_or_init(|| {
        let chunks = build_codebase_index(tool_operator.working_dir(), search_config);
        Mutex::new(chunks)
    });
    let idx = idx_mutex
        .lock()
        .map_err(|_| anyhow::anyhow!("codebase index lock poisoned"))?
        .clone();

    let structural_results = search::codebase_search(query, &idx, max_results);
    let merged_results = match EmbeddingConfig::from_env()? {
        Some(config) => {
            let semantic_scores = semantic::semantic_search(
                tool_operator.working_dir(),
                &idx,
                query,
                &config,
                max_results,
            )
            .await?;
            search::merge_search_results(&idx, structural_results, semantic_scores, max_results)
        }
        None => structural_results,
    };

    Ok(search::format_search_results(query, &merged_results))
}

async fn execute_mcp_tool(
    registry: Option<&std::sync::Arc<McpRegistry>>,
    name: &str,
    input: &serde_json::Value,
    tool_timeout: Duration,
) -> Result<String> {
    let registry = registry.ok_or_else(|| anyhow::anyhow!("MCP registry not loaded"))?;
    match tokio::time::timeout(tool_timeout, registry.call_tool(name, input)).await {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!(
            "Tool execution timed out after {}s for {name}",
            tool_timeout.as_secs()
        )),
    }
}

async fn execute_run_command_tool(
    tool_operator: &ToolOperator,
    sandbox: &ConfiguredSandbox,
    input: &serde_json::Value,
    tool_timeout: Duration,
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
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
    let command = render_command_session_command(program, &args);
    let session_id = RUN_COMMAND_SESSION_IDS.fetch_add(1, Ordering::Relaxed);

    if let Some(tx) = stream_delta_tx {
        let _ = tx.send(ConversationStreamUpdate::CommandSessionStarted {
            session_id,
            command: command.clone(),
        });
    }

    let request = sandbox.wrap(CommandRequest {
        program: program.to_string(),
        args,
        working_dir: Some(tool_operator.working_dir().to_path_buf()),
    })?;
    let runner = DefaultCommandRunner::new();
    let (output_tx, mut output_rx) = mpsc::channel(128);
    let mut handle = match runner.run_streaming(request, output_tx).await {
        Ok(handle) => handle,
        Err(error) => {
            if let Some(tx) = stream_delta_tx {
                let _ = tx.send(ConversationStreamUpdate::TranscriptLine(format!(
                    "[command session] error: {error}"
                )));
                let _ = tx.send(ConversationStreamUpdate::CommandSessionFinished { session_id });
            }
            return Err(error);
        }
    };

    if let Some(tx) = stream_delta_tx {
        let _ = tx.send(ConversationStreamUpdate::CommandSessionAttached {
            session_id,
            pid: handle.pid(),
        });
        let _ = tx.send(ConversationStreamUpdate::TranscriptLine(
            format_command_session_started(&command, handle.pid()),
        ));
    }

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut stdout_total: usize = 0;
    let mut stderr_total: usize = 0;
    let cap = max_command_output_bytes();
    let mut timed_out = false;
    let sleep = tokio::time::sleep(tool_timeout);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            _ = &mut sleep, if !timed_out => {
                timed_out = true;
                let _ = handle.cancel();
            }
            chunk = output_rx.recv() => {
                match chunk {
                    Some(chunk) => {
                        match &chunk.stream {
                            crate::runtime::StreamKind::Stdout => {
                                stdout_total += chunk.text.len();
                                append_capped(&mut stdout, &chunk.text, cap);
                            }
                            crate::runtime::StreamKind::Stderr => {
                                stderr_total += chunk.text.len();
                                append_capped(&mut stderr, &chunk.text, cap);
                            }
                        }
                        if let Some(tx) = stream_delta_tx {
                            for line in format_command_session_output(chunk) {
                                let _ = tx.send(ConversationStreamUpdate::TranscriptLine(line));
                            }
                        }
                    }
                    None => break,
                }
            }
        }
    }

    let wait_result = handle.wait().await;
    if let Some(tx) = stream_delta_tx {
        match &wait_result {
            Ok(result) if timed_out => {
                let _ = tx.send(ConversationStreamUpdate::TranscriptLine(format!(
                    "[command session] error: timed out after {}s",
                    tool_timeout.as_secs()
                )));
                let _ = tx.send(ConversationStreamUpdate::TranscriptLine(
                    format_command_session_cancelled(),
                ));
                let _ = tx.send(ConversationStreamUpdate::TranscriptLine(
                    format_command_session_exit(result.exit_code),
                ));
            }
            Ok(result) => {
                let _ = tx.send(ConversationStreamUpdate::TranscriptLine(
                    format_command_session_exit(result.exit_code),
                ));
            }
            Err(error) => {
                let _ = tx.send(ConversationStreamUpdate::TranscriptLine(format!(
                    "[command session] error: {error}"
                )));
            }
        }
        let _ = tx.send(ConversationStreamUpdate::CommandSessionFinished { session_id });
    }

    if timed_out {
        return Err(anyhow::anyhow!(
            "Tool execution timed out after {}s for run_command",
            tool_timeout.as_secs()
        ));
    }

    let output = wait_result?;
    let mut result = format!("exit_code: {}\n", output.exit_code);
    if !stdout.is_empty() {
        if stdout_total > cap {
            result.push_str(&format!(
                "stdout (last {} of {} bytes):\n{}",
                cap, stdout_total, stdout
            ));
        } else {
            result.push_str(&format!("stdout:\n{stdout}"));
        }
    }
    if !stderr.is_empty() {
        if stderr_total > cap {
            result.push_str(&format!(
                "stderr (last {} of {} bytes):\n{}",
                cap, stderr_total, stderr
            ));
        } else {
            result.push_str(&format!("stderr:\n{stderr}"));
        }
    }
    Ok(result)
}

fn render_command_session_command(program: &str, args: &[String]) -> String {
    let mut command = program.to_string();
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    command
}

// Submodules extracted from this file
pub(crate) mod config;
pub(crate) mod dispatch;
pub(crate) mod formatting;
pub(crate) mod index;
#[cfg(test)]
mod tests;
pub(crate) mod validation;

// Re-export all submodule items so `use super::tools::*` works for sibling modules
pub(crate) use self::dispatch::*;
pub(crate) use self::formatting::*;
pub(crate) use self::index::*;
pub(crate) use self::validation::*;
