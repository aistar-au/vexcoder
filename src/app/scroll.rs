use super::*;

#[cfg(test)]
use crate::ui::render::{input_visual_rows, MAX_INPUT_PANE_ROWS};
#[cfg(test)]
use std::time::{Duration, Instant};

impl TuiMode {
    /// Whether the transcript is pinned to the bottom (auto-following new
    /// content).  This is the single source of truth for follow-mode across
    /// the entire scroll subsystem — `transcript_scroll_offset == 0` means
    /// the viewport is at the live edge.
    pub fn auto_follow(&self) -> bool {
        self.transcript_scroll_offset == 0
    }

    fn clamp_transcript_scroll_offset(&mut self, total_rows: usize) {
        self.transcript_scroll_offset = self
            .transcript_scroll_offset
            .min(total_rows.saturating_sub(1));
    }

    fn clamp_inspector_scroll_offset(&mut self, total_rows: usize) {
        self.inspector_scroll_offset = self
            .inspector_scroll_offset
            .min(total_rows.saturating_sub(1));
    }

    pub(super) fn preserve_transcript_scroll_on_growth(&mut self, previous_expanded_rows: usize) {
        // When the user is following the bottom (offset=0), no adjustment
        // needed — new content automatically appears at the bottom.
        if self.transcript_scroll_offset == 0 {
            return;
        }

        let (_, rows, anchor) = self.task_output_view();
        let cols = self.history_content_width.get() as u16;
        let expanded = crate::ui::render::expand_rows_for_display(&rows, cols).len();
        if anchor == OutputScrollAnchor::Bottom && expanded > previous_expanded_rows {
            let growth = expanded - previous_expanded_rows;
            self.transcript_scroll_offset = self.transcript_scroll_offset.saturating_add(growth);
        }
        // Always clamp to prevent the offset from exceeding the scrollable
        // range — this also handles the case where rows were removed (e.g.
        // pending tool paragraph replacement).
        let max_offset = expanded.saturating_sub(1);
        self.transcript_scroll_offset = self.transcript_scroll_offset.min(max_offset);
    }

    /// Compute the total number of word-wrapped display rows for the current
    /// task output. Used to capture a pre-mutation snapshot that
    /// `preserve_transcript_scroll_on_growth` can compare against.
    pub(super) fn expanded_output_row_count(&self) -> usize {
        let (_, rows, _) = self.task_output_view();
        let cols = self.history_content_width.get() as u16;
        crate::ui::render::expand_rows_for_display(&rows, cols).len()
    }

    /// Append `delta` text to the current stream segment, creating a new
    /// segment if `active_stream_segment_index` is `None`.  Returns the
    /// segment index that was written to.
    pub(super) fn append_stream_segment_delta(&mut self, delta: &str) -> usize {
        if let Some(seg_idx) = self.active_stream_segment_index {
            if seg_idx < self.current_turn_stream_segments.len() {
                self.current_turn_stream_segments[seg_idx]
                    .text
                    .push_str(delta);
                return seg_idx;
            }
        }
        self.current_turn_stream_segments
            .push(StreamedResponseSegment {
                text: delta.to_owned(),
            });
        let idx = self.current_turn_stream_segments.len() - 1;
        self.active_stream_segment_index = Some(idx);
        idx
    }

    pub(super) fn push_history_line(&mut self, line: String) {
        if self.structured_streaming_active && self.history_state.turn_in_progress {
            self.materialize_current_turn_stream_segments();
        }
        self.history_state.lines.push(line);
        self.enforce_history_cap();
    }

    pub(super) fn enforce_history_cap(&mut self) {
        let cap = self.history_line_cap;
        if self.history_state.lines.len() <= cap {
            return;
        }

        let excess = self.history_state.lines.len() - cap;
        self.history_state.lines.drain(..excess);
        self.history_state.active_assistant_index = self
            .history_state
            .active_assistant_index
            .and_then(|idx| idx.checked_sub(excess));
    }

    /// Clamp the transcript scroll offset to a valid range after content
    /// mutations (line removals, replacements, cap enforcement).
    pub(super) fn clamp_transcript_after_mutation(&mut self) {
        let (_, rows, anchor) = self.task_output_view();
        let cols = self.history_content_width.get() as u16;
        let total_rows = crate::ui::render::expand_rows_for_display(&rows, cols).len();
        match anchor {
            OutputScrollAnchor::Bottom => self.clamp_transcript_scroll_offset(total_rows),
            OutputScrollAnchor::Top => self.clamp_inspector_scroll_offset(total_rows),
        }
    }

    /// Move the selected timeline entry up by one step.
    pub(super) fn apply_timeline_up(&mut self) {
        self.selected_timeline_index = self.selected_timeline_index.saturating_sub(1);
        self.timeline_follow_mode = false;
        self.inspector_scroll_offset = 0;
    }

    /// Move the selected timeline entry down by one step, clamped to the
    /// total number of available entries.
    pub(super) fn apply_timeline_down(&mut self, total_entries: usize) {
        let max = total_entries.saturating_sub(1);
        self.selected_timeline_index = (self.selected_timeline_index + 1).min(max);
        self.timeline_follow_mode = self.selected_timeline_index >= max;
        self.inspector_scroll_offset = 0;
    }

    /// Jump to the first timeline entry.
    pub(super) fn apply_timeline_home(&mut self) {
        self.selected_timeline_index = 0;
        self.timeline_follow_mode = false;
        self.inspector_scroll_offset = 0;
    }

    /// Jump to the last timeline entry.
    pub(super) fn apply_timeline_end(&mut self, total_entries: usize) {
        self.selected_timeline_index = total_entries.saturating_sub(1);
        self.timeline_follow_mode = true;
        self.inspector_scroll_offset = 0;
    }

