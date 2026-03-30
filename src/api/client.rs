use super::logging::{debug_payload_enabled, emit_debug_payload};
use crate::config::Config;
use crate::runtime::backend::{
    ByteStream, ModelBackend, ModelBackendKind, ModelProtocol, ToolCallMode,
};
use crate::types::{ApiMessage, Content, ContentBlock};
use crate::util::{is_local_endpoint_url, preferred_plain_http_url_for_local_endpoint};
use anyhow::anyhow;
use anyhow::Result;
use futures::StreamExt;
use serde_json::json;
use serde_json::Value;
use std::sync::{Arc, OnceLock, RwLock};
/// Base system prompt applied to every API call.
/// Project instructions are appended at runtime via
/// `ApiClient::with_project_instructions`.
const BASE_SYSTEM_PROMPT: &str = "You are a coding assistant.\n\
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
Use codebase_search to find functions, types, and code patterns before reading files. Only use read_file with offset/limit when you know the exact location.\n\
Prefer search_files for targeted string matches and avoid full-file reads unless required.\n\
Use list_files/search_files/read_file before saying a file is missing or present.\n\
For repository summaries or unfamiliar codebases, start with list_files at the workspace root and/or codebase_search; do not call read_file until you have an explicit non-empty path.\n\
For edit_file, use a focused old_str snippet around the target change and avoid whole-file replacements; use write_file only for smaller full-file rewrites that stay under the write-file guard thresholds.\n\
For large files, prefer apply_patch or edit_file over write_file; if write_file warns or rejects due to line limits, switch tools instead of retrying the same call.\n\
For code edits, prefer this sequence: search_files -> read_file -> edit_file -> read_file (verify), escalating to apply_patch when the change is too broad for edit_file.\n\
For read-only requests (show/read/list/count/status/log/diff), use read-only tools and do not call mutating tools unless the user explicitly asks for changes.\n\
If asked what git tools are available, only list built-in git tools: git_status, git_diff, git_log, git_show, git_add, git_commit.\n\
Do not claim unsupported git tools like git_clone, git_init, git_remote, git_config, git_pull, git_push, git_branch, git_checkout, or git_stash.\n\
Always send non-empty string paths for file tools.\n\
Avoid redundant loops: do not repeat identical read/search tool calls without new evidence.\n\
Tool results from earlier turns may be condensed to their first few lines; if you need the full output, re-run the tool instead of assuming the truncated text is complete.";

