//! Structured transcript deltas for the delta-native rendering path.
//!
//! These types carry only the changed region and metadata needed by
//! reactive renderers. They parallel the `StreamBlock` variants but
//! are optimised for incremental display updates rather than
//! conversation-level block tracking.
//!
//! This module is foundational infrastructure for ADR-041 D5ΓÇôD7.
//! Delta accumulators are wired into TuiMode (model_update) and the
//! draw-side methods are available for the render path switchover.

use std::collections::VecDeque;

/// Category of streaming block ΓÇö matches `StreamBlock` variants but
/// is a lightweight copy-friendly discriminator for the draw layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptBlockKind {
    Thinking,
    ToolCall,
    ToolResult,
    FinalText,
}

/// A single incremental update emitted during streaming.
///
/// Carries the text delta, completion flag, and block kind so that
/// the draw layer can apply minimal redraws without rebuilding
/// transcript state from prefix markers.
#[expect(
    dead_code,
    reason = "delta-native transcript payload remains staged until the ratatui surface consumes incremental block deltas directly"
)]
#[derive(Debug, Clone)]
pub struct TranscriptDelta {
    pub text: String,
    pub is_complete: bool,
    pub block_kind: TranscriptBlockKind,
}

/// Accumulates streaming text for a single block and extracts bounded
/// deltas for the renderer.
///
/// Uses bounded suffix comparison ΓÇö `O(new_text)` instead of
/// `O(total_content)` ΓÇö to deduplicate cumulative updates without
/// scanning the entire buffer.
pub struct DeltaAccumulator {
    content: String,
    last_emitted_len: usize,
    pending: VecDeque<TranscriptDelta>,
    block_kind: TranscriptBlockKind,
}

impl DeltaAccumulator {
    pub fn new(block_kind: TranscriptBlockKind) -> Self {
        Self {
            content: String::new(),
            last_emitted_len: 0,
            pending: VecDeque::new(),
            block_kind,
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
                self.pending.push_back(TranscriptDelta {
                    text: complete.to_string(),
                    is_complete: false,
                    block_kind: self.block_kind,
                });
                self.last_emitted_len += complete.len();
            }
        } else {
            // No newline yet: emit as partial chunk.
            self.pending.push_back(TranscriptDelta {
                text: new_region.to_string(),
                is_complete: false,
                block_kind: self.block_kind,
            });
        }
    }

    /// Mark block as complete and flush remaining text.
    pub fn complete(&mut self) {
        let remaining = &self.content[self.last_emitted_len..];
        self.pending.push_back(TranscriptDelta {
            text: remaining.to_string(),
            is_complete: true,
            block_kind: self.block_kind,
        });
        self.last_emitted_len = self.content.len();
    }

    /// Drain pending deltas for the renderer.
    pub fn flush_pending(&mut self) -> Vec<TranscriptDelta> {
        self.pending.drain(..).collect()
    }

    #[allow(unused)]
    pub fn content(&self) -> &str {
        &self.content
    }

    #[allow(unused)]
    pub fn set_block_kind(&mut self, kind: TranscriptBlockKind) {
        self.block_kind = kind;
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

    #[test]
    fn accumulator_emits_line_deltas() {
        let mut acc = DeltaAccumulator::new(TranscriptBlockKind::FinalText);
        acc.append_delta("line one\nline two\n");
        let deltas = acc.flush_pending();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].text, "line one\nline two\n");
        assert!(!deltas[0].is_complete);
    }

    #[test]
    fn accumulator_emits_partial_chunk() {
        let mut acc = DeltaAccumulator::new(TranscriptBlockKind::Thinking);
        acc.append_delta("partial");
        let deltas = acc.flush_pending();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].text, "partial");
    }

    #[test]
    fn accumulator_complete_flushes_remainder() {
        let mut acc = DeltaAccumulator::new(TranscriptBlockKind::ToolCall);
        acc.append_delta("hello\nworld");
        let _ = acc.flush_pending();
        acc.complete();
        let deltas = acc.flush_pending();
        assert!(deltas.iter().any(|d| d.is_complete));
    }

    #[test]
    fn set_block_kind_changes_delta_block_kind() {
        let mut acc = DeltaAccumulator::new(TranscriptBlockKind::Thinking);
        acc.set_block_kind(TranscriptBlockKind::FinalText);
        acc.append_delta("text");
        acc.complete();
        let deltas = acc.flush_pending();
        assert!(
            deltas
                .iter()
                .all(|d| d.block_kind == TranscriptBlockKind::FinalText),
            "block kind must reflect the set_block_kind update"
        );
    }

    #[test]
    fn accumulator_content_tracks_full_text() {
        let mut acc = DeltaAccumulator::new(TranscriptBlockKind::FinalText);
        acc.append_delta("hello ");
        acc.append_delta("world");
        assert_eq!(acc.content(), "hello world");
    }
}
