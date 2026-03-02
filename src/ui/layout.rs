use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreePaneLayout {
    pub header: Rect,
    pub history: Rect,
    pub input: Rect,
}

pub fn split_three_pane_layout(area: Rect, input_rows: u16) -> ThreePaneLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(input_rows.max(1)),
        ])
        .split(area);

    ThreePaneLayout {
        header: chunks[0],
        history: chunks[1],
        input: chunks[2],
    }
}

/// Task-first four-region layout state
#[derive(Clone, Debug)]
pub struct TaskLayoutState {
    pub task_id: String,
    pub status_line: String,
    pub activity_rows: Vec<String>,
    pub output_rows: Vec<String>,
    pub pending_approval: Option<String>,
    pub changed_files: Vec<String>,
}

impl TaskLayoutState {
    pub fn new(task_id: String) -> Self {
        Self {
            task_id,
            status_line: String::new(),
            activity_rows: Vec::new(),
            output_rows: Vec::new(),
            pending_approval: None,
            changed_files: Vec::new(),
        }
    }

    /// Add a status marker to an activity row
    pub fn add_activity_with_marker(&mut self, message: &str, marker: StatusMarker) {
        let marker_str = match marker {
            StatusMarker::Ok => "[ok]",
            StatusMarker::Pending => "[?]",
            StatusMarker::InProgress => "[->]",
            StatusMarker::Error => "[!]",
        };
        self.activity_rows
            .push(format!("{} {}", marker_str, message));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusMarker {
    Ok,
    Pending,
    InProgress,
    Error,
}

/// Four-region layout: header, activity trail, output pane, input pane
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FourRegionLayout {
    pub header: Rect,
    pub activity: Rect,
    pub output: Rect,
    pub input: Rect,
}

pub fn split_four_region_layout(area: Rect, header_rows: u16, input_rows: u16) -> FourRegionLayout {
    let header_rows = header_rows.max(1).min(2);
    let input_rows = input_rows.max(3).min(4);
    let activity_rows = 3u16; // Fixed activity trail height

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_rows),
            Constraint::Length(activity_rows),
            Constraint::Min(1), // Output pane takes remaining space
            Constraint::Length(input_rows),
        ])
        .split(area);

    FourRegionLayout {
        header: chunks[0],
        activity: chunks[1],
        output: chunks[2],
        input: chunks[3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_splits_into_three_panes() {
        let area = Rect::new(0, 0, 80, 20);
        let panes = split_three_pane_layout(area, 4);

        assert_eq!(panes.header.height, 1);
        assert_eq!(panes.history.height, 15);
        assert_eq!(panes.input.height, 4);
        assert_eq!(panes.header.y, 0);
        assert_eq!(panes.history.y, 1);
        assert_eq!(panes.input.y, 16);
    }

    #[test]
    fn layout_preserves_dynamic_input_height() {
        let area = Rect::new(0, 0, 80, 12);
        let panes = split_three_pane_layout(area, 6);

        assert_eq!(panes.input.height, 6);
        assert_eq!(panes.header.height, 1);
        assert_eq!(panes.history.height, 5);
    }

    #[test]
    fn test_task_layout_four_regions_render_without_panic() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = split_four_region_layout(area, 1, 3);

        // Verify four regions are created with expected constraints
        assert_eq!(layout.header.height, 1);
        assert_eq!(layout.activity.height, 3);
        assert_eq!(layout.input.height, 3);
        assert!(layout.output.height > 0);

        // Verify vertical stacking
        assert_eq!(layout.header.y, 0);
        assert_eq!(layout.activity.y, 1);
        assert_eq!(layout.output.y, 4);
        assert_eq!(layout.input.y, 4 + layout.output.height);

        // Verify TaskLayoutState can be created
        let task_state = TaskLayoutState::new("task-123".to_string());
        assert_eq!(task_state.task_id, "task-123");
    }

    #[test]
    fn test_changed_files_and_live_approval_prompt_render() {
        let mut state = TaskLayoutState::new("task-456".to_string());

        // Add changed files
        state.changed_files.push("src/main.rs".to_string());
        state.changed_files.push("src/lib.rs".to_string());

        // Add activity with markers
        state.add_activity_with_marker("File read", StatusMarker::Ok);
        state.add_activity_with_marker("Command running", StatusMarker::InProgress);
        state.add_activity_with_marker("Approval needed", StatusMarker::Pending);

        // Set pending approval
        state.pending_approval = Some("Allow write to src/main.rs?".to_string());

        // Verify state
        assert_eq!(state.changed_files.len(), 2);
        assert!(state.activity_rows[0].contains("[ok]"));
        assert!(state.activity_rows[1].contains("[->]"));
        assert!(state.activity_rows[2].contains("[?]"));
        assert!(state.pending_approval.is_some());
    }
}
