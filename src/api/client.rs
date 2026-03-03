use super::logging::{debug_payload_enabled, emit_debug_payload};
use crate::config::Config;
use crate::runtime::backend::{
    ByteStream, ModelBackend, ModelBackendKind, ModelProtocol, ToolCallMode,
};
use crate::types::{ApiMessage, Content, ContentBlock};
use crate::util::{is_local_endpoint_url, parse_bool_flag};
use anyhow::anyhow;
use anyhow::Result;
use futures::StreamExt;
use serde_json::json;
use serde_json::Value;
#[cfg(test)]
use std::sync::Arc;
const SYSTEM_PROMPT: &str = "You are a coding assistant.\n\
Use tools for all filesystem facts and changes.\n\
When a user asks for repository facts, command output, file content, or code edits, call tools instead of guessing.\n\
After each tool_result, reassess the task and either call the next needed tool or provide the final answer.\n\
Repeat this loop until the task is complete; do not stop early after a single tool result when more evidence is required.\n\
For requests that mention specific files/paths or code edits, do not answer with planning text; emit a tool call first.\n\
If native tool calls are unavailable, emit tagged tool syntax exactly:\n\
<function=tool_name>\n\
<parameter=arg>value</parameter>\n\
</function>\n\
Never claim a file was read/written/renamed/searched unless the corresponding tool call succeeded.\n\
Do not narrate intended actions without executing the tool call.\n\
Prefer search_files for targeted string matches and avoid full-file reads unless required.\n\
Use list_files/search_files/read_file before saying a file is missing or present.\n\
For edit_file, use a focused old_str snippet around the target change and avoid whole-file replacements; if an entire file rewrite is needed, use write_file instead.\n\
For code edits, prefer this sequence: search_files -> read_file -> edit_file -> read_file (verify).\n\
For read-only requests (show/read/list/count/status/log/diff), use read-only tools and do not call mutating tools unless the user explicitly asks for changes.\n\
If asked what git tools are available, only list built-in git tools: git_status, git_diff, git_log, git_show, git_add, git_commit.\n\
Do not claim unsupported git tools like git_clone, git_init, git_remote, git_config, git_pull, git_push, git_branch, git_checkout, or git_stash.\n\
Always send non-empty string paths for file tools.\n\
Avoid redundant loops: do not repeat identical read/search tool calls without new evidence.";

#[cfg(test)]
pub trait MockStreamProducer: Send + Sync {
    fn create_mock_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream>;
}

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    api_key: Option<String>,
    model: String,
    api_url: String,
    model_backend: ModelBackendKind,
    model_protocol: ModelProtocol,
    tool_call_mode: ToolCallMode,
    model_headers: reqwest::header::HeaderMap,
    #[cfg(test)]
    mock_stream_producer: Option<Arc<dyn MockStreamProducer>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiProtocol {
    MessagesV1,
    ChatCompat,
}

