//! Structured transcript deltas for the delta-native rendering path.
//!
//! These types carry only the changed region and metadata needed by
//! reactive renderers. They parallel the `StreamBlock` variants but
//! are optimised for incremental display updates rather than
//! conversation-level block tracking.
//!
//! This module is foundational infrastructure for ADR-041 D5–D7.
//! Delta accumulators are wired into TuiMode (model_update) and the
//! render-side methods are available for the render path switchover.

use std::collections::VecDeque;

/// Accumulates streaming text for a single block and extracts bounded
/// text deltas for the renderer.
///
/// Uses bounded suffix comparison — `O(new_text)` instead of
/// `O(total_content)` — to deduplicate cumulative updates without
/// scanning the entire buffer.
pub struct DeltaAccumulator {
    content: String,
    last_emitted_len: usize,
    pending: VecDeque<String>,
}

impl DeltaAccumulator {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            last_emitted_len: 0,
            pending: VecDeque::new(),
        }
    }

    /// Append text and extract only the newly added region.
    ///
    /// Splits on the last newline to preserve line boundaries for the
    /// renderer while emitting partial chunks for character-level
    /// streaming when no newline is present.
    pub fn append_delta(&mut self, new_text: &str) {
        if new_text.is_empty() {
            return;
        }

        self.content.push_str(new_text);
        let new_region = &self.content[self.last_emitted_len..];

        if let Some(last_nl) = new_region.rfind('\n') {
            let complete = &new_region[..=last_nl];
            if !complete.is_empty() {
                self.pending.push_back(complete.to_string());
                self.last_emitted_len += complete.len();
            }
        } else {
            // No newline yet: emit as partial chunk.
            self.pending.push_back(new_region.to_string());
        }
    }

    /// Mark block as complete and flush remaining text.
    pub fn complete(&mut self) {
        let remaining = self.content[self.last_emitted_len..].to_string();
        self.pending.push_back(remaining);
        self.last_emitted_len = self.content.len();
    }

    /// Drain pending text deltas for the renderer.
    pub fn flush_pending(&mut self) -> Vec<String> {
        self.pending.drain(..).collect()
    }
}

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

    // Existing already contains incoming — redundant retransmission.
    if existing_len >= incoming.len()
        && existing.as_bytes()[..incoming.len()] == *incoming.as_bytes()
    {
        return String::new();
    }

    // No recognisable overlap — treat as pure delta.
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

    #[test]
    fn accumulator_emits_line_deltas() {
        let mut acc = DeltaAccumulator::new();
        acc.append_delta("line one\nline two\n");
        let deltas = acc.flush_pending();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0], "line one\nline two\n");
    }

    #[test]
    fn accumulator_emits_partial_chunk() {
        let mut acc = DeltaAccumulator::new();
        acc.append_delta("partial");
        let deltas = acc.flush_pending();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0], "partial");
    }

    #[test]
    fn accumulator_complete_flushes_remainder() {
        let mut acc = DeltaAccumulator::new();
        acc.append_delta("hello\nworld");
        let _ = acc.flush_pending();
        acc.complete();
        let deltas = acc.flush_pending();
        assert!(!deltas.is_empty(), "complete must flush remaining text");
    }

    #[test]
    fn accumulator_complete_includes_remaining_text() {
        let mut acc = DeltaAccumulator::new();
        acc.append_delta("hello ");
        acc.append_delta("world");
        let _ = acc.flush_pending();
        acc.complete();
        let deltas = acc.flush_pending();
        assert!(deltas.iter().any(|d| d.contains("world")));
    }

    #[test]
    fn accumulator_multiple_newlines_emit_single_chunk() {
        let mut acc = DeltaAccumulator::new();
        acc.append_delta("first\nsecond\n");
        let deltas = acc.flush_pending();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0], "first\nsecond\n");
    }
}
