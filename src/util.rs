use anyhow::{Context, Result, anyhow};
use reqwest::Url;
use serde::Serialize;
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;

pub fn write_json_safe<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| {
        anyhow!(
            "missing parent directory for {} '{}'",
            label,
            path.display()
        )
    })?;
    std::fs::create_dir_all(dir).with_context(|| {
        format!(
            "failed to create directory for {}: {}",
            label,
            dir.display()
        )
    })?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid file name for {} '{}'", label, path.display()))?;
    let temp_path = dir.join(format!("{file_name}.tmp"));

    let json = serde_json::to_vec_pretty(value)
        .with_context(|| format!("failed to serialize {label}: {}", path.display()))?;
    let mut file = std::fs::File::create(&temp_path)
        .with_context(|| format!("failed to create temp {}: {}", label, temp_path.display()))?;
    file.write_all(&json)
        .with_context(|| format!("failed to write temp {}: {}", label, temp_path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to flush temp {}: {}", label, temp_path.display()))?;
    drop(file);

    std::fs::rename(&temp_path, path)
        .with_context(|| format!("failed to rename {} to: {}", label, path.display()))?;
    Ok(())
}

pub fn tool_definition_entry(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "input_schema": input_schema,
    })
}

pub fn parse_bool_flag(s: String) -> Option<bool> {
    parse_bool_str(&s)
}

pub fn parse_bool_str(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn is_local_endpoint_url(url: &str) -> bool {
    let parsed = match Url::parse(url.trim()) {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };

    match parsed.host_str() {
        Some(host) => {
            let normalized = host.trim().to_ascii_lowercase();
            if normalized == "localhost"
                || normalized == "::1"
                || normalized == "0.0.0.0"
                || normalized.starts_with("127.")
            {
                return true;
            }

            if let Ok(ip) = normalized.parse::<std::net::IpAddr>() {
                return match ip {
                    std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
                    std::net::IpAddr::V6(v6) => {
                        let seg = v6.segments();
                        (seg[0] & 0xffc0) == 0xfe80 || (seg[0] & 0xfe00) == 0xfc00
                    }
                };
            }
            false
        }
        None => false,
    }
}

pub fn preferred_plain_http_url_for_local_endpoint(url: &str) -> Option<String> {
    let parsed = Url::parse(url.trim()).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    if !is_local_endpoint_url(url) {
        return None;
    }

    let mut adapted = parsed;
    adapted.set_scheme("http").ok()?;
    Some(adapted.to_string())
}

pub fn preferred_loopback_url_for_local_endpoint(url: &str) -> Option<String> {
    let parsed = Url::parse(url.trim()).ok()?;
    if parsed.host_str()? != "0.0.0.0" {
        return None;
    }
    if !is_local_endpoint_url(url) {
        return None;
    }

    let mut adapted = parsed;
    adapted.set_host(Some("127.0.0.1")).ok()?;
    Some(adapted.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool_helpers() {
        assert_eq!(parse_bool_str("true"), Some(true));
        assert_eq!(parse_bool_str("0"), Some(false));
        assert_eq!(parse_bool_flag("YES".to_string()), Some(true));
        assert_eq!(parse_bool_flag("off".to_string()), Some(false));
        assert_eq!(parse_bool_str("maybe"), None);
    }

    #[test]
    fn test_is_local_endpoint_url_normalizes_case_and_space() {
        assert!(is_local_endpoint_url(" HTTP://LOCALHOST:8000/v1/messages "));
        assert!(is_local_endpoint_url("https://127.0.0.1/v1/messages"));
        assert!(is_local_endpoint_url("https://0.0.0.0/v1/messages"));
        assert!(!is_local_endpoint_url(
            "https://evil-localhost.com/v1/messages"
        ));
        assert!(!is_local_endpoint_url(
            "https://api.example.com/v1/messages"
        ));
    }

    #[test]
    fn test_is_local_endpoint_url_private_networks() {
        assert!(is_local_endpoint_url("http://192.168.1.100:11434/v1"));
        assert!(is_local_endpoint_url("http://10.0.0.5:8080/v1/messages"));
        assert!(is_local_endpoint_url("http://172.16.0.1:8000/v1"));
        assert!(is_local_endpoint_url("http://172.31.255.254:8000/v1"));

        assert!(!is_local_endpoint_url("http://172.32.0.1:8000/v1"));

        assert!(is_local_endpoint_url("http://169.254.1.1:8080/v1"));

        assert!(!is_local_endpoint_url("http://8.8.8.8:8080/v1"));
        assert!(!is_local_endpoint_url("http://203.0.113.1:8080/v1"));
    }

    #[test]
    fn test_preferred_plain_http_url_for_https_local_endpoint() {
        let adapted =
            preferred_plain_http_url_for_local_endpoint(" https://localhost:8000/v1/messages ");
        assert_eq!(
            adapted.as_deref(),
            Some("http://localhost:8000/v1/messages")
        );
    }

    #[test]
    fn test_preferred_plain_http_url_ignores_non_local_or_non_https_urls() {
        assert!(
            preferred_plain_http_url_for_local_endpoint("http://localhost:8000/v1/messages")
                .is_none()
        );
        assert!(
            preferred_plain_http_url_for_local_endpoint("https://api.example.com/v1/messages")
                .is_none()
        );
    }

    #[test]
    fn test_preferred_loopback_url_for_wildcard_local_endpoint() {
        let adapted =
            preferred_loopback_url_for_local_endpoint(" http://0.0.0.0:8000/v1/messages ");
        assert_eq!(
            adapted.as_deref(),
            Some("http://127.0.0.1:8000/v1/messages")
        );
    }

    #[test]
    fn test_preferred_loopback_url_ignores_non_wildcard_hosts() {
        assert!(
            preferred_loopback_url_for_local_endpoint("http://127.0.0.1:8000/v1/messages")
                .is_none()
        );
        assert!(
            preferred_loopback_url_for_local_endpoint("https://api.example.com/v1/messages")
                .is_none()
        );
    }
}