impl ApiClient {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::new(),
            api_key: config.model_token.clone(),
            model: config.model_name.clone(),
            api_url: config.model_url.clone(),
            model_backend: config.model_backend,
            model_protocol: config.model_protocol,
            tool_call_mode: config.tool_call_mode,
            model_headers: config.model_headers.clone(),
            #[cfg(test)]
            mock_stream_producer: None,
        })
    }

    #[cfg(test)]
    pub fn new_mock(mock_producer: Arc<dyn MockStreamProducer>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: None,
            model: "mock-model".to_string(),
            api_url: "http://localhost:8000/v1/messages".to_string(),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::Structured,
            model_headers: reqwest::header::HeaderMap::new(),
            mock_stream_producer: Some(mock_producer),
        }
    }

    pub fn supports_structured_tool_protocol(&self) -> bool {
        matches!(self.tool_call_mode, ToolCallMode::Structured)
    }

    pub fn is_local_endpoint(&self) -> bool {
        is_local_endpoint_url(&self.api_url)
    }

    fn api_protocol(&self) -> ApiProtocol {
        match self.model_protocol {
            ModelProtocol::MessagesV1 => ApiProtocol::MessagesV1,
            ModelProtocol::ChatCompat => ApiProtocol::ChatCompat,
        }
    }

    #[cfg(test)]
    pub fn with_structured_tool_protocol(mut self, enabled: bool) -> Self {
        self.tool_call_mode = if enabled {
            ToolCallMode::Structured
        } else {
            ToolCallMode::TaggedFallback
        };
        self
    }

    pub async fn create_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream> {
        #[cfg(test)]
        {
            if let Some(producer) = &self.mock_stream_producer {
                return producer.create_mock_stream(messages);
            }
        }

        let request_url = self.request_url();
        let max_tokens = resolve_max_tokens(&self.api_url);
        let api_protocol = self.api_protocol();
        let payload = match api_protocol {
            ApiProtocol::MessagesV1 => {
                let mut payload = json!({
                    "model": self.model,
                    "max_tokens": max_tokens,
                    "stream": true,
                    "system": SYSTEM_PROMPT,
                    "messages": messages,
                });
                if self.supports_structured_tool_protocol() {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("tool_choice".to_string(), json!({ "type": "auto" }));
                    payload_object.insert("tools".to_string(), tool_definitions());
                }
                payload
            }
            ApiProtocol::ChatCompat => {
                let mut payload = json!({
                    "model": self.model,
                    "max_tokens": max_tokens,
                    "stream": true,
                    "messages": chat_compat_messages(messages, SYSTEM_PROMPT),
                });
                if self.supports_structured_tool_protocol() {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("tool_choice".to_string(), json!("auto"));
                    payload_object.insert("tools".to_string(), tool_definitions_chat_compat());
                }
                payload
            }
        };

        let mut request = self
            .http
            .post(&request_url)
            .header("content-type", "application/json")
            .json(&payload);

        if debug_payload_enabled() {
            emit_debug_payload(&request_url, &payload);
        }

        // Apply operator-supplied headers. Reserved headers are excluded to
        // prevent duplicates — reqwest::RequestBuilder::header() appends, not
        // replaces, so auth headers must only be set once in the block below.
        for (name, value) in &self.model_headers {
            if !is_reserved_header(name.as_str()) {
                request = request.header(name, value);
            }
        }

        // Auth headers set last and exclusively. x-api-key and authorization
        // are in the reserved list above, so they cannot arrive here duplicated.
        match api_protocol {
            ApiProtocol::MessagesV1 => {
                if let Some(api_key) = &self.api_key {
                    request = request.header("x-api-key", api_key);
                }
            }
            ApiProtocol::ChatCompat => {
                if let Some(api_key) = &self.api_key {
                    request = request.header("authorization", format!("Bearer {api_key}"));
                }
            }
        }

        let response = request
            .send()
            .await
            .map_err(|error| map_api_request_error(error, &request_url))?
            .error_for_status()
            .map_err(|error| map_api_request_error(error, &request_url))?;

        let request_url_for_stream = request_url.clone();
        let stream = response.bytes_stream().map(move |item| {
            item.map_err(|error| map_api_request_error(error, &request_url_for_stream))
        });
        Ok(Box::pin(stream))
    }

    fn request_url(&self) -> String {
        match self.api_protocol() {
            ApiProtocol::MessagesV1 => self.api_url.clone(),
            ApiProtocol::ChatCompat => adapt_to_chat_compat_url(&self.api_url),
        }
    }
}

impl ModelBackend for ApiClient {
    fn backend_kind(&self) -> ModelBackendKind {
        self.model_backend
    }

    fn protocol(&self) -> ModelProtocol {
        self.model_protocol
    }

    fn supports_structured_tools(&self) -> bool {
        self.supports_structured_tool_protocol()
    }

    fn is_local(&self) -> bool {
        self.is_local_endpoint()
    }

    async fn create_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream> {
        self.create_stream(messages).await
    }
}

fn map_api_request_error(error: reqwest::Error, request_url: &str) -> anyhow::Error {
    if error.is_connect() && is_local_endpoint_url(request_url) {
        return anyhow!(
            "cannot reach local API endpoint '{}': {}. Start your local server or update VEX_MODEL_URL.",
            request_url,
            error
        );
    }
    if error.is_connect() {
        return anyhow!("cannot reach API endpoint '{}': {}", request_url, error);
    }
    if error.is_timeout() {
        return anyhow!("API request to '{}' timed out: {}", request_url, error);
    }
    if let Some(status) = error.status() {
        return anyhow!(
            "API endpoint '{}' returned HTTP {}: {}",
            request_url,
            status,
            error
        );
    }
    anyhow!("API request to '{}' failed: {}", request_url, error)
}

#[allow(dead_code)]
fn resolve_structured_tool_protocol(api_url: &str) -> bool {
    if let Some(value) = std::env::var("VEX_STRUCTURED_TOOL_PROTOCOL")
        .ok()
        .and_then(parse_bool_flag)
    {
        return value;
    }

    // Local endpoints default to text-protocol fallback because many local servers
    // do not implement structured tool call blocks consistently.
    !is_local_endpoint_url(api_url)
}

