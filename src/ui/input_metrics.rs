use unicode_width::UnicodeWidthChar;

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

pub fn cursor_row_col(input: &str, cursor_byte: usize, width: usize) -> (usize, usize) {
    let width = width.max(1);
    let mut row = 0usize;
    let mut col = 0usize;
    let cursor_byte = clamp_to_char_boundary_left(input, cursor_byte);

    for (idx, ch) in input.char_indices() {
        if idx >= cursor_byte {
            break;
        }
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            row += 1;
            col = 0;
            continue;
        }
        let ch_width = char_display_width(ch);
        if col + ch_width > width && col > 0 {
            row += 1;
            col = 0;
        }
        col += ch_width;
    }

    if col >= width {
        row += 1;
        col = 0;
    }

    (row, col)
}

pub fn cursor_byte_for_row_col(
    input: &str,
    target_row: usize,
    target_col: usize,
    width: usize,
) -> usize {
    let width = width.max(1);
    let spans = visual_row_spans(input, width);
    let (start, end) = spans
        .get(target_row)
        .copied()
        .unwrap_or_else(|| spans.last().copied().unwrap_or((0, 0)));
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

fn visual_row_spans(input: &str, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut spans = Vec::new();
    let mut row_start = 0usize;
    let mut col = 0usize;

    for (idx, ch) in input.char_indices() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            spans.push((row_start, idx));
            row_start = idx + ch.len_utf8();
            col = 0;
            continue;
        }

        let ch_width = char_display_width(ch);
        if col + ch_width > width && col > 0 {
            spans.push((row_start, idx));
            row_start = idx;
            col = 0;
        }
        col += ch_width;
    }

    spans.push((row_start.min(input.len()), input.len()));
    if spans.is_empty() {
        spans.push((0, 0));
    }
    spans
}
