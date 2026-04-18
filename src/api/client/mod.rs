use self::protocol_discovery::discover_protocol;
use super::eventsource::create_event_stream;
use super::logging::{debug_payload_enabled, emit_debug_payload};
use crate::config::{Config, ProtocolVariant};
use crate::runtime::backend::{
    EventStream, ModelBackend, ModelBackendKind, ModelProtocol, ToolCallMode, ToolPolicy,
};
use crate::types::{ApiMessage, Content, ContentBlock};
use crate::util::{is_local_endpoint_url, preferred_plain_http_url_for_local_endpoint};
use anyhow::Result;
use anyhow::anyhow;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::sync::{Arc, RwLock};

/// Server capabilities discovered from a local inference server.
/// Populated by `poll_server_info()` once per session for local endpoints.
#[derive(Debug, Clone, Default)]
pub struct ServerInfo {
    /// Total context window (tokens). 0 if unknown.
    pub n_ctx: u32,
    /// Decode batch size. 0 if unknown.
    pub n_batch: u32,
    /// Model identifier reported by the server.
    pub model: String,
    /// The protocol the server handles natively without conversion.
    /// When `Some`, the client prefers this over the user-configured
    /// protocol to avoid server-side format conversion overhead.
    pub native_protocol: Option<ModelProtocol>,
}

/// Response shape for a local inference server `/props` endpoint.
#[derive(Debug, Deserialize, Default)]
struct LocalServerProps {
    #[serde(default)]
    default_generation_settings: Option<LocalServerGenSettings>,
}

#[derive(Debug, Deserialize, Default)]
struct LocalServerGenSettings {
    #[serde(default)]
    n_ctx: u32,
    #[serde(default)]
    n_batch: u32,
    #[serde(default)]
    model: String,
}

