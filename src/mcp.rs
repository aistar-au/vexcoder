use anyhow::{anyhow, bail, Context, Result};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::config::{McpServerConfig, McpTransport};

/// Default MCP server connection timeout in seconds.
const DEFAULT_MCP_TIMEOUT_SECS: u64 = 30;

/// Resolve the effective connection timeout for one MCP server.
fn resolve_mcp_timeout(per_server: Option<u64>) -> Duration {
    let secs = per_server
        .or_else(|| {
            std::env::var("VEX_MCP_TIMEOUT")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
        })
        .unwrap_or(DEFAULT_MCP_TIMEOUT_SECS)
        .clamp(1, 300);
    Duration::from_secs(secs)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolSummary {
    pub full_name: String,
    pub short_name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerSnapshot {
    pub name: String,
    pub transport: String,
    pub tools: Vec<McpToolSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpRegistrySnapshot {
    pub servers: Vec<McpServerSnapshot>,
}

impl McpRegistrySnapshot {
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub fn all_tools(&self) -> Vec<McpToolSummary> {
        let mut tools = self
            .servers
            .iter()
            .flat_map(|server| server.tools.iter().cloned())
            .collect::<Vec<_>>();
        tools.sort_by(|left, right| left.full_name.cmp(&right.full_name));
        tools
    }
}

#[derive(Clone)]
pub struct McpRegistry {
    snapshot: McpRegistrySnapshot,
    tool_definitions: Vec<Value>,
    servers: Arc<Vec<McpConnectedServer>>,
    tool_lookup: Arc<HashMap<String, (usize, String)>>,
}

struct McpConnectedServer {
    runtime: tokio::sync::Mutex<RunningService<RoleClient, ()>>,
}

impl McpRegistry {
    pub async fn connect_all(configs: &[McpServerConfig]) -> Result<Option<Arc<Self>>> {
        if configs.is_empty() {
            return Ok(None);
        }

        let mut snapshots = Vec::new();
        let mut servers = Vec::new();
        let mut tool_definitions = Vec::new();
        let mut tool_lookup = HashMap::new();

        for (server_index, config) in configs.iter().enumerate() {
            let timeout = resolve_mcp_timeout(config.timeout_secs);
            let runtime = tokio::time::timeout(timeout, connect_server(config))
                .await
                .map_err(|_| {
                    anyhow!(
                        "MCP server '{}' connection timed out after {}s",
                        config.name,
                        timeout.as_secs()
                    )
                })?
                .with_context(|| format!("failed to connect MCP server '{}'", config.name))?;
            let tools = runtime.peer().list_all_tools().await.with_context(|| {
                format!("failed to list tools for MCP server '{}'", config.name)
            })?;

            let mut server_tools = Vec::new();
            for tool in tools {
                let short_name = tool.name.to_string();
                let full_name = format!("mcp.{}.{}", config.name, short_name);
                let description = tool
                    .description
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| {
                        format!("MCP tool '{}' from server '{}'", short_name, config.name)
                    });
                let schema = Value::Object((*tool.input_schema).clone());
                tool_definitions.push(json!({
                    "name": full_name,
                    "description": description,
                    "input_schema": schema,
                }));
                server_tools.push(McpToolSummary {
                    full_name: full_name.clone(),
                    short_name: short_name.clone(),
                    description: description.clone(),
                });
                tool_lookup.insert(full_name, (server_index, short_name));
            }
            server_tools.sort_by(|left, right| left.full_name.cmp(&right.full_name));
            snapshots.push(McpServerSnapshot {
                name: config.name.clone(),
                transport: config.transport.as_str().to_string(),
                tools: server_tools,
            });
            servers.push(McpConnectedServer {
                runtime: tokio::sync::Mutex::new(runtime),
            });
        }

        snapshots.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(Some(Arc::new(Self {
            snapshot: McpRegistrySnapshot { servers: snapshots },
            tool_definitions,
            servers: Arc::new(servers),
            tool_lookup: Arc::new(tool_lookup),
        })))
    }

    pub fn connect_all_blocking(configs: &[McpServerConfig]) -> Result<Option<Arc<Self>>> {
        if configs.is_empty() {
            return Ok(None);
        }

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            return tokio::task::block_in_place(|| handle.block_on(Self::connect_all(configs)));
        }

        let runtime = tokio::runtime::Runtime::new()
            .context("failed to create Tokio runtime for MCP startup")?;
        runtime.block_on(Self::connect_all(configs))
    }

    pub fn snapshot(&self) -> McpRegistrySnapshot {
        self.snapshot.clone()
    }

    pub fn tool_definitions(&self) -> Vec<Value> {
        self.tool_definitions.clone()
    }

    pub async fn call_tool(&self, full_name: &str, input: &Value) -> Result<String> {
        let (server_index, short_name) = self
            .tool_lookup
            .get(full_name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown MCP tool: {full_name}"))?;
        let arguments = match input {
            Value::Object(map) => map.clone(),
            Value::Null => Map::new(),
            _ => bail!("MCP tool input for '{full_name}' must be a JSON object"),
        };

        let server = self
            .servers
            .get(server_index)
            .ok_or_else(|| anyhow!("missing MCP server index for '{full_name}'"))?;
        let runtime = server.runtime.lock().await;
        let params = if arguments.is_empty() {
            CallToolRequestParams::new(short_name)
        } else {
            CallToolRequestParams::new(short_name).with_arguments(arguments)
        };
        let result = runtime
            .peer()
            .call_tool(params)
            .await
            .with_context(|| format!("failed to execute MCP tool '{full_name}'"))?;
        format_tool_result(full_name, &result)
    }
}

async fn connect_server(config: &McpServerConfig) -> Result<RunningService<RoleClient, ()>> {
    match config.transport {
        McpTransport::Stdio => {
            let command = config
                .command
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("stdio MCP server '{}' requires 'command'", config.name))?;
            let transport =
                TokioChildProcess::new(tokio::process::Command::new(command).configure(|cmd| {
                    cmd.args(&config.args);
                }))
                .with_context(|| format!("failed to spawn MCP stdio server '{}'", config.name))?;
            ().serve(transport)
                .await
                .with_context(|| format!("failed to initialize MCP stdio server '{}'", config.name))
        }
        McpTransport::Http => {
            let url = config
                .url
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("http MCP server '{}' requires 'url'", config.name))?;
            let mut headers = HashMap::new();
            for (name, value) in &config.headers {
                let header_name =
                    http::HeaderName::from_bytes(name.as_bytes()).with_context(|| {
                        format!(
                            "invalid header name '{}' for MCP server '{}'",
                            name, config.name
                        )
                    })?;
                let header_value = http::HeaderValue::from_str(value).with_context(|| {
                    format!(
                        "invalid header value for '{}' on MCP server '{}'",
                        name, config.name
                    )
                })?;
                headers.insert(header_name, header_value);
            }
            let transport = StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(url.clone()).custom_headers(headers),
            );
            ().serve(transport)
                .await
                .with_context(|| format!("failed to initialize MCP HTTP server '{}'", config.name))
        }
    }
}

