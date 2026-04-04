use std::time::Duration;

use crate::config::Config;
use crate::util::preferred_plain_http_url_for_local_endpoint;

pub const STARTUP_NOISE_GUARD: Duration = Duration::from_secs(15);

pub fn emit_model_endpoint_warnings(config: &Config) {
    if config.should_warn_about_model_tls_skip_check() {
        eprintln!(
            "[warning] model_url_skip_tls_check is enabled for {}; HTTPS certificate verification is bypassed for this launch",
            config.model_url
        );
    }
    if let Some(plain_url) = preferred_plain_http_url_for_local_endpoint(&config.model_url) {
        if config
            .model_url
            .to_ascii_lowercase()
            .starts_with("https://")
        {
            eprintln!(
                "[warning] local endpoint '{}' uses HTTPS; plain HTTP is required for local servers. Consider '{}'.",
                config.model_url, plain_url
            );
        }
    }
}

fn has_numbered_transcript_prefix(line: &str) -> bool {
    let mut saw_digit = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.peek() {
        if ch.is_ascii_digit() {
            saw_digit = true;
            chars.next();
            continue;
        }
        break;
    }
    saw_digit && chars.next() == Some(' ') && chars.next() == Some('|') && chars.next() == Some(' ')
}

fn transcript_signature_hits(text: &str) -> usize {
    let lower = text.to_ascii_lowercase();
    let signatures = [
        "mode:ready approval:",
        "view:scrolled",
        "view:following",
        "running tests/",
        "target/debug/deps/",
        "finished `dev` profile",
        "running `target/debug/vex`",
        "test result:",
        "[error] error sending request for url",
    ];
    signatures
        .iter()
        .filter(|pattern| lower.contains(*pattern))
        .count()
}

pub fn looks_like_terminal_transcript(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let signature_hits = transcript_signature_hits(trimmed);
    let numbered_lines = trimmed
        .lines()
        .take(64)
        .filter(|line| has_numbered_transcript_prefix(line))
        .count();

    signature_hits >= 2 || (signature_hits >= 1 && numbered_lines >= 2)
}

pub fn should_ignore_startup_paste_text(text: &str, within_startup_guard: bool) -> bool {
    text.contains('\u{1b}') || (within_startup_guard && looks_like_terminal_transcript(text))
}
