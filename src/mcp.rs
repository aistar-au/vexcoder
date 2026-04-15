use anyhow::{Context, Result, anyhow, bail};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::config::{McpServerConfig, McpTransport};
use crate::http_facade::{HeaderName, HeaderValue};
use crate::runtime::tokio::{
    process::Command as TokioCommand,
    runtime::{Builder as RuntimeBuilder, Handle, Runtime},
    sync::Mutex,
    time::timeout,
};

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
pub struct McpServerRollup {
    pub name: String,
    pub transport: String,
    pub tools: Vec<McpToolSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpRegistryRollup {
    pub servers: Vec<McpServerRollup>,
}

impl McpRegistryRollup {
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
    rollup: McpRegistryRollup,
    tool_definitions: Vec<Value>,
    servers: Arc<Vec<McpConnectedServer>>,
    tool_lookup: Arc<HashMap<String, (usize, String)>>,
}

struct McpConnectedServer {
    runtime: Mutex<Option<RunningService<RoleClient, ()>>>,
}

impl McpConnectedServer {
    async fn shutdown(&self) {
        let service = self.runtime.lock().await.take();
        if let Some(service) = service {
            // The cancel path may fail if the underlying STDIO process has
            // already exited (crash, signal, etc.).  A 5-second grace period
            // prevents a hung server from blocking session teardown.
            let _ = timeout(Duration::from_secs(5), service.cancel()).await;
        }
    }

    async fn shutdown_owned(self) {
        if let Some(service) = self.runtime.into_inner() {
            let _ = timeout(Duration::from_secs(5), service.cancel()).await;
        }
    }
}

impl McpRegistry {
    pub async fn connect_all(configs: &[McpServerConfig]) -> Result<Option<Arc<Self>>> {
        if configs.is_empty() {
            return Ok(None);
        }

        let mut rollups = Vec::new();
        let mut servers: Vec<McpConnectedServer> = Vec::new();
        let mut tool_definitions = Vec::new();
        let mut tool_lookup = HashMap::new();

        for (server_index, config) in configs.iter().enumerate() {
            let connect_timeout = resolve_mcp_timeout(config.timeout_secs);
            let connect_result = timeout(connect_timeout, connect_server(config))
                .await
                .map_err(|_| {
                    anyhow!(
                        "MCP server '{}' connection timed out after {}s",
                        config.name,
                        connect_timeout.as_secs()
                    )
                })
                .and_then(|inner| {
                    inner.with_context(|| format!("failed to connect MCP server '{}'", config.name))
                });

            let runtime = match connect_result {
                Ok(rt) => rt,
                Err(error) => {
                    // Explicitly cancel already-connected servers before propagating.
                    shutdown_connected_servers(std::mem::take(&mut servers)).await;
                    return Err(error);
                }
            };

            let tools = match runtime.peer().list_all_tools().await {
                Ok(t) => t,
                Err(error) => {
                    let _ = runtime.cancel().await;
                    shutdown_connected_servers(std::mem::take(&mut servers)).await;
                    return Err(error).with_context(|| {
                        format!("failed to list tools for MCP server '{}'", config.name)
                    });
                }
            };

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
                tool_definitions.push(crate::util::tool_definition_entry(
                    &full_name,
                    &description,
                    schema,
                ));
                server_tools.push(McpToolSummary {
                    full_name: full_name.clone(),
                    short_name: short_name.clone(),
                    description: description.clone(),
                });
                tool_lookup.insert(full_name, (server_index, short_name));
            }
            server_tools.sort_by(|left, right| left.full_name.cmp(&right.full_name));
            rollups.push(McpServerRollup {
                name: config.name.clone(),
                transport: config.transport.as_str().to_string(),
                tools: server_tools,
            });
            servers.push(McpConnectedServer {
                runtime: Mutex::new(Some(runtime)),
            });
        }

        rollups.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(Some(Arc::new(Self {
            rollup: McpRegistryRollup { servers: rollups },
            tool_definitions,
            servers: Arc::new(servers),
            tool_lookup: Arc::new(tool_lookup),
        })))
    }

    pub fn connect_all_blocking(configs: &[McpServerConfig]) -> Result<Option<Arc<Self>>> {
        if configs.is_empty() {
            return Ok(None);
        }

        if Handle::try_current().is_ok() {
            let configs = configs.to_vec();
            return std::thread::spawn(move || -> Result<Option<Arc<Self>>> {
                let runtime = RuntimeBuilder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("failed to create Tokio runtime for MCP startup")?;
                runtime.block_on(Self::connect_all(&configs))
            })
            .join()
            .map_err(|_| anyhow!("MCP startup thread panicked"))?;
        }

        let runtime = Runtime::new().context("failed to create Tokio runtime for MCP startup")?;
        runtime.block_on(Self::connect_all(configs))
    }

    pub fn rollup(&self) -> McpRegistryRollup {
        self.rollup.clone()
    }

    pub fn tool_definitions(&self) -> Vec<Value> {
        self.tool_definitions.clone()
    }