fn format_tool_result(full_name: &str, result: &CallToolResult) -> Result<String> {
    let mut parts = Vec::new();
    if let Some(structured) = &result.structured_content {
        parts.push(
            serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string()),
        );
    }
    for content in &result.content {
        if let Some(text) = content.as_text() {
            parts.push(text.text.clone());
            continue;
        }
        if let Some(resource) = content.as_resource() {
            let resource_text = match &resource.resource {
                rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.clone(),
                other => serde_json::to_string(other)
                    .unwrap_or_else(|_| "[resource content omitted]".to_string()),
            };
            parts.push(resource_text);
            continue;
        }
        parts.push(
            serde_json::to_string(content)
                .unwrap_or_else(|_| "[non-text MCP content omitted]".to_string()),
        );
    }
    let rendered = if parts.is_empty() {
        format!("MCP tool '{}' completed without textual output.", full_name)
    } else {
        parts.join("\n\n")
    };
    if result.is_error.unwrap_or(false) {
        bail!("MCP tool '{}' returned an error:\n{}", full_name, rendered);
    }
    Ok(rendered)
}

pub fn resolve_mcp_header_env(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if let Some(name) = trimmed
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
    {
        let resolved = std::env::var(name).with_context(|| {
            format!(
                "missing environment variable '{}' referenced in MCP headers",
                name
            )
        })?;
        let resolved = resolved.trim().to_string();
        if resolved.is_empty() {
            bail!(
                "environment variable '{}' referenced in MCP headers resolved to an empty value",
                name
            );
        }
        return Ok(resolved);
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{resolve_mcp_header_env, resolve_mcp_timeout, DEFAULT_MCP_TIMEOUT_SECS};
    use std::time::Duration;

    #[test]
    fn test_resolve_mcp_header_env_expands_reference() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MCP_TOKEN", "secret-token");
        assert_eq!(
            resolve_mcp_header_env("${VEX_MCP_TOKEN}").unwrap(),
            "secret-token"
        );
        std::env::remove_var("VEX_MCP_TOKEN");
    }

    #[test]
    fn test_resolve_mcp_timeout_defaults() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::remove_var("VEX_MCP_TIMEOUT");
        assert_eq!(
            resolve_mcp_timeout(None),
            Duration::from_secs(DEFAULT_MCP_TIMEOUT_SECS),
        );
    }

    #[test]
    fn test_resolve_mcp_timeout_per_server_wins() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MCP_TIMEOUT", "60");
        assert_eq!(resolve_mcp_timeout(Some(10)), Duration::from_secs(10));
        std::env::remove_var("VEX_MCP_TIMEOUT");
    }

    #[test]
    fn test_resolve_mcp_timeout_env_fallback() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MCP_TIMEOUT", "45");
        assert_eq!(resolve_mcp_timeout(None), Duration::from_secs(45));
        std::env::remove_var("VEX_MCP_TIMEOUT");
    }

    #[test]
    fn test_resolve_mcp_timeout_clamps_to_range() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::remove_var("VEX_MCP_TIMEOUT");
        assert_eq!(resolve_mcp_timeout(Some(0)), Duration::from_secs(1));
        assert_eq!(resolve_mcp_timeout(Some(999)), Duration::from_secs(300));
    }
}
