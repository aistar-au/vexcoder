//! Streaming block buffer for the structured transcript rendering path.
//!
//! `StreamingBlockBuffer` accumulates the live text of a single streaming
//! block and exposes it for the render path.  `bounded_incremental_suffix`
//! deduplicates cumulative backend updates so callers only process net-new
//! content.
//!
//! This module is foundational infrastructure for ADR-041.
//! Buffers are wired into TuiMode (model_update) and consumed by the
//! transcript display path to drive the streaming cursor.

/// Category of streaming block — matches `StreamBlock` variants but
/// is a lightweight copy-friendly discriminator for the draw layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptBlockKind {
    Thinking,
    ToolCall,
    ToolResult,
    FinalText,
}

/// Accumulates the full text of a single streaming block.
///
/// The buffer is keyed by block index in `TuiMode::delta_accumulators`
/// and queried by the render path to gate the streaming cursor and
/// provide an authoritative live-content indicator.
pub struct StreamingBlockBuffer {
    content: String,
    kind: TranscriptBlockKind,
}

impl StreamingBlockBuffer {
    pub fn new(kind: TranscriptBlockKind) -> Self {
        Self {
            content: String::new(),
            kind,
        }
    }

    /// Append incoming text to the accumulated block content.
    pub fn append_delta(&mut self, delta: &str) {
        self.content.push_str(delta);
    }

    /// Return the full accumulated content for this block.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Return the block kind for routing decisions in the render path.
    pub fn kind(&self) -> TranscriptBlockKind {
        self.kind
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
    fn buffer_accumulates_content() {
        let mut buf = StreamingBlockBuffer::new(TranscriptBlockKind::FinalText);
        buf.append_delta("line one\nline two\n");
        assert_eq!(buf.content(), "line one\nline two\n");
        assert_eq!(buf.kind(), TranscriptBlockKind::FinalText);
    }

    #[test]
    fn buffer_accumulates_partial_content() {
        let mut buf = StreamingBlockBuffer::new(TranscriptBlockKind::Thinking);
        buf.append_delta("partial");
        assert_eq!(buf.content(), "partial");
    }

    #[test]
    fn buffer_content_tracks_full_text() {
        let mut buf = StreamingBlockBuffer::new(TranscriptBlockKind::FinalText);
        buf.append_delta("hello ");
        buf.append_delta("world");
        assert_eq!(buf.content(), "hello world");
    }

    #[test]
    fn buffer_kind_reflects_block_type() {
        let tool_buf = StreamingBlockBuffer::new(TranscriptBlockKind::ToolCall);
        assert_eq!(tool_buf.kind(), TranscriptBlockKind::ToolCall);

        let result_buf = StreamingBlockBuffer::new(TranscriptBlockKind::ToolResult);
        assert_eq!(result_buf.kind(), TranscriptBlockKind::ToolResult);
    }
}
