use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::config::ApiTransport;
use crate::runtime::{ModelBackendKind, ModelProtocol, SandboxKind, ToolCallMode};

pub(crate) fn parse_model_backend(value: String) -> Option<ModelBackendKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "local-runtime" | "local_runtime" | "local" => Some(ModelBackendKind::LocalRuntime),
        "api-server" | "api_server" | "api" | "remote" => Some(ModelBackendKind::ApiServer),
        _ => None,
    }
}

pub(crate) fn parse_model_protocol(value: String) -> Option<ModelProtocol> {
    match value.trim().to_ascii_lowercase().as_str() {
        "messages-v1" | "messages_v1" | "messages" | "v1" => Some(ModelProtocol::MessagesV1),
        "chat-compat" | "chat_compat" | "chat" => Some(ModelProtocol::ChatCompat),
        _ => None,
    }
}

pub(crate) fn parse_tool_call_mode(value: String) -> Option<ToolCallMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "structured" => Some(ToolCallMode::Structured),
        "tagged-fallback" | "tagged_fallback" | "fallback" | "tagged" => {
            Some(ToolCallMode::TaggedFallback)
        }
        _ => None,
    }
}

pub(crate) fn parse_api_transport(value: String) -> Option<ApiTransport> {
    match value.trim().to_ascii_lowercase().as_str() {
        "http" => Some(ApiTransport::Http),
        "unix" => Some(ApiTransport::Unix),
        "both" => Some(ApiTransport::Both),
        _ => None,
    }
}

pub(crate) fn parse_sandbox_kind(value: String) -> Option<SandboxKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "passthrough" => Some(SandboxKind::Passthrough),
        "macos-exec" | "macos_exec" => Some(SandboxKind::MacosExec),
        "container" => Some(SandboxKind::Container),
        // Gap 36: Linux userspace sandbox via bubblewrap.
        "bubblewrap" | "bwrap" | "linux-bwrap" => Some(SandboxKind::Bubblewrap),
        _ => None,
    }
}

pub(crate) fn infer_model_protocol(_api_url: &str) -> ModelProtocol {
    // messages-v1 is always the default wire protocol regardless of the URL
    // path.  ChatCompat is selected only when the user explicitly sets
    // `model_protocol = "chat-compat"` in config, or when server discovery
    // proves the endpoint exclusively exposes /v1/chat/completions.  No
    // URL-based inference is needed: both protocols are always attempted at
    // session start via detect_native_protocol() for local endpoints.
    ModelProtocol::MessagesV1
}

pub(crate) fn parse_model_headers_json() -> Result<HeaderMap> {
    let raw = match std::env::var("VEX_MODEL_HEADERS_JSON") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(HeaderMap::new()),
    };
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("VEX_MODEL_HEADERS_JSON is not a valid JSON object: {e}"))?;
    let mut headers = HeaderMap::new();
    for (k, v) in &map {
        let name = HeaderName::from_bytes(k.as_bytes()).map_err(|e| {
            anyhow::anyhow!("VEX_MODEL_HEADERS_JSON invalid header name {k:?}: {e}")
        })?;
        let val_str = v.as_str().ok_or_else(|| {
            anyhow::anyhow!("VEX_MODEL_HEADERS_JSON value for {k:?} must be a string")
        })?;
        let value = HeaderValue::from_str(val_str).map_err(|e| {
            anyhow::anyhow!("VEX_MODEL_HEADERS_JSON invalid header value for {k:?}: {e}")
        })?;
        headers.insert(name, value);
    }
    Ok(headers)
}

pub(crate) fn legacy_messages_protocol_value() -> &'static str {
    concat!("anth", "ropic")
}

pub(crate) fn legacy_chat_protocol_value() -> &'static str {
    concat!("open", "ai")
}
