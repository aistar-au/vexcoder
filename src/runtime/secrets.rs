//! Secret sanitization using `regex-lite`.
//!
//! Scans text for common secret patterns (API keys, bearer tokens, AWS
//! credentials, GitHub PATs, PEM private key headers, connection strings,
//! and generic secret assignments) and replaces matches with a fixed
//! placeholder.  All patterns are ASCII-only -- `regex-lite`'s `\d`
//! and `\w` metaclasses match `[0-9]` and `[0-9A-Za-z_]` respectively.
//!
//! Design boundary: this module handles *output* sanitization (log lines,
//! debug traces, streamed assistant text).  Secret *resolution* from
//! config values lives in `crate::config::load::resolve`.

use std::sync::OnceLock;

const SANITIZED: &str = "[SANITIZED]";

/// A pattern entry with its compiled regex accessor and replacement template.
struct SecretPattern {
    regex: fn() -> &'static regex_lite::Regex,
    replacement: &'static str,
}

// ---------------------------------------------------------------------------
// Pattern registry — compile once via OnceLock
// ---------------------------------------------------------------------------

/// Vendor API key (sk-prefix style): `sk-` followed by 20+ alphanumeric characters.
fn re_openai_key() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"sk-[A-Za-z0-9]{20,}").unwrap())
}

/// AWS access key ID: `AKIA` followed by exactly 16 uppercase alphanumeric.
fn re_aws_access_key() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap())
}

/// Bearer token: preserves the `Bearer ` prefix while replacing the token value.
fn re_bearer_token() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9_.~+/=-]{20,}").unwrap())
}

/// GitHub personal access token: `ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`
/// followed by 36+ alphanumeric characters.
fn re_github_token() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{36,}\b").unwrap())
}

/// PEM private key header line. Replaces the entire key block opener so
/// downstream consumers never see even the algorithm identifier.
fn re_private_key_header() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"-----BEGIN [A-Z ]+ PRIVATE KEY-----").unwrap())
}

/// Connection string with embedded credentials:
/// `protocol://user:password@host` -- replaces the password portion.
fn re_connection_string() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(r"(://[A-Za-z0-9._-]+:)([A-Za-z0-9_.~%+/=-]{8,})(@)").unwrap()
    })
}

/// Generic secret assignment: `API_KEY=value`, `token: "value"`, etc.
/// Preserves the key name and punctuation while replacing only the secret value.
fn re_generic_secret_assignment() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(
            r#"(?i)((?:api[_-]?key|secret[_-]?key|token|password|credential)[=:\s]+["']?)([A-Za-z0-9_.~+/=-]{16,})"#,
        )
        .unwrap()
    })
}

/// Ordered pattern list.  Specific patterns come first to avoid the
/// generic pattern partially matching a structured secret.  Bearer,
/// generic-assignment, and connection-string patterns use capture-group
/// backreferences in their replacement templates so the surrounding
/// context is preserved.
const PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        regex: re_openai_key,
        replacement: SANITIZED,
    },
    SecretPattern {
        regex: re_aws_access_key,
        replacement: SANITIZED,
    },
    SecretPattern {
        regex: re_github_token,
        replacement: SANITIZED,
    },
    SecretPattern {
        regex: re_private_key_header,
        replacement: "[SANITIZED PRIVATE KEY]",
    },
    SecretPattern {
        regex: re_bearer_token,
        replacement: "${1}[SANITIZED]",
    },
    SecretPattern {
        regex: re_connection_string,
        replacement: "${1}[SANITIZED]${3}",
    },
    SecretPattern {
        regex: re_generic_secret_assignment,
        replacement: "${1}[SANITIZED]",
    },
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Sanitize all recognised secret patterns from `text`, replacing each match
/// with `[SANITIZED]` (or a context-preserving variant). Returns the
/// original string unmodified when no secrets are detected.
pub fn sanitize_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for pat in PATTERNS {
        let re = (pat.regex)();
        out = re.replace_all(&out, pat.replacement).into_owned();
    }
    out
}

/// Sanitize sensitive URL components before logging.
pub fn sanitize_url_for_logs(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.set_query(None);
            parsed.set_fragment(None);
            sanitize_secrets(parsed.as_ref())
        }
        Err(_) => sanitize_secrets(url),
    }
}