fn resolve_max_tokens(api_url: &str) -> u32 {
    if let Some(value) = std::env::var("VEX_MAX_TOKENS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
    {
        return value.clamp(128, 8192);
    }

    if is_local_endpoint_url(api_url) {
        1024
    } else {
        4096
    }
}

#[allow(dead_code)]
fn parse_protocol(value: String) -> Option<ApiProtocol> {
    match value.trim().to_ascii_lowercase().as_str() {
        "messages-v1" | "messages_v1" | "messages" | "v1/messages" => Some(ApiProtocol::MessagesV1),
        "chat-compat" | "chat_compat" | "chat" | "chat_completions" => {
            Some(ApiProtocol::ChatCompat)
        }
        _ => None,
    }
}

#[allow(dead_code)]
fn infer_api_protocol(api_url: &str) -> ApiProtocol {
    let normalized = api_url.trim().to_ascii_lowercase();
    if normalized.contains("/chat/completions") || normalized.ends_with("/v1") {
        ApiProtocol::ChatCompat
    } else {
        ApiProtocol::MessagesV1
    }
}

fn is_reserved_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "x-api-key"
            | "content-type"
            | "content-length"
            | "host"
            | "transfer-encoding"
            | "connection"
    )
}

fn adapt_to_chat_compat_url(api_url: &str) -> String {
    let normalized = api_url.trim_end_matches('/');
    if normalized.ends_with("/chat/completions") {
        return normalized.to_string();
    }
    if let Some(prefix) = normalized.strip_suffix("/messages") {
        return format!("{prefix}/chat/completions");
    }
    if normalized.ends_with("/v1") {
        return format!("{normalized}/chat/completions");
    }
    normalized.to_string()
}

fn chat_compat_messages(messages: &[ApiMessage], system_prompt: &str) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    out.push(json!({
        "role": "system",
        "content": system_prompt
    }));

    for message in messages {
        append_chat_compat_message(&mut out, message);
    }

    out
}

fn append_chat_compat_message(out: &mut Vec<Value>, message: &ApiMessage) {
    match (&message.role[..], &message.content) {
        (role, Content::Text(text)) => {
            out.push(json!({
                "role": role,
                "content": text
            }));
        }
        ("assistant", Content::Blocks(blocks)) => {
            let mut content = String::new();
            let mut tool_calls = Vec::new();

            for block in blocks {
                match block {
                    ContentBlock::Text { text } => content.push_str(text),
                    ContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": tool_input_to_json_string(input),
                            }
                        }));
                    }
                    ContentBlock::ToolResult { .. } => {}
                }
            }

            let mut assistant_message = serde_json::Map::new();
            assistant_message.insert("role".to_string(), json!("assistant"));
            if content.is_empty() {
                assistant_message.insert("content".to_string(), Value::Null);
            } else {
                assistant_message.insert("content".to_string(), Value::String(content));
            }
            if !tool_calls.is_empty() {
                assistant_message.insert("tool_calls".to_string(), Value::Array(tool_calls));
            }
            out.push(Value::Object(assistant_message));
        }
        (role, Content::Blocks(blocks)) => {
            let mut pushed = false;
            for block in blocks {
                match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        out.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content
                        }));
                        pushed = true;
                    }
                    ContentBlock::Text { text } => {
                        out.push(json!({
                            "role": role,
                            "content": text
                        }));
                        pushed = true;
                    }
                    ContentBlock::ToolUse { .. } => {}
                }
            }

            if !pushed {
                out.push(json!({
                    "role": role,
                    "content": ""
                }));
            }
        }
    }
}

fn tool_input_to_json_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
    }
}

