use super::*;

#[cfg(test)]
use std::time::{Duration, Instant};

impl TuiMode {
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
        if self.transcript_scroll_offset == 0 {
            return;
        }

        let (_, rows, anchor) = self.task_output_view();
        let expanded = rows.len();
        if anchor == OutputScrollAnchor::Bottom && expanded > previous_expanded_rows {
            let growth = expanded - previous_expanded_rows;
            self.transcript_scroll_offset = self.transcript_scroll_offset.saturating_add(growth);
        }
        let max_offset = expanded.saturating_sub(1);
        self.transcript_scroll_offset = self.transcript_scroll_offset.min(max_offset);
    }

    pub(super) fn expanded_output_row_count(&self) -> usize {
        let (_, rows, _) = self.task_output_view();
        rows.len()
    }

    pub(super) fn push_history_line(&mut self, line: String) {
        self.push_document_notice(line, crate::runtime::NoticeSeverity::Info);
    }

    pub(super) fn clamp_transcript_after_mutation(&mut self) {
        let (_, rows, anchor) = self.task_output_view();
        let total_rows = rows.len();
        match anchor {
            OutputScrollAnchor::Bottom => self.clamp_transcript_scroll_offset(total_rows),
            OutputScrollAnchor::Top => self.clamp_inspector_scroll_offset(total_rows),
        }
    }

    pub(super) fn apply_timeline_up(&mut self) {
        self.selected_timeline_index = self.selected_timeline_index.saturating_sub(1);
        self.timeline_follow_mode = false;
        self.inspector_scroll_offset = 0;
    }

    pub(super) fn apply_timeline_down(&mut self, total_entries: usize) {
        let max = total_entries.saturating_sub(1);
        self.selected_timeline_index = (self.selected_timeline_index + 1).min(max);
        self.timeline_follow_mode = self.selected_timeline_index >= max;
        self.inspector_scroll_offset = 0;
    }

    pub(super) fn apply_timeline_home(&mut self) {
        self.selected_timeline_index = 0;
        self.timeline_follow_mode = false;
        self.inspector_scroll_offset = 0;
    }

    pub(super) fn apply_timeline_end(&mut self, total_entries: usize) {
        self.selected_timeline_index = total_entries.saturating_sub(1);
        self.timeline_follow_mode = true;
        self.inspector_scroll_offset = 0;
    }

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
pub(super) fn input_rows_for_buffer(_input: &str, _width: usize) -> u16 {
    1
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
        assert_eq!(offset, 5);
        apply_bounded_scroll(&mut offset, ScrollAction::Home, 5);
        assert_eq!(offset, 0);
        apply_bounded_scroll(&mut offset, ScrollAction::End, 5);
        assert_eq!(offset, 5);
    }
}