#[cfg(test)]
pub trait MockStreamProducer: Send + Sync {
    fn create_mock_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream>;
}

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    api_key: Option<String>,
    model: Arc<RwLock<String>>,
    supplementary_system_prompt: Arc<RwLock<Option<String>>>,
    api_url: String,
    model_backend: ModelBackendKind,
    model_protocol: ModelProtocol,
    tool_call_mode: ToolCallMode,
    model_headers: reqwest::header::HeaderMap,
    temperature: f32,
    top_p: f32,
    max_tokens: u32,
    stop_sequences: Vec<String>,
    reasoning_budget: u32,
    /// Project instructions block appended to the base prompt when present.
    project_instructions: Option<String>,
    notes_content: Option<String>,
    extra_tool_definitions: Vec<Value>,
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
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(config.model_url_skip_tls_check)
            .build()?;
        Ok(Self {
            http,
            api_key: config.model_token.clone(),
            model: Arc::new(RwLock::new(config.model_name.clone())),
            supplementary_system_prompt: Arc::new(RwLock::new(None)),
            api_url: config.model_url.clone(),
            model_backend: config.model_backend,
            model_protocol: config.model_protocol,
            tool_call_mode: config.tool_call_mode,
            model_headers: config.model_headers.clone(),
            temperature: config.model_profile.temperature,
            top_p: config.model_profile.top_p,
            max_tokens: config.model_profile.max_tokens,
            stop_sequences: config.model_profile.stop_sequences.clone(),
            reasoning_budget: config.model_profile.reasoning_budget,
            project_instructions: None,
            notes_content: None,
            extra_tool_definitions: Vec::new(),
            #[cfg(test)]
            mock_stream_producer: None,
        })
    }

    #[cfg(test)]
    pub fn new_mock(mock_producer: Arc<dyn MockStreamProducer>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: None,
            model: Arc::new(RwLock::new("mock-model".to_string())),
            supplementary_system_prompt: Arc::new(RwLock::new(None)),
            // Test-only override for mock endpoint URL; defaults to portless localhost.
            api_url: std::env::var("VEX_TEST_MODEL_URL")
                .unwrap_or_else(|_| "http://localhost/v1/messages".to_string()),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::Structured,
            model_headers: reqwest::header::HeaderMap::new(),
            temperature: 0.3,
            top_p: 1.0,
            max_tokens: 4096,
            stop_sequences: Vec::new(),
            reasoning_budget: 0,
            project_instructions: None,
            notes_content: None,
            extra_tool_definitions: Vec::new(),
            mock_stream_producer: Some(mock_producer),
        }
    }

    pub fn with_notes_content(mut self, content: Option<String>) -> Self {
        self.notes_content = content;
        self
    }

    pub fn with_extra_tool_definitions(mut self, extra_tools: Vec<Value>) -> Self {
        self.extra_tool_definitions = extra_tools;
        self
    }

    /// Attach project-instructions text. Builder pattern; consumes and
    /// returns self. Pass `None` to use the base prompt unmodified.
    pub fn with_project_instructions(mut self, instructions: Option<String>) -> Self {
        self.project_instructions = instructions;
        self
    }

    pub fn supports_structured_tool_protocol(&self) -> bool {
        matches!(self.tool_call_mode, ToolCallMode::Structured)
    }

    pub fn is_local_endpoint(&self) -> bool {
        is_local_endpoint_url(&self.api_url)
    }

    /// Return a startup warning when a local endpoint uses HTTPS.
    /// HTTPS is not supported for local inference servers; HTTP is required.
    pub fn https_local_startup_warning(&self) -> Option<String> {
        let lower = self.api_url.to_ascii_lowercase();
        if !self.is_local_endpoint() || !lower.starts_with("https://") {
            return None;
        }
        let plain = preferred_plain_http_url_for_local_endpoint(&self.api_url)?;
        Some(format!(
            "[warning] local endpoint '{}' uses HTTPS; plain HTTP is required for local servers. Consider '{}'.",
            self.api_url, plain
        ))
    }

    pub fn model_name(&self) -> String {
        self.model
            .read()
            .expect("api client model lock poisoned")
            .clone()
    }

    pub fn set_model_name(&self, name: impl Into<String>) {
        *self.model.write().expect("api client model lock poisoned") = name.into();
    }

    pub fn set_supplementary_system_prompt(&self, prompt: Option<String>) {
        *self
            .supplementary_system_prompt
            .write()
            .expect("api client supplementary prompt lock poisoned") = prompt
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }

    fn api_protocol(&self) -> ApiProtocol {
        match self.model_protocol {
            ModelProtocol::MessagesV1 => ApiProtocol::MessagesV1,
            ModelProtocol::ChatCompat => ApiProtocol::ChatCompat,
        }
    }

    fn effective_system_prompt(&self) -> String {
        let mut prompt = BASE_SYSTEM_PROMPT.to_string();

        if let Some(supplementary) = self
            .supplementary_system_prompt
            .read()
            .expect("api client supplementary prompt lock poisoned")
            .as_deref()
            .map(str::trim)
            .filter(|supplementary| !supplementary.is_empty())
        {
            prompt.push_str("\n\n---\n[coding prompt: start]\n");
            prompt.push_str(supplementary);
            prompt.push_str("\n[coding prompt: end]\n---");
        }

        if let Some(instructions) = self
            .project_instructions
            .as_deref()
            .map(str::trim)
            .filter(|instructions| !instructions.is_empty())
        {
            prompt.push_str("\n\n---\n[project instructions: start]\n");
            prompt.push_str(instructions);
            prompt.push_str("\n[project instructions: end]\n---");
        }

        if let Some(notes) = self
            .notes_content
            .as_deref()
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
        {
            prompt.push_str("\n\n<memory>\n");
            prompt.push_str(notes);
            prompt.push_str("\n</memory>");
        }

        prompt
    }

    fn system_prompt(&self) -> String {
        self.effective_system_prompt()
    }

    #[cfg(test)]
    pub fn test_system_prompt(&self) -> String {
        self.system_prompt()
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
        let max_tokens = resolve_max_tokens(self.max_tokens);
        let api_protocol = self.api_protocol();
        let model = self.model_name();
        let system_prompt = self.system_prompt();
        let payload = match api_protocol {
            ApiProtocol::MessagesV1 => {
                let mut payload = json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "temperature": self.temperature,
                    "top_p": self.top_p,
                    "stream": true,
                    "system": system_prompt.clone(),
                    "messages": messages,
                });
                if self.supports_structured_tool_protocol() {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("tool_choice".to_string(), json!({ "type": "auto" }));
                    payload_object.insert(
                        "tools".to_string(),
                        tool_definitions_with_extra(&self.extra_tool_definitions),
                    );
                }
                if !self.stop_sequences.is_empty() {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("stop_sequences".to_string(), json!(self.stop_sequences));
                }
                payload
            }
            ApiProtocol::ChatCompat => {
                let mut payload = json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "temperature": self.temperature,
                    "top_p": self.top_p,
                    "stream": true,
                    "messages": chat_compat_messages(messages, &system_prompt),
                });
                if self.supports_structured_tool_protocol() {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("tool_choice".to_string(), json!("auto"));
                    payload_object.insert(
                        "tools".to_string(),
                        tool_definitions_chat_compat_with_extra(&self.extra_tool_definitions),
                    );
                }
                if !self.stop_sequences.is_empty() {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("stop".to_string(), json!(self.stop_sequences));
                }
                if self.reasoning_budget > 0 {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object
                        .insert("reasoning_effort".to_string(), json!(self.reasoning_budget));
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
            .map_err(|error| map_api_request_error(error, &request_url))?;

        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            let body = response.text().await.unwrap_or_default();
            return Err(map_api_status_error(status, &body, &request_url));
        }

        let request_url_for_stream = request_url.clone();
        let stream = response.bytes_stream().map(move |item| {
            item.map_err(|error| map_api_request_error(error, &request_url_for_stream))
        });
        Ok(Box::pin(stream))
    }

    fn request_url(&self) -> String {
        match self.api_protocol() {
            ApiProtocol::MessagesV1 => adapt_to_messages_v1_url(&self.api_url),
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
    let local_http_hint = local_plain_http_hint(request_url);

    if error.is_connect() && is_local_endpoint_url(request_url) {
        return anyhow!(
            "cannot reach local API endpoint '{}': {}. Start your local server or update VEX_MODEL_URL.{}",
            request_url,
            error,
            local_http_hint
        );
    }
    if error.is_connect() {
        return anyhow!("cannot reach API endpoint '{}': {}", request_url, error);
    }
    if error.is_timeout() {
        return anyhow!(
            "API request to '{}' timed out: {}.{}",
            request_url,
            error,
            local_http_hint
        );
    }
    if let Some(status) = error.status() {
        if status == reqwest::StatusCode::BAD_REQUEST && is_local_endpoint_url(request_url) {
            let detected = infer_api_protocol(request_url);
            return anyhow!(
                "API endpoint '{}' returned HTTP 400 Bad Request: {}\n  \
                 detected protocol: {:?}. Check: model name, protocol format \
                 (MessagesV1 vs ChatCompat), and whether the server supports streaming.{}",
                request_url,
                error,
                detected,
                local_http_hint
            );
        }
        return anyhow!(
            "API endpoint '{}' returned HTTP {}: {}",
            request_url,
            status,
            error
        );
    }
    anyhow!("API request to '{}' failed: {}", request_url, error)
}

/// Handle HTTP 4xx responses where the body has already been read.
/// Detects context-overflow errors from local inference servers and provides
/// actionable guidance including `--ctx-size` configuration hints.
fn map_api_status_error(
    status: reqwest::StatusCode,
    body: &str,
    request_url: &str,
) -> anyhow::Error {
    let local_http_hint = local_plain_http_hint(request_url);
    let is_local = is_local_endpoint_url(request_url);

    // Detect context-window overflow from local inference servers.
    if is_context_overflow(body) {
        let ctx_hint = if is_local {
            "\n  The conversation has exceeded the server's context window. \
             Increase the server context size (e.g. --ctx-size 8192) or use /compact to reset."
        } else {
            "\n  The conversation has exceeded the endpoint's context window. \
             Use /compact to reset the conversation."
        };
        return anyhow!(
            "API endpoint '{}' returned HTTP {}: {}{}\n  Server message: {}",
            request_url,
            status.as_u16(),
            status.canonical_reason().unwrap_or(""),
            ctx_hint,
            body.chars().take(300).collect::<String>()
        );
    }

    // Local 400 with protocol hint (non-context-overflow).
    if status == reqwest::StatusCode::BAD_REQUEST && is_local {
        let detected = infer_api_protocol(request_url);
        return anyhow!(
            "API endpoint '{}' returned HTTP 400 Bad Request.\n  \
             detected protocol: {:?}. Check: model name, protocol format \
             (MessagesV1 vs ChatCompat), and whether the server supports streaming.{}\n  \
             Server message: {}",
            request_url,
            detected,
            local_http_hint,
            body.chars().take(300).collect::<String>()
        );
    }

    anyhow!(
        "API endpoint '{}' returned HTTP {}: {}",
        request_url,
        status.as_u16(),
        if body.is_empty() {
            status
                .canonical_reason()
                .unwrap_or("unknown error")
                .to_string()
        } else {
            body.chars().take(500).collect::<String>()
        }
    )
}

/// Returns true when the response body indicates the request exceeded the
/// server's configured context window.
pub fn is_context_overflow(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("exceeds the available context size")
        || lower.contains("exceeds context")
        || lower.contains("context length exceeded")
        || lower.contains("maximum context length")
        || lower.contains("context window")
}

fn local_plain_http_hint(request_url: &str) -> String {
    preferred_plain_http_url_for_local_endpoint(request_url)
        .map(|http_url| format!(" Try '{}'.", http_url))
        .unwrap_or_default()
}

fn resolve_max_tokens(default_max_tokens: u32) -> u32 {
    if let Some(value) = std::env::var("VEX_MAX_TOKENS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
    {
        return value.clamp(128, 8192);
    }
    default_max_tokens
}

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

fn adapt_to_messages_v1_url(api_url: &str) -> String {
    let normalized = api_url.trim_end_matches('/');
    if normalized.ends_with("/messages") {
        return normalized.to_string();
    }
    if let Some(prefix) = normalized.strip_suffix("/chat/completions") {
        return format!("{prefix}/messages");
    }
    if normalized.ends_with("/v1") {
        return format!("{normalized}/messages");
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
                    ContentBlock::Text { text, .. } => content.push_str(text),
                    ContentBlock::ToolUse {
                        id, name, input, ..
                    } => {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": tool_input_to_json_string(input),
                            }
                        }));
                    }
                    ContentBlock::ToolResult { .. }
                    | ContentBlock::Thinking { .. }
                    | ContentBlock::RedactedThinking { .. }
                    | ContentBlock::ServerToolUse { .. }
                    | ContentBlock::WebSearchToolResult { .. } => {}
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
                    ContentBlock::Text { text, .. } => {
                        out.push(json!({
                            "role": role,
                            "content": text
                        }));
                        pushed = true;
                    }
                    ContentBlock::ToolUse { .. }
                    | ContentBlock::Thinking { .. }
                    | ContentBlock::RedactedThinking { .. }
                    | ContentBlock::ServerToolUse { .. }
                    | ContentBlock::WebSearchToolResult { .. } => {}
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