    /// Dispatch a scroll action to the timeline selection.
    pub(super) fn apply_timeline_scroll_action(
        &mut self,
        action: ScrollAction,
        total_entries: usize,
    ) {
        match action {
            ScrollAction::LineUp => self.apply_timeline_up(),
            ScrollAction::LineDown => self.apply_timeline_down(total_entries),
            ScrollAction::PageUp(step) => {
                self.selected_timeline_index =
                    self.selected_timeline_index.saturating_sub(step.max(1));
                self.timeline_follow_mode = false;
                self.inspector_scroll_offset = 0;
            }
            ScrollAction::PageDown(step) => {
                let max = total_entries.saturating_sub(1);
                self.selected_timeline_index = self
                    .selected_timeline_index
                    .saturating_add(step.max(1))
                    .min(max);
                self.timeline_follow_mode = self.selected_timeline_index >= max;
                self.inspector_scroll_offset = 0;
            }
            ScrollAction::Home => self.apply_timeline_home(),
            ScrollAction::End => self.apply_timeline_end(total_entries),
        }
    }

    pub(super) fn apply_output_scroll_action(&mut self, action: ScrollAction) {
        let (_, rows, anchor) = self.task_output_view();
        // Use the expanded (word-wrapped) row count so the scroll range
        // matches the display row count used by the draw path.
        let cols = self.history_content_width.get() as u16;
        let total_rows = crate::ui::render::expand_rows_for_display(&rows, cols).len();

        match anchor {
            // Bottom-anchored view uses inverted semantics: LineUp scrolls
            // the offset upward (increasing the distance from the bottom),
            // while LineDown scrolls it back toward the bottom.
            OutputScrollAnchor::Bottom => match action {
                ScrollAction::LineUp => {
                    self.transcript_scroll_offset = self.transcript_scroll_offset.saturating_add(1);
                }
                ScrollAction::LineDown => {
                    self.transcript_scroll_offset = self.transcript_scroll_offset.saturating_sub(1);
                }
                ScrollAction::PageUp(step) => {
                    self.transcript_scroll_offset =
                        self.transcript_scroll_offset.saturating_add(step.max(1));
                }
                ScrollAction::PageDown(step) => {
                    self.transcript_scroll_offset =
                        self.transcript_scroll_offset.saturating_sub(step.max(1));
                }
                ScrollAction::Home => {
                    self.transcript_scroll_offset = total_rows.saturating_sub(1);
                }
                ScrollAction::End => {
                    self.transcript_scroll_offset = 0;
                }
            },
            OutputScrollAnchor::Top => {
                apply_bounded_scroll(
                    &mut self.inspector_scroll_offset,
                    action,
                    total_rows.saturating_sub(1),
                );
            }
        }

        match anchor {
            OutputScrollAnchor::Bottom => {
                self.clamp_transcript_scroll_offset(total_rows);
            }
            OutputScrollAnchor::Top => self.clamp_inspector_scroll_offset(total_rows),
        }
    }
}

/// Apply a [`ScrollAction`] to a bounded offset, clamping between 0 and `max`.
/// Used by patch overlay and inspector scrolling.
pub(crate) fn apply_bounded_scroll(offset: &mut usize, action: ScrollAction, max: usize) {
    *offset = match action {
        ScrollAction::LineUp => offset.saturating_sub(1),
        ScrollAction::LineDown => offset.saturating_add(1).min(max),
        ScrollAction::PageUp(step) => offset.saturating_sub(step.max(1)),
        ScrollAction::PageDown(step) => offset.saturating_add(step.max(1)).min(max),
        ScrollAction::Home => 0,
        ScrollAction::End => max,
    };
}

#[cfg(test)]
pub(super) fn input_rows_for_buffer(input: &str, width: usize) -> u16 {
    input_visual_rows(input, width).clamp(1, MAX_INPUT_PANE_ROWS) as u16
}

#[cfg(test)]
pub(super) struct RenderGuard {
    dirty: bool,
    cursor_tick: Duration,
    status_tick: Duration,
    last_draw_at: Instant,
    last_render_state_hash: Option<u64>,
}

#[cfg(test)]
impl RenderGuard {
    pub(super) fn with_intervals(
        cursor_tick: Duration,
        status_tick: Duration,
        now: Instant,
    ) -> Self {
        Self {
            dirty: true,
            cursor_tick,
            status_tick,
            last_draw_at: now,
            last_render_state_hash: None,
        }
    }

    pub(super) fn poll_timeout(&self) -> Duration {
        self.cursor_tick.min(self.status_tick)
    }

    pub(super) fn should_draw(&mut self, now: Instant, state_hash: u64) -> bool {
        if self.last_render_state_hash != Some(state_hash) {
            self.dirty = true;
        }

        if self.dirty || now.saturating_duration_since(self.last_draw_at) >= self.poll_timeout() {
            self.dirty = false;
            self.last_draw_at = now;
            self.last_render_state_hash = Some(state_hash);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_bounded_scroll_clamps_to_max() {
        let mut offset = 3usize;
        apply_bounded_scroll(&mut offset, ScrollAction::LineDown, 5);
        assert_eq!(offset, 4);
        apply_bounded_scroll(&mut offset, ScrollAction::PageDown(10), 5);
        assert_eq!(offset, 5);
        apply_bounded_scroll(&mut offset, ScrollAction::LineDown, 5);
        assert_eq!(offset, 5); // clamped at max
        apply_bounded_scroll(&mut offset, ScrollAction::Home, 5);
        assert_eq!(offset, 0);
        apply_bounded_scroll(&mut offset, ScrollAction::End, 5);
        assert_eq!(offset, 5);
    }
}
