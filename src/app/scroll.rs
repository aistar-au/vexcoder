use super::*;

#[cfg(test)]
use crate::ui::render::{input_visual_rows, MAX_INPUT_PANE_ROWS};
#[cfg(test)]
use std::time::{Duration, Instant};

impl TuiMode {
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

    pub(super) fn preserve_transcript_scroll_on_growth(&mut self, previous_rows: usize) {
        if self.transcript_scroll_offset == 0 {
            return;
        }

        let (_, rows, anchor) = self.task_output_view();
        if anchor == OutputScrollAnchor::Bottom && rows.len() > previous_rows {
            self.transcript_scroll_offset = self
                .transcript_scroll_offset
                .saturating_add(rows.len() - previous_rows);
        }
        self.clamp_transcript_scroll_offset(rows.len());
    }

    pub(super) fn push_history_line(&mut self, line: String) {
        if self.structured_streaming_active && self.history_state.turn_in_progress {
            self.materialize_current_turn_stream_segments();
        }
        self.history_state.lines.push(line);
        self.enforce_history_cap();
        if self.history_state.auto_follow {
            self.set_scroll_to_bottom();
        } else {
            self.clamp_scroll_offset();
        }
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
        self.history_state.scroll_offset = self.history_state.scroll_offset.saturating_sub(excess);
        self.clamp_scroll_offset();
    }

    pub(super) fn max_scroll_offset(&self) -> usize {
        history_visual_line_count(&self.history_state.lines, self.history_content_width.get())
            .saturating_sub(1)
    }

    pub(super) fn set_scroll_to_bottom(&mut self) {
        self.history_state.scroll_offset = self.max_scroll_offset();
    }

    pub(super) fn clamp_scroll_offset(&mut self) {
        let max = self.max_scroll_offset();
        self.history_state.scroll_offset = self.history_state.scroll_offset.min(max);
    }

    /// Advance the transcript scroll to the bottom when auto-follow is on,
    /// or clamp to a valid offset when it is off.
    pub(super) fn apply_auto_follow_or_clamp(&mut self) {
        if self.history_state.auto_follow {
            self.set_scroll_to_bottom();
        } else {
            self.clamp_scroll_offset();
        }
    }

    pub(super) fn apply_page_up(&mut self, page_step: usize) {
        self.history_state.scroll_offset = self
            .history_state
            .scroll_offset
            .saturating_sub(page_step.max(1));
        self.history_state.auto_follow = false;
    }

    pub(super) fn apply_page_down(&mut self, page_step: usize) {
        let max = self.max_scroll_offset();
        self.history_state.scroll_offset = self
            .history_state
            .scroll_offset
            .saturating_add(page_step.max(1))
            .min(max);
        self.history_state.auto_follow = self.history_state.scroll_offset >= max;
    }

    pub(super) fn apply_home(&mut self) {
        self.history_state.scroll_offset = 0;
        self.history_state.auto_follow = false;
    }

    pub(super) fn apply_end(&mut self) {
        self.set_scroll_to_bottom();
        self.history_state.auto_follow = true;
    }

    pub(super) fn apply_history_scroll_action(&mut self, action: ScrollAction) {
        match action {
            ScrollAction::LineUp => self.apply_page_up(1),
            ScrollAction::LineDown => self.apply_page_down(1),
            ScrollAction::PageUp(step) => self.apply_page_up(step),
            ScrollAction::PageDown(step) => self.apply_page_down(step),
            ScrollAction::Home => self.apply_home(),
            ScrollAction::End => self.apply_end(),
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
        let total_rows = rows.len();

        match anchor {
            // Bottom-anchored view uses inverted semantics: LineUp scrolls
            // the offset upward (increasing the distance from the bottom),
            // while LineDown scrolls it back toward the bottom.  Do NOT use
            // `apply_bounded_scroll` here — the direction mapping is reversed.
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
            OutputScrollAnchor::Bottom => self.clamp_transcript_scroll_offset(total_rows),
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
