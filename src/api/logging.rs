use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

const DEFAULT_API_LOG_FILENAME: &str = "vex-debug-payload.log";
const DEBUG_PAYLOAD_ENV: &str = "VEX_DEBUG_PAYLOAD";
const API_LOG_PATH_ENV: &str = "VEX_API_LOG_PATH";

pub fn debug_payload_enabled() -> bool {
    std::env::var(DEBUG_PAYLOAD_ENV)
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

pub fn emit_debug_payload(request_url: &str, payload: &Value) {
    emit_log_value(&json!({
        "event": "api.payload_request",
        "url": request_url,
        "payload_summary": summarize_debug_payload(payload),
    }));
}

pub fn emit_sse_parse_error(
    event_type: Option<&str>,
    json_data: &str,
    parse_error: &serde_json::Error,
) {
    emit_log_value(&json!({
        "event": "api.sse_parse_failed",
        "event_type": event_type.unwrap_or("<none>"),
        "error": parse_error.to_string(),
        "data_summary": summarize_sse_payload(json_data),
    }));
}

fn summarize_debug_payload(payload: &Value) -> Value {
    let message_roles = payload
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| summarize_message_roles(messages))
        .unwrap_or_default();
    let message_count = payload
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| messages.len())
        .unwrap_or(0);
    let redacted_message_chars = payload
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| messages.iter().map(redacted_value_chars).sum::<usize>())
        .unwrap_or(0);
    let tool_names = payload
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(extract_tool_name)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let stop_count = payload
        .get("stop")
        .and_then(Value::as_array)
        .map(|stop| stop.len())
        .unwrap_or(0);
    let mut top_level_keys = payload
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    top_level_keys.sort();

    json!({
        "model": payload.get("model").and_then(Value::as_str).unwrap_or("unknown"),
        "stream": payload.get("stream").and_then(Value::as_bool).unwrap_or(false),
        "top_level_keys": top_level_keys,
        "message_count": message_count,
        "message_roles": message_roles,
        "redacted_message_chars": redacted_message_chars,
        "system_prompt_present": payload.get("system").is_some(),
        "tool_count": tool_names.len(),
        "tool_names": tool_names,
        "stop_count": stop_count,
        "has_reasoning_effort": payload.get("reasoning_effort").is_some(),
        "max_tokens": payload.get("max_tokens").cloned().unwrap_or(Value::Null),
        "temperature": payload.get("temperature").cloned().unwrap_or(Value::Null),
        "top_p": payload.get("top_p").cloned().unwrap_or(Value::Null),
    })
}

fn summarize_message_roles(messages: &[Value]) -> serde_json::Map<String, Value> {
    let mut counts = serde_json::Map::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let next = counts.get(&role).and_then(Value::as_u64).unwrap_or(0) + 1;
        counts.insert(role, Value::from(next));
    }
    counts
}

fn redacted_value_chars(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Bool(_) | Value::Number(_) => 0,
        Value::String(text) => text.chars().count(),
        Value::Array(values) => values.iter().map(redacted_value_chars).sum(),
        Value::Object(map) => map.values().map(redacted_value_chars).sum(),
    }
}

