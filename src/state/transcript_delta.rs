//! Suffix deduplication for cumulative streaming backend updates.
//!
//! `bounded_incremental_suffix` extracts only the net-new content from a
//! cumulative backend chunk, avoiding `O(existing.len())` rescans.
//!
//! This module is foundational infrastructure for ADR-041.

/// Bounded suffix deduplication for cumulative streaming updates.
///
/// When a backend sends cumulative text (the full content so far on
/// every chunk), this function extracts only the new suffix using a
/// bounded comparison window of `O(incoming.len())` rather than
/// `O(existing.len())`.
pub fn bounded_incremental_suffix(existing: &str, incoming: &str) -> String {
    if incoming.is_empty() {
        return String::new();
    }

    // Fast path: incoming is strictly longer and starts with existing.
    // Use bounded comparison: only check the prefix up to existing.len().
    let existing_len = existing.len();
    if incoming.len() > existing_len && incoming.as_bytes()[..existing_len] == *existing.as_bytes()
    {
        return incoming[existing_len..].to_string();
    }

    // Existing already contains incoming ΓÇö redundant retransmission.
    if existing_len >= incoming.len()
        && existing.as_bytes()[..incoming.len()] == *incoming.as_bytes()
    {
        return String::new();
    }

    // No recognisable overlap ΓÇö treat as pure delta.
    incoming.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_suffix_extracts_new_text() {
        assert_eq!(bounded_incremental_suffix("hello", "hello world"), " world");
    }

    #[test]
    fn bounded_suffix_empty_incoming() {
        assert_eq!(bounded_incremental_suffix("hello", ""), "");
    }

    #[test]
    fn bounded_suffix_redundant_retransmission() {
        assert_eq!(bounded_incremental_suffix("hello world", "hello"), "");
    }

    #[test]
    fn bounded_suffix_no_overlap() {
        assert_eq!(bounded_incremental_suffix("aaa", "bbb"), "bbb");
    }
}