fn tool_definitions_chat_compat() -> Value {
    let base = tool_definitions();
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

fn tool_definitions() -> serde_json::Value {
    json!([
        {
            "name": "read_file",
            "description": "Read file content",
            "input_schema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "write_file",
            "description": "Write file content",
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
            "description": "List files and directories under a path.",
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
            "description": "Alias for list_files. List files and directories under a path.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "max_entries": { "type": "integer", "minimum": 1, "maximum": 2000 }
                }
            }
        },
        {
            "name": "search_files",
            "description": "Search text across files and return matching lines.",
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
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::backend::{ModelBackendKind, ModelProtocol, ToolCallMode};
    use std::collections::BTreeSet;

    #[test]
    fn test_protocol_inference_defaults_to_messages_v1() {
        let protocol = infer_api_protocol("http://localhost:8000/v1/messages");
        assert_eq!(protocol, ApiProtocol::MessagesV1);
    }

    #[test]
    fn test_protocol_inference_detects_chat_compat() {
        let protocol = infer_api_protocol("http://localhost:8000/v1/chat/completions");
        assert_eq!(protocol, ApiProtocol::ChatCompat);
    }

    #[test]
    fn test_chat_compat_url_adapter_from_messages_endpoint() {
        let adapted = adapt_to_chat_compat_url("http://localhost:8000/v1/messages");
        assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
    }

    #[test]
    fn test_chat_compat_url_adapter_from_v1_base_endpoint() {
        let adapted = adapt_to_chat_compat_url("http://localhost:8000/v1");
        assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
    }

    #[test]
    fn test_resolve_max_tokens_defaults_for_local() {
        let tokens = resolve_max_tokens("http://localhost:8000/v1/messages");
        assert_eq!(tokens, 1024);
    }

    #[test]
    fn test_tool_definitions_cover_execute_tool_dispatch_names() {
        let expected: BTreeSet<&str> = BTreeSet::from([
            "read_file",
            "write_file",
            "edit_file",
            "rename_file",
            "list_files",
            "list_directory",
            "search_files",
            "search",
            "git_status",
            "git_diff",
            "git_log",
            "git_show",
            "git_add",
            "git_commit",
            "search_content",
            "find_files",
        ]);

        let names: BTreeSet<String> = tool_definitions()
            .as_array()
            .expect("tool definitions must be an array")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(|value| value.as_str()))
            .map(ToOwned::to_owned)
            .collect();

        let expected_owned: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
        assert_eq!(names, expected_owned);
    }

    #[test]
    fn test_structured_tool_protocol_env_off_disables_protocol() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_STRUCTURED_TOOL_PROTOCOL", "off");
        let config = crate::config::Config {
            model_token: None,
            model_name: "mock-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::TaggedFallback,
            model_headers: reqwest::header::HeaderMap::new(),
        };

        let client = ApiClient::new(&config).expect("client should build");
        assert!(!client.supports_structured_tool_protocol());
        std::env::remove_var("VEX_STRUCTURED_TOOL_PROTOCOL");
    }

    #[test]
    fn test_structured_tool_protocol_defaults_off_for_local_endpoint() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::remove_var("VEX_STRUCTURED_TOOL_PROTOCOL");
        let config = crate::config::Config {
            model_token: None,
            model_name: "local/llama.cpp".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::TaggedFallback,
            model_headers: reqwest::header::HeaderMap::new(),
        };

        let client = ApiClient::new(&config).expect("client should build");
        assert!(!client.supports_structured_tool_protocol());
    }

    #[test]
    fn test_structured_tool_protocol_defaults_on_for_remote_endpoint() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::remove_var("VEX_STRUCTURED_TOOL_PROTOCOL");
        let config = crate::config::Config {
            model_token: Some("test-key".to_string()),
            model_name: "mistral-7b-instruct".to_string(),
            model_url: "https://model.example.internal/v1/messages".to_string(),
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::ApiServer,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::Structured,
            model_headers: reqwest::header::HeaderMap::new(),
        };

        let client = ApiClient::new(&config).expect("client should build");
        assert!(client.supports_structured_tool_protocol());
    }

    #[test]
    fn test_reserved_header_guard_blocks_auth_headers() {
        assert!(is_reserved_header("authorization"));
        assert!(is_reserved_header("Authorization"));
        assert!(is_reserved_header("x-api-key"));
        assert!(is_reserved_header("X-Api-Key"));
        assert!(is_reserved_header("content-length"));
        assert!(!is_reserved_header("x-custom-header"));
        assert!(!is_reserved_header("x-api-version"));
    }

    #[test]
    fn test_chat_compat_tool_definitions_match_base_tool_names() {
        let base_names: BTreeSet<String> = tool_definitions()
            .as_array()
            .expect("tool definitions must be an array")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(|value| value.as_str()))
            .map(ToOwned::to_owned)
            .collect();

        let chat_compat_names: BTreeSet<String> = tool_definitions_chat_compat()
            .as_array()
            .expect("chat-compat tool definitions must be an array")
            .iter()
            .filter_map(|tool| {
                tool.get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(|name| name.as_str())
            })
            .map(ToOwned::to_owned)
            .collect();

        assert_eq!(chat_compat_names, base_names);
    }

    #[test]
    fn test_system_prompt_restricts_git_tool_capability_claims() {
        assert!(SYSTEM_PROMPT.contains("only list built-in git tools"));
        assert!(
            SYSTEM_PROMPT.contains("git_status, git_diff, git_log, git_show, git_add, git_commit")
        );
        assert!(SYSTEM_PROMPT.contains("Do not claim unsupported git tools"));
    }
}
