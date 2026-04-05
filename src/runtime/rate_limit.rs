//! Rate-limit and Retry-After extraction using `regex-lite`.
//!
//! Parses retry delay hints from HTTP `Retry-After` header values and
//! from error response body text (e.g. "try again in 5 seconds").
//! All patterns are ASCII-only.
//!
//! Design boundary: this module extracts *structured delay hints* from
//! unstructured error text.  HTTP status code routing (e.g. 429 dispatch)
//! lives in `crate::api::client`.

use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A parsed retry delay hint.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryHint {
    /// Suggested delay in milliseconds before retrying.
    pub delay_ms: u64,
    /// Where the hint was extracted from.
    pub source: RetryHintSource,
}

/// Origin of a retry hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryHintSource {
    /// Extracted from a `Retry-After` header value (integer seconds).
    Header,
    /// Extracted from an error response body.
    Body,
}

// ---------------------------------------------------------------------------
// Regex compilation
// ---------------------------------------------------------------------------

/// Matches a plain integer or decimal in a `Retry-After` header value.
fn re_retry_after_header() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"^(\d+(?:\.\d+)?)$").unwrap())
}

/// Matches body text like "try again in 5 seconds", "retry in 500ms", etc.
fn re_retry_body() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(
            r"(?i)(?:try again|retry|wait)\s+(?:in\s+)?(\d+(?:\.\d+)?)\s*(s|ms|sec|seconds?|milliseconds?)",
        )
        .unwrap()
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract a retry delay from a `Retry-After` header value.
///
/// Handles the simple numeric-seconds form.  Does not handle HTTP-date
/// values (RFC 7231 §7.1.3).
pub fn parse_retry_after_header(value: &str) -> Option<RetryHint> {
    let re = re_retry_after_header();
    let caps = re.captures(value.trim())?;
    let seconds: f64 = caps[1].parse().ok()?;
    Some(RetryHint {
        delay_ms: (seconds * 1000.0) as u64,
        source: RetryHintSource::Header,
    })
}

/// Extract a retry delay from an error response body.
///
/// Scans for natural-language phrases like "try again in 5 seconds",
/// "retry in 500ms", "wait 30 sec", etc.
pub fn parse_retry_from_body(body: &str) -> Option<RetryHint> {
    let re = re_retry_body();
    let caps = re.captures(body)?;
    let value: f64 = caps[1].parse().ok()?;
    let unit = caps[2].to_ascii_lowercase();
    let delay_ms = if unit.starts_with("ms") || unit.starts_with("milli") {
        value as u64
    } else {
        (value * 1000.0) as u64
    };
    Some(RetryHint {
        delay_ms,
        source: RetryHintSource::Body,
    })
}

/// Returns `true` if the error body contains rate-limit language.
pub fn looks_like_rate_limit(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("throttl")
        || re_retry_body().is_match(body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_retry_after_header_integer() {
        let hint = parse_retry_after_header("5").unwrap();
        assert_eq!(hint.delay_ms, 5000);
        assert_eq!(hint.source, RetryHintSource::Header);
    }

    #[test]
    fn test_parse_retry_after_header_decimal() {
        let hint = parse_retry_after_header("1.5").unwrap();
        assert_eq!(hint.delay_ms, 1500);
    }

    #[test]
    fn test_parse_retry_after_header_with_whitespace() {
        let hint = parse_retry_after_header("  10  ").unwrap();
        assert_eq!(hint.delay_ms, 10_000);
    }

    #[test]
    fn test_parse_retry_after_header_invalid() {
        assert!(parse_retry_after_header("Thu, 01 Dec 2025 16:00:00 GMT").is_none());
        assert!(parse_retry_after_header("abc").is_none());
        assert!(parse_retry_after_header("").is_none());
    }

    #[test]
    fn test_parse_retry_from_body_seconds() {
        let hint = parse_retry_from_body("Rate limited: try again in 5 seconds").unwrap();
        assert_eq!(hint.delay_ms, 5000);
        assert_eq!(hint.source, RetryHintSource::Body);
    }

    #[test]
    fn test_parse_retry_from_body_milliseconds() {
        let hint = parse_retry_from_body("Please retry in 500ms").unwrap();
        assert_eq!(hint.delay_ms, 500);
    }

    #[test]
    fn test_parse_retry_from_body_decimal_seconds() {
        let hint = parse_retry_from_body("wait 1.5 seconds").unwrap();
        assert_eq!(hint.delay_ms, 1500);
    }

    #[test]
    fn test_parse_retry_from_body_sec_abbreviation() {
        let hint = parse_retry_from_body("try again in 30 sec").unwrap();
        assert_eq!(hint.delay_ms, 30_000);
    }

    #[test]
    fn test_parse_retry_from_body_no_match() {
        assert!(parse_retry_from_body("bad request").is_none());
        assert!(parse_retry_from_body("internal server error").is_none());
        assert!(parse_retry_from_body("").is_none());
    }

    #[test]
    fn test_looks_like_rate_limit_positive() {
        assert!(looks_like_rate_limit("Rate limit exceeded"));
        assert!(looks_like_rate_limit("429 Too Many Requests"));
        assert!(looks_like_rate_limit("Request throttled"));
        assert!(looks_like_rate_limit("try again in 5 seconds"));
    }

    #[test]
    fn test_looks_like_rate_limit_negative() {
        assert!(!looks_like_rate_limit("bad request"));
        assert!(!looks_like_rate_limit("internal server error"));
        assert!(!looks_like_rate_limit(""));
    }
}
