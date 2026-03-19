use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualLayout {
    spans: Vec<(usize, usize)>,
    cursor_row: usize,
    cursor_col: usize,
}

impl VisualLayout {
    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    pub fn row_count(&self) -> usize {
        self.spans.len().max(1)
    }

    pub fn row_bounds(&self, target_row: usize) -> (usize, usize) {
        self.spans
            .get(target_row)
            .copied()
            .unwrap_or_else(|| self.spans.last().copied().unwrap_or((0, 0)))
    }

    pub fn byte_for_row_col(&self, input: &str, target_row: usize, target_col: usize) -> usize {
        let (start, end) = self.row_bounds(target_row);
        byte_for_row_col_in_span(input, start, end, target_col)
    }
}

pub fn wrap_input_lines(input: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![String::new()];
    let mut line_widths = vec![0usize];
    for ch in input.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            lines.push(String::new());
            line_widths.push(0);
            continue;
        }
        let ch_width = char_display_width(ch);
        let current_width = *line_widths.last().unwrap_or(&0);
        if current_width + ch_width > width && current_width > 0 {
            lines.push(String::new());
            line_widths.push(0);
        }
        if let Some(line) = lines.last_mut() {
            line.push(ch);
        }
        if let Some(line_width) = line_widths.last_mut() {
            *line_width += ch_width;
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn visual_layout(input: &str, cursor_byte: usize, width: usize) -> VisualLayout {
    let width = width.max(1);
    let cursor_byte = clamp_to_char_boundary_left(input, cursor_byte);
    let mut spans = Vec::new();
    let mut row_start = 0usize;
    let mut row = 0usize;
    let mut col = 0usize;
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    let mut cursor_set = false;

    for (idx, ch) in input.char_indices() {
        if ch == '\r' {
            if !cursor_set && idx >= cursor_byte {
                cursor_row = row;
                cursor_col = col;
                cursor_set = true;
            }
            continue;
        }
        if ch == '\n' {
            if !cursor_set && idx >= cursor_byte {
                cursor_row = row;
                cursor_col = col;
                cursor_set = true;
            }
            spans.push((row_start, idx));
            row_start = idx + ch.len_utf8();
            row += 1;
            col = 0;
            continue;
        }

        let ch_width = char_display_width(ch);
        let wraps_before = col + ch_width > width && col > 0;
        if !cursor_set && idx >= cursor_byte {
            if idx == cursor_byte && wraps_before {
                cursor_row = row + 1;
                cursor_col = 0;
            } else {
                cursor_row = row;
                cursor_col = col;
            }
            cursor_set = true;
        }

        if wraps_before {
            spans.push((row_start, idx));
            row_start = idx;
            row += 1;
            col = 0;
        }
        col += ch_width;
    }

    if !cursor_set {
        cursor_row = row;
        cursor_col = col;
    }

    spans.push((row_start.min(input.len()), input.len()));

    if col >= width {
        spans.push((input.len(), input.len()));
        if cursor_byte == input.len() {
            cursor_row += 1;
            cursor_col = 0;
        }
    }

    VisualLayout {
        spans,
        cursor_row,
        cursor_col,
    }
}

pub fn cursor_row_col(input: &str, cursor_byte: usize, width: usize) -> (usize, usize) {
    let layout = visual_layout(input, cursor_byte, width);
    (layout.cursor_row(), layout.cursor_col())
}

pub fn visual_row_count(input: &str, width: usize) -> usize {
    visual_layout(input, input.len(), width).row_count()
}

pub fn visual_row_bounds(input: &str, target_row: usize, width: usize) -> (usize, usize) {
    visual_layout(input, input.len(), width).row_bounds(target_row)
}

pub fn cursor_byte_for_row_col(
    input: &str,
    target_row: usize,
    target_col: usize,
    width: usize,
) -> usize {
    let layout = visual_layout(input, input.len(), width);
    layout.byte_for_row_col(input, target_row, target_col)
}

pub fn visual_window_start(cursor_row: usize, visible_rows: usize) -> usize {
    cursor_row
        .saturating_add(1)
        .saturating_sub(visible_rows.max(1))
}

pub fn truncate_to_display_width(text: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let ch_width = char_display_width(ch);
        if used + ch_width > max_width && used > 0 {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out
}

pub fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

pub fn display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

pub fn clamp_to_char_boundary_left(input: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(input.len());
    while cursor > 0 && !input.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn byte_for_row_col_in_span(input: &str, start: usize, end: usize, target_col: usize) -> usize {
    if start >= end {
        return start.min(input.len());
    }

    let mut col = 0usize;
    for (offset, ch) in input[start..end].char_indices() {
        let idx = start + offset;
        if target_col <= col {
            return idx;
        }

        let next_col = col + char_display_width(ch);
        if target_col < next_col {
            return idx;
        }
        col = next_col;
    }

    end
}
