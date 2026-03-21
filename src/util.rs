use reqwest::Url;

/// Parse "true"/"false"/"1"/"0" from an owned String.
pub fn parse_bool_flag(s: String) -> Option<bool> {
    parse_bool_str(&s)
}

/// Parse "true"/"false"/"1"/"0" from a &str.
pub fn parse_bool_str(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Returns true for localhost, loopback IPv4/IPv6, and 0.0.0.0 URLs.
pub fn is_local_endpoint_url(url: &str) -> bool {
    let parsed = match Url::parse(url.trim()) {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };

    match parsed.host_str() {
        Some(host) => {
            let normalized = host.trim().to_ascii_lowercase();
            normalized == "localhost"
                || normalized == "::1"
                || normalized == "0.0.0.0"
                || normalized.starts_with("127.")
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
}
