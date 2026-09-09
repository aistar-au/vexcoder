#[derive(Clone, Debug, Default)]
pub struct TaskViewProjection {
    pub status_line: String,
    pub expanded_output_rows: std::sync::Arc<[TranscriptRow]>,
    pub output_scroll_offset: usize,
    pub output_scroll_anchor: OutputScrollAnchor,
    pub composer_text: String,
    pub composer_cursor: usize,
    pub composer_focused: bool,
    pub picker_overlay: Vec<PickerOverlayLine>,
}

impl TaskLayoutState {
    pub fn into_view_projection(
        self,
        expanded_output_rows: std::sync::Arc<[TranscriptRow]>,
    ) -> TaskViewProjection {
        TaskViewProjection {
            status_line: self.status_line,
            expanded_output_rows,
            output_scroll_offset: self.output_scroll_offset,
            output_scroll_anchor: self.output_scroll_anchor,
            composer_text: self.composer_text,
            composer_cursor: self.composer_cursor,
            composer_focused: self.composer_focused,
            picker_overlay: self.picker_overlay,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskContextSummaryState {
    pub file_rollups: usize,
    pub related_paths: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub git_context_included: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskTelemetryState {
    pub mode: String,
    pub approval: String,
    pub model_name: String,
    pub model_backend: Option<crate::runtime::ModelBackendKind>,
    pub sandbox_kind: Option<crate::runtime::SandboxKind>,
    pub context_summary: Option<TaskContextSummaryState>,
    pub history_rows: usize,
    pub total_tokens: u64,

    pub tokens_sent: u64,

    pub tokens_received: u64,
    pub active_tools: usize,
    pub active_commands: usize,
    pub waiting_summary: Option<String>,
    pub timing_summary: Option<String>,

    pub git_branch: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickerOverlayLine {
    pub text: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMentionPickerState {
    pub range: Range<usize>,
    pub prefix: String,
    pub matches: Vec<String>,
    pub total_matches: usize,
}

pub(crate) fn file_picker_match_summary(
    prefix: &str,
    visible_count: usize,
    total_matches: usize,
) -> String {
    if let Some((dir_prefix, is_filtered)) = directory_picker_context(prefix) {
        if is_filtered && visible_count < total_matches {
            format!("[file] {visible_count} shown of {total_matches} in {dir_prefix}")
        } else {
            format!("[file] {total_matches} item(s) in {dir_prefix}")
        }
    } else if visible_count < total_matches {
        format!("[file] {visible_count} of {total_matches} match(es)")
    } else {
        format!("[file] {visible_count} match(es)")
    }
}
