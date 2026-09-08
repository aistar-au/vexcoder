//! BPE token counting for budget checks (ADR-051 Batch 1).
//!
//! Replaces the `len / 4` byte heuristic at the `session_notes` and
//! `project_instructions` call sites. Encoding lookup and the zero-allocation
//! `count` path follow the `tiktoken` crate API (`docs.rs/tiktoken`).

use tiktoken::CoreBpe;

/// Bundled vocabulary used when no model-specific encoding is selected.
/// Name is the crate encoding identifier from `tiktoken::get_encoding`.
const DEFAULT_ENCODING: &str = "o200k_base";

/// Cached encoder for the default vocabulary.
///
/// `tiktoken::get_encoding` already returns a `'static` instance; this helper
/// keeps the fallback encoding name in one place.
pub fn default_encoder() -> &'static CoreBpe {
    tiktoken::get_encoding(DEFAULT_ENCODING).expect("bundled vocabulary")
}

/// Look up an encoding for `model_name`, falling back to [`default_encoder`].
///
/// `tiktoken::encoding_for_model` returns `None` for unknown names
/// (`docs.rs/tiktoken`).
pub fn encoder_for_model(model_name: &str) -> &'static CoreBpe {
    tiktoken::encoding_for_model(model_name).unwrap_or_else(default_encoder)
}

/// Token count on the zero-allocation `CoreBpe::count` path.
pub fn token_count(text: &str) -> usize {
    default_encoder().count(text)
}

/// Token count using a caller-held encoder, for reuse across pulses.
pub fn token_count_with(encoding: &CoreBpe, text: &str) -> usize {
    encoding.count(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_count_empty_is_zero() {
        assert_eq!(token_count(""), 0);
    }

    #[test]
    fn token_count_is_not_byte_quarter_heuristic() {
        let text = "a";
        assert_eq!(
            text.len() / 4,
            0,
            "byte-quarter heuristic is zero for a single ASCII byte"
        );
        assert!(
            token_count(text) >= 1,
            "BPE count must count a single byte as at least one token"
        );
    }

    #[test]
    fn encoder_for_unknown_model_uses_default_encoding() {
        let encoder = encoder_for_model("not-a-known-model-id");
        assert_eq!(encoder.count("hi"), default_encoder().count("hi"));
    }

    #[test]
    fn token_count_with_matches_held_encoder() {
        let encoding = default_encoder();
        let text = "budget-check sample";
        assert_eq!(token_count_with(encoding, text), encoding.count(text));
    }
}