fn tool_definitions() -> serde_json::Value {
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

fn tool_definitions_with_extra(extra: &[Value]) -> serde_json::Value {
    if extra.is_empty() {
        return tool_definitions();
    }
    let mut definitions = tool_definitions().as_array().cloned().unwrap_or_default();
    definitions.extend(extra.iter().cloned());
    Value::Array(definitions)
}

fn tool_definitions_chat_compat_with_extra(extra: &[Value]) -> Value {
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
    fn test_local_messages_endpoint_keeps_messages_v1_wire_protocol() {
        let config = crate::config::Config {
            model_token: None,
            model_name: "local/test-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            model_url_skip_tls_check: false,
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            http_hooks: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };

        let client = ApiClient::new(&config).expect("client should build");
        assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
        assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
        assert_eq!(client.protocol(), ModelProtocol::MessagesV1);
    }

    #[test]
    fn test_local_bare_v1_endpoint_resolves_messages_v1_url() {
        let config = crate::config::Config {
            model_token: None,
            model_name: "local/test-model".to_string(),
            model_url: "http://localhost:8000/v1".to_string(),
            model_url_skip_tls_check: false,
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            http_hooks: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };

        let client = ApiClient::new(&config).expect("client should build");
        assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
        assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
        assert_eq!(client.protocol(), ModelProtocol::MessagesV1);
    }

    #[test]
    fn test_local_bare_v1_endpoint_resolves_chat_compat_url() {
        let config = crate::config::Config {
            model_token: None,
            model_name: "local/test-model".to_string(),
            model_url: "http://localhost:8000/v1".to_string(),
            model_url_skip_tls_check: false,
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::ChatCompat,
            tool_call_mode: ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            http_hooks: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };

        let client = ApiClient::new(&config).expect("client should build");
        assert_eq!(client.api_protocol(), ApiProtocol::ChatCompat);
        assert_eq!(
            client.request_url(),
            "http://localhost:8000/v1/chat/completions"
        );
        assert_eq!(client.protocol(), ModelProtocol::ChatCompat);
    }

    #[test]
    fn test_remote_messages_endpoint_preserves_messages_wire_protocol() {
        let config = crate::config::Config {
            model_token: Some("test-key".to_string()),
            model_name: "remote-test-model".to_string(),
            model_url: "https://model.example.internal/v1/messages".to_string(),
            model_url_skip_tls_check: false,
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::ApiServer,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::Structured,
            model_profile: crate::types::ModelProfile::default_for_backend(
                ModelBackendKind::ApiServer,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            http_hooks: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };

        let client = ApiClient::new(&config).expect("client should build");
        assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
        assert_eq!(
            client.request_url(),
            "https://model.example.internal/v1/messages"
        );
    }

    #[test]
    fn test_https_localhost_messages_endpoint_preserves_full_request_url() {
        let config = crate::config::Config {
            model_token: Some("test-key".to_string()),
            model_name: "remote-test-model".to_string(),
            model_url: "https://localhost:8443/v1/messages".to_string(),
            model_url_skip_tls_check: true,
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::ApiServer,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::Structured,
            model_profile: crate::types::ModelProfile::default_for_backend(
                ModelBackendKind::ApiServer,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            http_hooks: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
        };

        let client = ApiClient::new(&config).expect("client should build");
        assert_eq!(client.request_url(), "https://localhost:8443/v1/messages");
    }

    #[test]
    fn test_local_plain_http_hint_suggests_plain_http_endpoint() {
        let hint = local_plain_http_hint("https://localhost:8000/v1/messages");
        assert_eq!(hint, " Try 'http://localhost:8000/v1/messages'.");
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
    fn test_resolve_max_tokens_defaults_to_profile_budget() {
        let tokens = resolve_max_tokens(4096);
        assert_eq!(tokens, 4096);
    }

    #[test]
    fn test_tool_definitions_cover_execute_tool_dispatch_names() {
        let expected: BTreeSet<&str> = BTreeSet::from([
            "read_file",
            "write_file",
            "apply_patch",
            "edit_file",
            "rename_file",
            "list_files",
            "list_directory",
            "list_dir",
            "glob_files",
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
            "codebase_search",
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
            model_url_skip_tls_check: false,
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            http_hooks: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
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
            model_name: "local/test-model".to_string(),
            model_url: "http://localhost:8000/v1/messages".to_string(),
            model_url_skip_tls_check: false,
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::TaggedFallback,
            model_profile: crate::types::ModelProfile::default_for_backend(
                ModelBackendKind::LocalRuntime,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            http_hooks: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
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
            model_url_skip_tls_check: false,
            working_dir: std::path::PathBuf::from("."),
            model_backend: ModelBackendKind::ApiServer,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::Structured,
            model_profile: crate::types::ModelProfile::default_for_backend(
                ModelBackendKind::ApiServer,
            ),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: crate::runtime::SandboxConfig::default(),
            model_headers: reqwest::header::HeaderMap::new(),
            mcp_servers: Vec::new(),
            http_hooks: Vec::new(),
            notes_path: None,
            api: crate::config::ApiConfig::default(),
            hooks: Vec::new(),
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

        let chat_compat_names: BTreeSet<String> = tool_definitions_chat_compat_with_extra(&[])
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
    fn test_system_prompt_includes_memory_notes() {
        let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        )))
        .with_notes_content(Some("remember this".to_string()));
        let prompt = client.system_prompt();
        assert!(prompt.starts_with(BASE_SYSTEM_PROMPT));
        assert!(prompt.contains("<memory>\nremember this\n</memory>"));
    }

    #[test]
    fn test_system_prompt_omits_blank_memory_notes() {
        let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        )))
        .with_notes_content(Some("   \n".to_string()));
        assert_eq!(client.system_prompt(), BASE_SYSTEM_PROMPT);
    }

    #[test]
    fn test_system_prompt_restricts_git_tool_capability_claims() {
        assert!(BASE_SYSTEM_PROMPT.contains("only list built-in git tools"));
        assert!(BASE_SYSTEM_PROMPT
            .contains("git_status, git_diff, git_log, git_show, git_add, git_commit"));
        assert!(BASE_SYSTEM_PROMPT.contains("Do not claim unsupported git tools"));
    }

    #[test]
    fn test_system_prompt_includes_large_file_edit_guidance() {
        assert!(BASE_SYSTEM_PROMPT.contains(
            "use write_file only for smaller full-file rewrites that stay under the write-file guard thresholds"
        ));
        assert!(BASE_SYSTEM_PROMPT
            .contains("For large files, prefer apply_patch or edit_file over write_file"));
        assert!(BASE_SYSTEM_PROMPT
            .contains("escalating to apply_patch when the change is too broad for edit_file"));
    }

    #[test]
    fn test_write_file_tool_description_uses_guard_names_instead_of_hardcoded_numbers() {
        let definitions = tool_definitions();
        let description = definitions
            .as_array()
            .expect("tool definitions must be an array")
            .iter()
            .find(|entry| entry.get("name").and_then(|v| v.as_str()) == Some("write_file"))
            .and_then(|entry| entry.get("description"))
            .and_then(|value| value.as_str())
            .expect("write_file description must be present");

        assert!(description.contains("diff-preferred threshold"));
        assert!(description.contains("max line limit"));
        assert!(!description.contains("~200"));
        assert!(!description.contains("~500"));
    }

    #[test]
    fn test_system_prompt_includes_history_condensing_awareness() {
        assert!(BASE_SYSTEM_PROMPT.contains("Tool results from earlier turns may be condensed"));
    }

    #[test]
    fn test_with_project_instructions_none_uses_base_prompt() {
        let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        )))
        .with_project_instructions(None);
        assert_eq!(client.effective_system_prompt(), BASE_SYSTEM_PROMPT);
    }

    #[test]
    fn test_with_project_instructions_some_wraps_in_delimiters() {
        let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        )))
        .with_project_instructions(Some("# no unwrap".to_string()));
        let prompt = client.effective_system_prompt();
        assert!(prompt.starts_with(BASE_SYSTEM_PROMPT));
        assert!(prompt.contains("[project instructions: start]"));
        assert!(prompt.contains("# no unwrap"));
        assert!(prompt.contains("[project instructions: end]"));
    }

    #[test]
    fn test_system_prompt_includes_supplementary_prompt_when_set() {
        let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        )));
        client.set_supplementary_system_prompt(Some("coding mode active".to_string()));

        let prompt = client.system_prompt();

        assert!(prompt.contains("[coding prompt: start]"));
        assert!(prompt.contains("coding mode active"));
        assert!(prompt.contains("[coding prompt: end]"));
    }

    #[test]
    fn test_system_prompt_clears_supplementary_prompt() {
        let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        )));
        client.set_supplementary_system_prompt(Some("coding mode active".to_string()));
        client.set_supplementary_system_prompt(None);

        assert_eq!(client.system_prompt(), BASE_SYSTEM_PROMPT);
    }

    #[test]
    fn test_is_context_overflow_detects_token_exceeded() {
        assert!(is_context_overflow(
            "request (4291 tokens) exceeds the available context size (4096 tokens), try increasing it"
        ));
    }

    #[test]
    fn test_is_context_overflow_detects_max_context_length() {
        assert!(is_context_overflow(
            "This model's maximum context length is 4096 tokens"
        ));
    }

    #[test]
    fn test_is_context_overflow_negative() {
        assert!(!is_context_overflow("invalid model name"));
        assert!(!is_context_overflow("bad request"));
        assert!(!is_context_overflow(""));
    }

    #[test]
    fn test_map_api_status_error_context_overflow_local() {
        let err = map_api_status_error(
            reqwest::StatusCode::BAD_REQUEST,
            "request (4291 tokens) exceeds the available context size (4096 tokens)",
            "http://localhost:8000/v1/messages",
        );
        let msg = format!("{}", err);
        assert!(
            msg.contains("exceeded the server's context window"),
            "got: {msg}"
        );
        assert!(msg.contains("--ctx-size"), "got: {msg}");
        assert!(msg.contains("/compact"), "got: {msg}");
    }

    #[test]
    fn test_map_api_status_error_generic_400_local() {
        let err = map_api_status_error(
            reqwest::StatusCode::BAD_REQUEST,
            "invalid model name",
            "http://localhost:8000/v1/messages",
        );
        let msg = format!("{}", err);
        assert!(msg.contains("protocol"), "got: {msg}");
        assert!(msg.contains("MessagesV1"), "got: {msg}");
    }

    #[test]
    fn test_map_api_status_error_remote_400() {
        let err = map_api_status_error(
            reqwest::StatusCode::BAD_REQUEST,
            "bad request body",
            "https://api.example.com/v1/messages",
        );
        let msg = format!("{}", err);
        assert!(msg.contains("bad request body"), "got: {msg}");
        assert!(!msg.contains("--ctx-size"), "got: {msg}");
    }

    #[test]
    fn test_map_api_status_error_server_500() {
        let err = map_api_status_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "internal server error: out of memory",
            "http://localhost:8000/v1/messages",
        );
        let msg = format!("{}", err);
        assert!(msg.contains("500"), "got: {msg}");
        assert!(msg.contains("out of memory"), "got: {msg}");
    }
}