/// Attempt to discover server capabilities from a local inference endpoint.
/// Returns `None` if the server is not reachable or does not expose a
/// recognised discovery endpoint. Best-effort; never blocks the session.
pub async fn poll_server_info(http: &reqwest::Client, api_url: &str) -> Option<ServerInfo> {
    let base = local_endpoint_base_url(api_url)?;
    let props_url = format!("{base}/props");
    let models_url = format!("{base}/v1/models");

    // Try the /props discovery endpoint first (supported by some local servers).
    let mut info: Option<ServerInfo> = None;
    tracing::debug!(
        target: "vex::http",
        method = "GET",
        url = %crate::runtime::rewrite_url_for_logs(&props_url),
        "sending local server discovery request"
    );
    if let Ok(resp) = http
        .get(&props_url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        && resp.status().is_success()
        && let Ok(props) = resp.json::<LocalServerProps>().await
        && let Some(gs) = props.default_generation_settings
        && gs.n_ctx > 0
    {
        info = Some(ServerInfo {
            n_ctx: gs.n_ctx,
            n_batch: gs.n_batch,
            model: gs.model,
            native_protocol: None,
        });
    }

    // Fallback: try /v1/models for other servers.
    tracing::debug!(
        target: "vex::http",
        method = "GET",
        url = %crate::runtime::rewrite_url_for_logs(&models_url),
        "sending local server discovery request"
    );
    if info.is_none()
        && let Ok(resp) = http
            .get(&models_url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<Value>().await
    {
        let model = body
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|m| m.get("id"))
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string();
        if !model.is_empty() {
            info = Some(ServerInfo {
                n_ctx: 0,
                n_batch: 0,
                model,
                native_protocol: None,
            });
        }
    }

    info
}

fn local_endpoint_base_url(api_url: &str) -> Option<String> {
    if !is_local_endpoint_url(api_url) {
        return None;
    }

    Some(
        api_url
            .trim_end_matches('/')
            .trim_end_matches("/chat/completions")
            .trim_end_matches("/messages")
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_string(),
    )
}

async fn discover_native_protocol(
    http: &reqwest::Client,
    base_url: &str,
    explicit_protocol: Option<ProtocolVariant>,
    probe_timeout_ms: u64,
) -> Option<ModelProtocol> {
    if let Some(protocol) = explicit_protocol {
        return Some(protocol_variant_to_model_protocol(protocol));
    }

    discover_protocol(
        base_url,
        http,
        std::time::Duration::from_millis(probe_timeout_ms.max(1)),
    )
    .await
    .ok()
    .map(|result| protocol_variant_to_model_protocol(result.protocol))
}
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
Available tools are exactly: read_file, write_file, apply_patch, edit_file, rename_file, list_files, list_directory, list_dir, glob_files, search_files, search, git_status, git_diff, git_log, git_show, git_add, git_commit, search_content, find_files, codebase_search, run_command. Shell aliases (run_shell_command, bash, execute_command, execute_bash) all route to run_command. Every run_command call requires explicit user approval before execution. For counting, aggregation, or analysis, prefer search_files/read_file results and compute in your response before resorting to run_command. Rely on workspace context and prior tool results (memory) rather than assuming shell access.\n\
Always send non-empty string paths for file tools.\n\
Avoid redundant loops: do not repeat identical read/search tool calls without new evidence.\n\
Tool results from earlier turns may be condensed to their first few lines; if you need the full output, re-run the tool instead of assuming the shortened excerpt is complete.";

#[cfg(test)]
pub trait MockStreamProducer: Send + Sync {
    fn create_mock_stream(&self, messages: &[ApiMessage]) -> Result<EventStream>;
}

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    api_key: Option<String>,
    model: Arc<RwLock<String>>,
    supplementary_system_prompt: Arc<RwLock<Option<String>>>,
    api_url: String,
    api_client_explicit_protocol: Option<ProtocolVariant>,
    model_backend: ModelBackendKind,
    model_protocol: ModelProtocol,
    tool_call_mode: ToolCallMode,
    tool_policy: ToolPolicy,
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
    server_info: Arc<RwLock<Option<ServerInfo>>>,
    tls_verification_disabled: bool,
    probe_timeout_ms: u64,
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
        let api_client_base_url = configured_api_client_base_url(config);
        let http = crate::net::http_client::default_client(config.model_url_skip_tls_check)?;
        Ok(Self {
            http,
            api_key: config.model_token.clone(),
            model: Arc::new(RwLock::new(config.model_name.clone())),
            supplementary_system_prompt: Arc::new(RwLock::new(None)),
            api_url: api_client_base_url
                .clone()
                .unwrap_or_else(|| config.model_url.clone()),
            api_client_explicit_protocol: config.api_client.explicit_protocol,
            model_backend: config.model_backend,
            model_protocol: config
                .api_client
                .explicit_protocol
                .map(protocol_variant_to_model_protocol)
                .unwrap_or(config.model_protocol),
            tool_call_mode: config.tool_call_mode,
            tool_policy: config.tool_policy,
            model_headers: config.model_headers.clone(),
            temperature: config.model_profile.temperature,
            top_p: config.model_profile.top_p,
            max_tokens: config.model_profile.max_tokens,
            stop_sequences: config.model_profile.stop_sequences.clone(),
            reasoning_budget: config.model_profile.reasoning_budget,
            project_instructions: None,
            notes_content: None,
            extra_tool_definitions: Vec::new(),
            server_info: Arc::new(RwLock::new(None)),
            tls_verification_disabled: config.model_url_skip_tls_check,
            probe_timeout_ms: config.api_client.probe_timeout_ms,
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
            api_client_explicit_protocol: None,
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::Structured,
            tool_policy: ToolPolicy::Full,
            model_headers: reqwest::header::HeaderMap::new(),
            temperature: 0.3,
            top_p: 1.0,
            max_tokens: 4096,
            stop_sequences: Vec::new(),
            reasoning_budget: 0,
            project_instructions: None,
            notes_content: None,
            extra_tool_definitions: Vec::new(),
            server_info: Arc::new(RwLock::new(None)),
            tls_verification_disabled: false,
            probe_timeout_ms: crate::config::ApiClientConfig::default().probe_timeout_ms,
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

    /// Poll the local inference server for capabilities and cache the result.
    /// No-op if the endpoint is not local or the server does not respond.
    pub async fn populate_server_info(&self) {
        let Some(base_url) = local_endpoint_base_url(&self.api_url) else {
            return;
        };

        let native_protocol = discover_native_protocol(
            &self.http,
            &base_url,
            self.api_client_explicit_protocol,
            self.probe_timeout_ms,
        )
        .await;

        if let Some(mut info) = poll_server_info(&self.http, &base_url).await {
            info.native_protocol = native_protocol;
            self.set_server_info(info);
            return;
        }

        if let Some(native_protocol) = native_protocol {
            self.set_server_info(ServerInfo {
                native_protocol: Some(native_protocol),
                ..ServerInfo::default()
            });
        }
    }

    /// Store server capabilities discovered by `poll_server_info()`.
    pub fn set_server_info(&self, info: ServerInfo) {
        *self.server_info.write().expect("server_info lock poisoned") = Some(info);
    }

    /// Read cached server info, if available.
    pub fn server_info(&self) -> Option<ServerInfo> {
        self.server_info
            .read()
            .expect("server_info lock poisoned")
            .clone()
    }

    /// Estimated context window size in tokens. Returns the server-reported
    /// `n_ctx` when known, or a conservative default of 8192 otherwise.
    pub fn context_window_tokens(&self) -> usize {
        self.server_info()
            .map(|i| i.n_ctx as usize)
            .filter(|&n| n > 0)
            .unwrap_or(8192)
    }

    /// Attach project-instructions text. Builder pattern; consumes and
    /// returns self. Pass `None` to use the base prompt unmodified.
    pub fn with_project_instructions(mut self, instructions: Option<String>) -> Self {
        self.project_instructions = instructions;
        self
    }

    pub fn supports_structured_tool_protocol(&self) -> bool {
        matches!(self.tool_call_mode, ToolCallMode::Structured)
            && !matches!(self.tool_policy, ToolPolicy::Chat)
    }

    pub fn tool_policy(&self) -> ToolPolicy {
        self.tool_policy
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
        // Local discovery and explicit protocol overrides pin a concrete wire
        // format for the session. Prefer that route once it is known.
        if let Some(native) = self.server_info().and_then(|si| si.native_protocol) {
            return match native {
                ModelProtocol::MessagesV1 => ApiProtocol::MessagesV1,
                ModelProtocol::ChatCompat => ApiProtocol::ChatCompat,
            };
        }
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

    pub async fn create_stream(&self, messages: &[ApiMessage]) -> Result<EventStream> {
        #[cfg(test)]
        {
            if let Some(producer) = &self.mock_stream_producer {
                return producer.create_mock_stream(messages);
            }
        }

        let request_url = self.request_url();
        let server_n_ctx = self.server_info().map(|si| si.n_ctx).unwrap_or(0);
        let max_tokens = resolve_max_tokens(self.max_tokens, server_n_ctx);
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
                        tool_definitions_for_policy(self.tool_policy, &self.extra_tool_definitions),
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
                if self.is_local_endpoint() {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    apply_local_chat_compat_stream_flags(payload_object);
                }
                if self.supports_structured_tool_protocol() {
                    let payload_object = payload
                        .as_object_mut()
                        .expect("payload must be a JSON object");
                    payload_object.insert("tool_choice".to_string(), json!("auto"));
                    payload_object.insert(
                        "tools".to_string(),
                        tool_definitions_chat_compat_for_policy(
                            self.tool_policy,
                            &self.extra_tool_definitions,
                        ),
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

        if debug_payload_enabled() {
            emit_debug_payload(&request_url, &payload);
        }

        if self.tls_verification_disabled {
            tracing::debug!(
                target: "vex::tls",
                url = %request_url,
                "sending request with certificate verification disabled"
            );
        }

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        // Signal the expected SSE dialect so servers that multiplex multiple
        // streaming formats can select the right one (mirrors the probe in
        // protocol_discovery.rs).
        let accept_value = match api_protocol {
            ApiProtocol::MessagesV1 => "application/vnd.block-delta+sse",
            ApiProtocol::ChatCompat => "application/vnd.choices-delta+sse",
        };
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static(accept_value),
        );
        headers.insert(
            reqwest::header::ACCEPT_ENCODING,
            reqwest::header::HeaderValue::from_static("identity"),
        );
        headers.insert(
            reqwest::header::CACHE_CONTROL,
            reqwest::header::HeaderValue::from_static("no-store"),
        );

        // Apply operator-supplied headers. Reserved headers are excluded to
        // prevent duplicates so auth headers are only set once in the block below.
        for (name, value) in &self.model_headers {
            if is_reserved_header(name.as_str()) {
                tracing::warn!(header = %name, "ignoring reserved model header override");
                continue;
            }
            headers.insert(name.clone(), value.clone());
        }

        // Auth headers set last and exclusively. x-api-key and authorization
        // are in the reserved list above, so they cannot arrive here duplicated.
        match api_protocol {
            ApiProtocol::MessagesV1 => {
                if let Some(api_key) = &self.api_key {
                    headers.insert(
                        reqwest::header::HeaderName::from_static("x-api-key"),
                        reqwest::header::HeaderValue::from_str(api_key)
                            .map_err(|error| anyhow!("invalid x-api-key header: {error}"))?,
                    );
                }
            }
            ApiProtocol::ChatCompat => {
                if let Some(api_key) = &self.api_key {
                    let bearer = format!("Bearer {api_key}");
                    headers.insert(
                        reqwest::header::AUTHORIZATION,
                        reqwest::header::HeaderValue::from_str(&bearer)
                            .map_err(|error| anyhow!("invalid authorization header: {error}"))?,
                    );
                }
            }
        }

        create_event_stream(self.http.clone(), &request_url, &payload, &headers).await
    }

    fn request_url(&self) -> String {
        match self.api_protocol() {
            ApiProtocol::MessagesV1 => adapt_to_messages_v1_url(&self.api_url),
            ApiProtocol::ChatCompat => adapt_to_chat_compat_url(&self.api_url),
        }
    }
}

fn configured_api_client_base_url(config: &Config) -> Option<String> {
    let base = config.api_client.base_url.trim();
    if base.is_empty() {
        None
    } else {
        Some(base.trim_end_matches('/').to_string())
    }
}

fn protocol_variant_to_model_protocol(protocol: ProtocolVariant) -> ModelProtocol {
    match protocol {
        ProtocolVariant::BlockDelta => ModelProtocol::MessagesV1,
        ProtocolVariant::ChoicesDelta => ModelProtocol::ChatCompat,
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

    async fn create_stream(&self, messages: &[ApiMessage]) -> Result<EventStream> {
        self.create_stream(messages).await
    }
}

pub(crate) fn map_api_request_error(error: reqwest::Error, request_url: &str) -> anyhow::Error {
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
/// Also detects 429 rate-limit responses and extracts retry hints from
/// both the `Retry-After` header and the response body text.
pub(crate) fn map_api_status_error(
    status: reqwest::StatusCode,
    body: &str,
    request_url: &str,
    retry_after_header: Option<&str>,
) -> anyhow::Error {
    let local_http_hint = local_plain_http_hint(request_url);
    let is_local = is_local_endpoint_url(request_url);

    // Detect 429 rate-limit responses.
    if status.as_u16() == 429
        || (status.is_client_error() && crate::runtime::rate_limit::looks_like_rate_limit(body))
    {
        // Prefer the Retry-After header; fall back to body text hints.
        let retry_hint = retry_after_header
            .and_then(crate::runtime::rate_limit::parse_retry_after_header)
            .or_else(|| crate::runtime::rate_limit::parse_retry_from_body(body));
        let delay_msg = match retry_hint {
            Some(hint) => format!(
                " Retry suggested after {:.1}s.",
                hint.delay_ms as f64 / 1000.0
            ),
            None => String::new(),
        };
        return anyhow!(
            "API endpoint '{}' returned HTTP {} (rate limited).{}\n  Server message: {}",
            request_url,
            status.as_u16(),
            delay_msg,
            body.chars().take(300).collect::<String>()
        );
    }

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

fn resolve_max_tokens(default_max_tokens: u32, server_n_ctx: u32) -> u32 {
    let base = if let Some(value) = std::env::var("VEX_MAX_TOKENS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
    {
        value
    } else {
        default_max_tokens
    };
    // When server context is known, cap at 75% of n_ctx to leave room for
    // the prompt. When unknown (0), use a generous default ceiling.
    let ceiling = if server_n_ctx > 0 {
        // Multiply in u64 to keep the calculation in integer arithmetic and
        // avoid an unnecessary float conversion for the 75% ceiling.
        ((server_n_ctx as u64 * 3) / 4).min(u32::MAX as u64) as u32
    } else {
        16384
    };
    // Guard against ceiling < 128 (server reports n_ctx < 171) which would
    // panic in clamp because min > max is not permitted. Preserve `base` as
    // the upper bound even in this small-ceiling fallback.
    if ceiling < 128 {
        base.min(ceiling)
    } else {
        base.clamp(128, ceiling)
    }
}

fn infer_api_protocol(api_url: &str) -> ApiProtocol {
    let normalized = api_url.trim().to_ascii_lowercase();
    if normalized.contains("/chat/completions") {
        ApiProtocol::ChatCompat
    } else if normalized.contains("/messages") {
        // Covers both "/v1/messages" and the transposed "/messages/v1".
        ApiProtocol::MessagesV1
    } else if normalized.ends_with("/v1") {
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
            | "keep-alive"
            | "te"
            | "trailer"
            | "upgrade"
            | "proxy-authenticate"
            | "proxy-authorization"
    )
}

fn adapt_to_chat_compat_url(api_url: &str) -> String {
    let normalized = api_url.trim_end_matches('/');
    if normalized.ends_with("/chat/completions") {
        return normalized.to_string();
    }
    // Detect "/messages/v1" as a transposed variant of "/v1/messages".
    if let Some(prefix) = normalized.strip_suffix("/messages/v1") {
        return format!("{prefix}/v1/chat/completions");
    }
    if let Some(prefix) = normalized.strip_suffix("/messages") {
        return format!("{prefix}/chat/completions");
    }
    if normalized.ends_with("/v1") {
        return format!("{normalized}/chat/completions");
    }
    format!("{normalized}/v1/chat/completions")
}

fn adapt_to_messages_v1_url(api_url: &str) -> String {
    let normalized = api_url.trim_end_matches('/');
    if normalized.ends_with("/messages") {
        return normalized.to_string();
    }
    // Detect "/messages/v1" as a transposed variant of "/v1/messages".
    if let Some(prefix) = normalized.strip_suffix("/messages/v1") {
        return format!("{prefix}/v1/messages");
    }
    if let Some(prefix) = normalized.strip_suffix("/chat/completions") {
        return format!("{prefix}/messages");
    }
    if normalized.ends_with("/v1") {
        return format!("{normalized}/messages");
    }
    format!("{normalized}/v1/messages")
}

fn chat_compat_messages(messages: &[ApiMessage], system_prompt: &str) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len().saturating_add(1));
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
                    | ContentBlock::ThinkingData { .. }
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
                    | ContentBlock::ThinkingData { .. }
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

pub mod protocol_discovery;

mod tools;
pub(crate) use tools::{builtin_tool_summaries, is_readonly_tool};
#[cfg(test)]
use tools::{tool_definitions, tool_definitions_chat_compat_with_extra};
use tools::{tool_definitions_chat_compat_for_policy, tool_definitions_for_policy};

fn apply_local_chat_compat_stream_flags(payload_object: &mut serde_json::Map<String, Value>) {
    payload_object.insert("return_progress".to_string(), json!(true));
    payload_object.insert("timings_per_token".to_string(), json!(true));
    // Enable prompt caching so the server can reuse KV-cache across turns
    // and batch prompt evaluation instead of processing one token at a time.
    payload_object.insert("cache_prompt".to_string(), json!(true));
}

#[cfg(test)]
mod tests;