/// Returns `true` if `text` contains any recognised secret pattern.
pub fn contains_secret(text: &str) -> bool {
    PATTERNS.iter().any(|pat| (pat.regex)().is_match(text))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_openai_key() {
        let input = "key is sk-abc123def456ghi789jkl012mno345pqr678";
        let result = sanitize_secrets(input);
        assert_eq!(result, "key is [SANITIZED]");
        assert!(!result.contains("sk-"));
    }

    #[test]
    fn test_sanitize_aws_key() {
        let input = "access key AKIAIOSFODNN7EXAMPLE";
        let result = sanitize_secrets(input);
        assert_eq!(result, "access key [SANITIZED]");
        assert!(!result.contains("AKIA"));
    }

    #[test]
    fn test_sanitize_bearer_token_preserves_prefix() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature";
        let result = sanitize_secrets(input);
        assert!(result.contains("Bearer [SANITIZED]"));
        assert!(!result.contains("eyJ"));
    }

    #[test]
    fn test_sanitize_generic_secret_assignment() {
        let input = "API_KEY=sk_live_abc123def456ghi789jkl";
        let result = sanitize_secrets(input);
        assert!(result.starts_with("API_KEY="));
        assert!(result.contains("[SANITIZED]"));
        assert!(!result.contains("sk_live"));
    }

    #[test]
    fn test_sanitize_generic_secret_quoted() {
        let input = r#"token: "ghp_ABCDEFGHIJKLMNOPq12345""#;
        let result = sanitize_secrets(input);
        assert!(result.contains("token:"));
        assert!(result.contains("[SANITIZED]"));
        assert!(!result.contains("ghp_"));
    }

    #[test]
    fn test_no_secrets_unchanged() {
        let input = "this is a normal log line with no secrets";
        assert_eq!(sanitize_secrets(input), input);
    }

    #[test]
    fn test_sanitize_url_for_logs_strips_userinfo_query_and_fragment() {
        let input = "https://user:supersecretpassword@example.com/path?token=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij1234#frag";
        let result = sanitize_url_for_logs(input);
        assert_eq!(result, "https://example.com/path");
    }

    #[test]
    fn test_sanitize_url_for_logs_falls_back_to_secret_sanitization() {
        let input = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature";
        let result = sanitize_url_for_logs(input);
        assert_eq!(result, "Bearer [SANITIZED]");
    }

    #[test]
    fn test_contains_secret_positive() {
        assert!(contains_secret(
            "key sk-abc123def456ghi789jkl012mno345pqr678"
        ));
        assert!(contains_secret("AKIAIOSFODNN7EXAMPLE"));
        assert!(contains_secret("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6"));
    }

    #[test]
    fn test_contains_secret_negative() {
        assert!(!contains_secret("just normal text"));
        assert!(!contains_secret("sk-short"));
        assert!(!contains_secret("AKIA_too_short"));
    }

    #[test]
    fn test_multiple_secrets_in_one_string() {
        let input = "key=sk-abc123def456ghi789jkl012mno345pqr678 and also AKIAIOSFODNN7EXAMPLE";
        let result = sanitize_secrets(input);
        assert!(!result.contains("sk-"));
        assert!(!result.contains("AKIA"));
        assert_eq!(result.matches("[SANITIZED]").count(), 2);
    }

    #[test]
    fn test_sanitize_github_pat() {
        let input = "token ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij1234";
        let result = sanitize_secrets(input);
        assert_eq!(result, "token [SANITIZED]");
        assert!(!result.contains("ghp_"));
    }

    #[test]
    fn test_sanitize_github_token_variants() {
        for prefix in &["gho_", "ghu_", "ghs_", "ghr_"] {
            let token = format!("{prefix}ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab");
            let result = sanitize_secrets(&token);
            assert_eq!(result, "[SANITIZED]", "failed for prefix {prefix}");
        }
    }

    #[test]
    fn test_sanitize_private_key_header() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...";
        let result = sanitize_secrets(input);
        assert!(result.contains("[SANITIZED PRIVATE KEY]"));
        assert!(!result.contains("BEGIN RSA PRIVATE KEY"));
    }

    #[test]
    fn test_sanitize_connection_string() {
        let input = "postgres://admin:supersecretpassword@db.example.com:5432/mydb";
        let result = sanitize_secrets(input);
        assert!(result.contains("[SANITIZED]"));
        assert!(!result.contains("supersecretpassword"));
        assert!(result.contains("@db.example.com"));
    }

    #[test]
    fn test_contains_secret_github_token() {
        assert!(contains_secret(
            "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij1234"
        ));
    }

    #[test]
    fn test_contains_secret_private_key() {
        assert!(contains_secret("-----BEGIN EC PRIVATE KEY-----"));
    }
}
