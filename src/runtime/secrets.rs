//! Secret redaction using `regex-lite`.
//!
//! Scans text for common secret patterns (API keys, bearer tokens, AWS
//! credentials, generic secret assignments) and replaces matches with a
//! fixed placeholder.  All patterns are ASCII-only — `regex-lite`'s `\d`
//! and `\w` metaclasses match `[0-9]` and `[0-9A-Za-z_]` respectively.
//!
//! Design boundary: this module handles *output* redaction (log lines,
//! debug traces, streamed assistant text).  Secret *resolution* from
//! config values lives in `crate::config::load::resolve`.

use std::sync::OnceLock;

const REDACTED: &str = "[REDACTED]";

/// A pattern entry with its compiled regex accessor and replacement template.
struct SecretPattern {
    regex: fn() -> &'static regex_lite::Regex,
    replacement: &'static str,
}

// ---------------------------------------------------------------------------
// Pattern registry — compile once via OnceLock
// ---------------------------------------------------------------------------

/// OpenAI-style API key: `sk-` followed by 20+ alphanumeric characters.
fn re_openai_key() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"sk-[A-Za-z0-9]{20,}").unwrap())
}

/// AWS access key ID: `AKIA` followed by exactly 16 uppercase alphanumeric.
fn re_aws_access_key() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap())
}

/// Bearer token: preserves the `Bearer ` prefix, redacts the token value.
fn re_bearer_token() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9_.~+/=-]{20,}").unwrap()
    })
}

/// Generic secret assignment: `API_KEY=value`, `token: "value"`, etc.
/// Preserves the key name and punctuation, redacts only the secret value.
fn re_generic_secret_assignment() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(
            r#"(?i)((?:api[_-]?key|secret[_-]?key|token|password|credential)[=:\s]+["']?)([A-Za-z0-9_.~+/=-]{16,})"#,
        )
        .unwrap()
    })
}

/// Ordered pattern list.  Bearer and generic-assignment patterns use
/// capture-group backreferences in their replacement templates so the
/// surrounding context (e.g. `Bearer `, `API_KEY=`) is preserved.
const PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        regex: re_openai_key,
        replacement: REDACTED,
    },
    SecretPattern {
        regex: re_aws_access_key,
        replacement: REDACTED,
    },
    SecretPattern {
        regex: re_bearer_token,
        replacement: "${1}[REDACTED]",
    },
    SecretPattern {
        regex: re_generic_secret_assignment,
        replacement: "${1}[REDACTED]",
    },
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Redact all recognised secret patterns from `text`, replacing each match
/// with `[REDACTED]` (or a context-preserving variant).  Returns the
/// original string unmodified when no secrets are detected.
pub fn redact_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for pat in PATTERNS {
        let re = (pat.regex)();
        out = re.replace_all(&out, pat.replacement).into_owned();
    }
    out
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
    fn test_redact_openai_key() {
        let input = "key is sk-abc123def456ghi789jkl012mno345pqr678";
        let result = redact_secrets(input);
        assert_eq!(result, "key is [REDACTED]");
        assert!(!result.contains("sk-"));
    }

    #[test]
    fn test_redact_aws_key() {
        let input = "access key AKIAIOSFODNN7EXAMPLE";
        let result = redact_secrets(input);
        assert_eq!(result, "access key [REDACTED]");
        assert!(!result.contains("AKIA"));
    }

    #[test]
    fn test_redact_bearer_token_preserves_prefix() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature";
        let result = redact_secrets(input);
        assert!(result.contains("Bearer [REDACTED]"));
        assert!(!result.contains("eyJ"));
    }

    #[test]
    fn test_redact_generic_secret_assignment() {
        let input = "API_KEY=sk_live_abc123def456ghi789jkl";
        let result = redact_secrets(input);
        assert!(result.starts_with("API_KEY="));
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("sk_live"));
    }

    #[test]
    fn test_redact_generic_secret_quoted() {
        let input = r#"token: "ghp_ABCDEFGHIJKLMNOPq12345""#;
        let result = redact_secrets(input);
        assert!(result.contains("token:"));
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("ghp_"));
    }

    #[test]
    fn test_no_secrets_unchanged() {
        let input = "this is a normal log line with no secrets";
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn test_contains_secret_positive() {
        assert!(contains_secret("key sk-abc123def456ghi789jkl012mno345pqr678"));
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
        let result = redact_secrets(input);
        assert!(!result.contains("sk-"));
        assert!(!result.contains("AKIA"));
        assert_eq!(result.matches("[REDACTED]").count(), 2);
    }
}