    pub async fn shutdown(&self) {
        for server in self.servers.iter() {
            server.shutdown().await;
        }
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
        let mut runtime = server.runtime.lock().await;
        let runtime = runtime
            .as_mut()
            .ok_or_else(|| anyhow!("MCP server for '{full_name}' is shutting down"))?;
        let params = if arguments.is_empty() {
            CallToolRequestParams::new(short_name)
        } else {
            CallToolRequestParams::new(short_name).with_arguments(arguments)
        };
        let result = match timeout(Duration::from_secs(300), runtime.peer().call_tool(params)).await
        {
            Ok(Ok(r)) => r,
            Ok(Err(err)) => {
                bail!("MCP tool '{full_name}' failed (server process may have exited): {err:#}");
            }
            Err(_) => {
                bail!("MCP tool '{full_name}' timed out after 300s");
            }
        };
        format_tool_result(full_name, &result)
    }
}

impl Drop for McpRegistry {
    fn drop(&mut self) {
        if self.servers.is_empty() {
            return;
        }

        spawn_shutdown_thread(Arc::clone(&self.servers));
    }
}

async fn shutdown_connected_servers(servers: Vec<McpConnectedServer>) {
    for server in servers {
        server.shutdown_owned().await;
    }
}

fn spawn_shutdown_thread(servers: Arc<Vec<McpConnectedServer>>) {
    let _ = std::thread::Builder::new()
        .name("vex-mcp-shutdown".to_string())
        .spawn(move || {
            let Ok(runtime) = RuntimeBuilder::new_current_thread().enable_all().build() else {
                return;
            };
            runtime.block_on(async move {
                for server in servers.iter() {
                    server.shutdown().await;
                }
            });
        });
}

async fn connect_server(config: &McpServerConfig) -> Result<RunningService<RoleClient, ()>> {
    match config.transport {
        McpTransport::Stdio => {
            let command = config
                .command
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("stdio MCP server '{}' requires 'command'", config.name))?;
            let transport = TokioChildProcess::new(TokioCommand::new(command).configure(|cmd| {
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
                let header_name = HeaderName::from_bytes(name.as_bytes()).with_context(|| {
                    format!(
                        "invalid header name '{}' for MCP server '{}'",
                        name, config.name
                    )
                })?;
                let header_value = HeaderValue::from_str(value).with_context(|| {
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
    let mut rendered = String::with_capacity(trimmed.len());
    let mut rest = trimmed;

    while let Some(start) = rest.find("${") {
        rendered.push_str(&rest[..start]);
        let suffix = &rest[start + 2..];
        let end = suffix.find('}').ok_or_else(|| {
            anyhow!(
                "unterminated environment variable reference in MCP headers: '{}'",
                trimmed
            )
        })?;
        let name = &suffix[..end];
        if name.is_empty() {
            bail!("empty environment variable reference in MCP headers");
        }
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
        rendered.push_str(&resolved);
        rest = &suffix[end + 1..];
    }
    rendered.push_str(rest);
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MCP_TIMEOUT_SECS, McpRegistry, resolve_mcp_header_env, resolve_mcp_timeout,
    };
    use crate::config::{McpServerConfig, McpTransport};
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn test_resolve_mcp_header_env_expands_reference() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        crate::test_support::test_set_var(&_lock, "VEX_MCP_TOKEN", "secret-token");
        assert_eq!(
            resolve_mcp_header_env("${VEX_MCP_TOKEN}").unwrap(),
            "secret-token"
        );
        crate::test_support::test_remove_var(&_lock, "VEX_MCP_TOKEN");
    }

    #[test]
    fn test_resolve_mcp_header_env_expands_templated_reference() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        crate::test_support::test_set_var(&_lock, "VEX_MCP_TOKEN", "secret-token");
        assert_eq!(
            resolve_mcp_header_env("Bearer ${VEX_MCP_TOKEN}").unwrap(),
            "Bearer secret-token"
        );
        crate::test_support::test_remove_var(&_lock, "VEX_MCP_TOKEN");
    }

    #[test]
    fn test_resolve_mcp_timeout_defaults() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        crate::test_support::test_remove_var(&_lock, "VEX_MCP_TIMEOUT");
        assert_eq!(
            resolve_mcp_timeout(None),
            Duration::from_secs(DEFAULT_MCP_TIMEOUT_SECS),
        );
    }

    #[test]
    fn test_resolve_mcp_timeout_per_server_wins() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        crate::test_support::test_set_var(&_lock, "VEX_MCP_TIMEOUT", "60");
        assert_eq!(resolve_mcp_timeout(Some(10)), Duration::from_secs(10));
        crate::test_support::test_remove_var(&_lock, "VEX_MCP_TIMEOUT");
    }

    #[test]
    fn test_resolve_mcp_timeout_env_fallback() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        crate::test_support::test_set_var(&_lock, "VEX_MCP_TIMEOUT", "45");
        assert_eq!(resolve_mcp_timeout(None), Duration::from_secs(45));
        crate::test_support::test_remove_var(&_lock, "VEX_MCP_TIMEOUT");
    }

    #[test]
    fn test_resolve_mcp_timeout_clamps_to_range() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        crate::test_support::test_remove_var(&_lock, "VEX_MCP_TIMEOUT");
        assert_eq!(resolve_mcp_timeout(Some(0)), Duration::from_secs(1));
        assert_eq!(resolve_mcp_timeout(Some(999)), Duration::from_secs(300));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_connect_all_blocking_avoids_current_thread_runtime_panic() {
        let configs = vec![McpServerConfig {
            name: "bad".to_string(),
            transport: McpTransport::Stdio,
            command: None,
            args: Vec::new(),
            url: None,
            headers: BTreeMap::new(),
            timeout_secs: Some(1),
        }];

        assert!(McpRegistry::connect_all_blocking(&configs).is_err());
    }
}