fn extract_tool_name(tool: &Value) -> Option<String> {
    tool.get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

fn summarize_sse_payload(json_data: &str) -> Value {
    match serde_json::from_str::<Value>(json_data) {
        Ok(value) => {
            let mut top_level_keys = value
                .as_object()
                .map(|object| object.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            top_level_keys.sort();
            json!({
                "kind": "json",
                "top_level_keys": top_level_keys,
                "redacted_chars": redacted_value_chars(&value),
                "contains_tool_calls": json_data.contains("\"tool_calls\""),
            })
        }
        Err(_) => json!({
            "kind": "non_json",
            "bytes": json_data.len(),
            "contains_tool_calls": json_data.contains("\"tool_calls\""),
            "contains_tool_use": json_data.contains("\"tool_use\""),
        }),
    }
}

fn emit_log_message(message: &str) {
    if let Some(path) = resolve_log_path()
        && append_log_file(&path, message).is_ok()
    {
        return;
    }

    eprintln!("{message}");
}

fn emit_log_value(value: &Value) {
    let encoded = serde_json::to_string(value)
        .unwrap_or_else(|_| "{\"event\":\"api.log.encode_failed\"}".to_string());
    emit_log_message(&format!("{encoded}\n"));
}

fn resolve_log_path() -> Option<String> {
    std::env::var(API_LOG_PATH_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            if std::io::stderr().is_terminal() {
                Some(default_log_path().to_string_lossy().into_owned())
            } else {
                None
            }
        })
}

fn default_log_path() -> PathBuf {
    std::env::temp_dir().join(DEFAULT_API_LOG_FILENAME)
}

fn append_log_file(path: &str, message: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(message.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_payload_enabled_accepts_true_variants() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        crate::test_support::test_set_var(&_env_lock, DEBUG_PAYLOAD_ENV, "1");
        assert!(debug_payload_enabled());
        crate::test_support::test_set_var(&_env_lock, DEBUG_PAYLOAD_ENV, "TRUE");
        assert!(debug_payload_enabled());
        crate::test_support::test_remove_var(&_env_lock, DEBUG_PAYLOAD_ENV);
    }

    #[test]
    fn test_resolve_log_path_uses_api_log_path() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        crate::test_support::test_set_var(&_env_lock, API_LOG_PATH_ENV, "/tmp/test-api.log");
        assert_eq!(resolve_log_path().as_deref(), Some("/tmp/test-api.log"));
        crate::test_support::test_remove_var(&_env_lock, API_LOG_PATH_ENV);
    }

    #[test]
    fn test_default_log_path_uses_temp_dir() {
        assert_eq!(
            default_log_path(),
            std::env::temp_dir().join(DEFAULT_API_LOG_FILENAME)
        );
    }

    #[test]
    fn test_emit_debug_payload_redacts_prompt_text() {
        let env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _guard = crate::test_support::EnvRestore::capture(&env_lock, API_LOG_PATH_ENV);
        let temp = tempfile::NamedTempFile::new().unwrap();
        crate::test_support::test_set_var(
            &env_lock,
            API_LOG_PATH_ENV,
            temp.path().to_string_lossy().to_string(),
        );

        emit_debug_payload(
            "http://127.0.0.1:8000/v1/messages",
            &json!({
                "model": "local/test",
                "messages": [{
                    "role": "user",
                    "content": "super secret prompt text"
                }],
                "tools": [{"function": {"name": "read_file"}}],
                "stream": true,
            }),
        );

        let content = std::fs::read_to_string(temp.path()).unwrap();
        assert!(
            !content.contains("super secret prompt text"),
            "got: {content}"
        );
        assert!(content.contains("\"message_count\":1"), "got: {content}");
        assert!(
            content.contains("\"tool_names\":[\"read_file\"]"),
            "got: {content}"
        );
    }

    #[test]
    fn test_emit_sse_parse_error_redacts_event_data() {
        let env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _guard = crate::test_support::EnvRestore::capture(&env_lock, API_LOG_PATH_ENV);
        let temp = tempfile::NamedTempFile::new().unwrap();
        crate::test_support::test_set_var(
            &env_lock,
            API_LOG_PATH_ENV,
            temp.path().to_string_lossy().to_string(),
        );
        let parse_error = serde_json::from_str::<Value>("{").unwrap_err();

        emit_sse_parse_error(
            Some("content_block_delta"),
            "{\"tool_calls\":[{\"function\":{\"arguments\":\"super secret arguments\"}}]",
            &parse_error,
        );

        let content = std::fs::read_to_string(temp.path()).unwrap();
        assert!(
            !content.contains("super secret arguments"),
            "got: {content}"
        );
        assert!(
            content.contains("\"contains_tool_calls\":true"),
            "got: {content}"
        );
    }
}
