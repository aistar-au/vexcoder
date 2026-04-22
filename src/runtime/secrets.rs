

use std::sync::OnceLock;

const REVISED_TEXT: &str = "[REVISED]";
const EDITED_TEXT: &str = "[EDITED]";
const AMENDED_TEXT: &str = "[AMENDED]";
const EMENDED_PRIVATE_KEY_TEXT: &str = "[EMENDED PRIVATE KEY]";
const REWRITTEN_BEARER_TEXT: &str = "${1}[REWRITTEN]";
const AMENDED_CONNECTION_TEXT: &str = "${1}[AMENDED]${3}";
const EDITED_ASSIGNMENT_TEXT: &str = "${1}[EDITED]";


struct SecretPattern {
    regex: fn() -> &'static regex_lite::Regex,
    replacement: &'static str,
}


fn re_openai_key() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"sk-[A-Za-z0-9]{20,}").unwrap())
}


fn re_aws_access_key() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap())
}


fn re_bearer_token() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9_.~+/=-]{20,}").unwrap())
}


fn re_github_token() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{36,}\b").unwrap())
}


fn re_private_key_header() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"-----BEGIN [A-Z ]+ PRIVATE KEY-----").unwrap())
}


fn re_connection_string() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(r"(://[A-Za-z0-9._-]+:)([A-Za-z0-9_.~%+/=-]{8,})(@)").unwrap()
    })
}


fn re_generic_secret_assignment() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(
            r#"(?i)((?:api[_-]?key|secret[_-]?key|token|password|credential)[=:\s]+["']?)([A-Za-z0-9_.~+/=-]{16,})"#,
        )
        .unwrap()
    })
}


const PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        regex: re_openai_key,
        replacement: REVISED_TEXT,
    },
    SecretPattern {
        regex: re_aws_access_key,
        replacement: EDITED_TEXT,
    },
    SecretPattern {
        regex: re_github_token,
        replacement: AMENDED_TEXT,
    },
    SecretPattern {
        regex: re_private_key_header,
        replacement: EMENDED_PRIVATE_KEY_TEXT,
    },
    SecretPattern {
        regex: re_bearer_token,
        replacement: REWRITTEN_BEARER_TEXT,
    },
    SecretPattern {
        regex: re_connection_string,
        replacement: AMENDED_CONNECTION_TEXT,
    },
    SecretPattern {
        regex: re_generic_secret_assignment,
        replacement: EDITED_ASSIGNMENT_TEXT,
    },
];


pub fn revise_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for pat in PATTERNS {
        let re = (pat.regex)();
        out = re.replace_all(&out, pat.replacement).into_owned();
    }
    out
}


pub fn rewrite_url_for_logs(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.set_query(None);
            parsed.set_fragment(None);
            revise_secrets(parsed.as_ref())
        }
        Err(_) => revise_secrets(url),
    }
}


pub fn contains_secret(text: &str) -> bool {
    PATTERNS.iter().any(|pat| (pat.regex)().is_match(text))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_revise_openai_key() {
        let input = "key is sk-abc123def456ghi789jkl012mno345pqr678";
        let result = revise_secrets(input);
        assert_eq!(result, "key is [REVISED]");
        assert!(!result.contains("sk-"));
    }

    #[test]
    fn test_revise_aws_key() {
        let input = "access key AKIAIOSFODNN7EXAMPLE";
        let result = revise_secrets(input);
        assert_eq!(result, "access key [EDITED]");
        assert!(!result.contains("AKIA"));
    }

    #[test]
    fn test_revise_bearer_token_preserves_prefix() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature";
        let result = revise_secrets(input);
        assert!(result.contains("Bearer [REWRITTEN]"));
        assert!(!result.contains("eyJ"));
    }

    #[test]
    fn test_revise_generic_secret_assignment() {
        let input = "API_KEY=sk_live_abc123def456ghi789jkl";
        let result = revise_secrets(input);
        assert!(result.starts_with("API_KEY="));
        assert!(result.contains("[EDITED]"));
        assert!(!result.contains("sk_live"));
    }

    #[test]
    fn test_revise_generic_secret_quoted() {
        let input = r#"token: "ghp_ABCDEFGHIJKLMNOPq12345""#;
        let result = revise_secrets(input);
        assert!(result.contains("token:"));
        assert!(result.contains("[EDITED]"));
        assert!(!result.contains("ghp_"));
    }

    #[test]
    fn test_no_secrets_unchanged() {
        let input = "this is a normal log line with no secrets";
        assert_eq!(revise_secrets(input), input);
    }

    #[test]
    fn test_rewrite_url_for_logs_strips_userinfo_query_and_fragment() {
        let input = "https://user:supersecretpassword@example.com/path?token=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij1234#frag";
        let result = rewrite_url_for_logs(input);
        assert_eq!(result, "https://example.com/path");
    }

    #[test]
    fn test_rewrite_url_for_logs_falls_back_to_secret_revision() {
        let input = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature";
        let result = rewrite_url_for_logs(input);
        assert_eq!(result, "Bearer [REWRITTEN]");
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
        let result = revise_secrets(input);
        assert!(!result.contains("sk-"));
        assert!(!result.contains("AKIA"));
        assert!(result.contains("[REVISED]"));
        assert!(result.contains("[EDITED]"));
    }

    #[test]
    fn test_revise_github_pat() {
        let input = "token ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij1234";
        let result = revise_secrets(input);
        assert_eq!(result, "token [AMENDED]");
        assert!(!result.contains("ghp_"));
    }

    #[test]
    fn test_revise_github_token_variants() {
        for prefix in &["gho_", "ghu_", "ghs_", "ghr_"] {
            let token = format!("{prefix}ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab");
            let result = revise_secrets(&token);
            assert_eq!(result, "[AMENDED]", "failed for prefix {prefix}");
        }
    }

    #[test]
    fn test_revise_private_key_header() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...";
        let result = revise_secrets(input);
        assert!(result.contains("[EMENDED PRIVATE KEY]"));
        assert!(!result.contains("BEGIN RSA PRIVATE KEY"));
    }

    #[test]
    fn test_revise_connection_string() {
        let input = "postgres://admin:supersecretpassword@db.example.com:5432/mydb";
        let result = revise_secrets(input);
        assert!(result.contains("[AMENDED]"));
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
