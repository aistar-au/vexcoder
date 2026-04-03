use super::*;
use crate::status_contract::{status_tone, StatusTone};
use std::borrow::Cow;
use std::io::Write;

// ── Shared transcript rendering helpers ────────────────────────────

/// Write a styled prefix icon followed by truncated content text.
///
/// This consolidates the repeated pattern:
///   set_fg(icon_color) → write(icon) → reset → set_fg(text_color) → truncate+write → reset
pub(crate) fn draw_icon_line(
    w: &mut dyn Write,
    icon_color: u8,
    icon: &str,
    text_color: u8,
    text: &str,
    cols: u16,
    bold_icon: bool,
) {
    if bold_icon {
        set_bold(w);
    }
    set_fg(w, icon_color);
    let _ = write!(w, " {icon} ");
    reset_style(w);
    set_fg(w, text_color);
    let truncated = truncate_to_width(
        text,
        (cols as usize).saturating_sub(display_width(icon) + 2),
    );
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

/// Write a left-bar prefixed line (used for code blocks and blockquotes).
pub(crate) fn draw_bar_line(w: &mut dyn Write, text: &str, cols: u16, dim: bool) {
    if dim {
        set_dim(w);
    }
    set_fg(w, DIM_GRAY);
    let _ = write!(w, " \u{2502} "); // │
    set_fg(w, WHITE);
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(3));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

/// Write a bold header line (used for markdown # headings).
pub(crate) fn draw_heading(w: &mut dyn Write, text: &str, color: u8, cols: u16) {
    set_bold(w);
    set_fg(w, color);
    let _ = write!(w, " ");
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(1));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

/// Write a tool paragraph header line at 2-space disclosure level.
///
/// Renders with a ✦ cosmic accent marker in bold violet followed by the
/// tool summary text with a brighter tool name, a dimmer target hint,
/// and a status accent when present:
///
/// ```text
///   ✦ read_file src/main.rs
/// ```
pub(crate) fn draw_tool_paragraph_header(w: &mut dyn Write, text: &str, cols: u16) {
    set_bold(w);
    set_fg(w, MAGENTA);
    let _ = write!(w, "  \u{2726} "); // 2-space indent + ✦
    reset_style(w);
    if let Some((leading, status)) = split_tool_summary(text) {
        let available = (cols as usize).saturating_sub(4);
        let leading_text = leading.join(" · ");
        let full_text = format!("{leading_text} · {status}");
        let truncated = truncate_to_width(&full_text, available);
        if truncated != full_text {
            set_fg(w, WHITE);
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        for (index, segment) in leading.iter().enumerate() {
            if index > 0 {
                set_dim(w);
                set_fg(w, DIM_GRAY);
                let _ = write!(w, " \u{00b7} ");
                reset_style(w);
            }
            set_fg(w, if index == 0 { WHITE } else { GRAY });
            let _ = write!(w, "{segment}");
            reset_style(w);
        }
        set_dim(w);
        set_fg(w, DIM_GRAY);
        let _ = write!(w, " \u{00b7} ");
        reset_style(w);
        set_bold(w);
        set_fg(w, status_color(status).unwrap_or(WHITE));
        let _ = write!(w, "{status}");
        reset_style(w);
        return;
    }
    set_fg(w, WHITE);
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(4));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

/// Write a tool evidence line at 6-space disclosure level.
///
/// Renders with a ✧ accent marker in dim styling for enriched evidence
pub(crate) fn draw_status_paragraph_header(
    w: &mut dyn Write,
    accent: u8,
    marker: &str,
    text: &str,
    cols: u16,
    body: u8,
) {
    set_bold(w);
    set_fg(w, accent);
    let _ = write!(w, "  {marker} ");
    reset_style(w);
    set_fg(w, body);
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(4));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

pub(crate) fn split_tool_summary(text: &str) -> Option<(Vec<&str>, &str)> {
    let segments: Vec<&str> = text.split(" \u{00b7} ").collect();
    let (status, leading) = segments.split_last()?;
    if leading.is_empty() || status_tone(status).is_none() {
        return None;
    }
    Some((leading.to_vec(), *status))
}

pub(crate) fn status_color(status: &str) -> Option<u8> {
    match status_tone(status)? {
        StatusTone::Success => Some(GREEN),
        StatusTone::Error => Some(RED),
        StatusTone::Progress => Some(MAGENTA),
        StatusTone::Attention => Some(YELLOW),
    }
}

pub(crate) fn is_inline_telemetry_summary(line: &str) -> bool {
    line.starts_with('[') && line.ends_with(']') && line.contains("total:")
}

pub(crate) fn telemetry_segment_color(label: &str) -> u8 {
    match label {
        "ttft" => CYAN,
        "\u{2191}" | "read" => MAGENTA,
        "\u{2193}" | "generate" => GREEN,
        "total" => YELLOW,
        _ => DIM_GRAY,
    }
}

pub(crate) fn rewrite_telemetry_label(label: &str) -> &str {
    match label {
        "read" => "\u{2191}",
        "generate" => "\u{2193}",
        _ => label,
    }
}

pub(crate) fn rewrite_telemetry_segment(segment: &str) -> Cow<'_, str> {
    let Some((label, value)) = segment.split_once(':') else {
        return Cow::Borrowed(segment);
    };
    let display_label = rewrite_telemetry_label(label);
    if display_label == label {
        Cow::Borrowed(segment)
    } else {
        Cow::Owned(format!("{display_label}:{value}"))
    }
}

pub(crate) fn draw_inline_telemetry_summary(w: &mut dyn Write, line: &str, cols: u16) {
    let inner = line
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(line);
    let available = cols as usize;
    let segments = inner
        .split(" | ")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(rewrite_telemetry_segment)
        .collect::<Vec<_>>();
    let compact = format!(
        "[{}]",
        segments
            .iter()
            .map(|segment| segment.as_ref())
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let truncated = truncate_to_width(&compact, available);
    if truncated != compact {
        set_dim(w);
        set_fg(w, DIM_GRAY);
        let _ = write!(w, "{truncated}");
        reset_style(w);
        return;
    }

    set_dim(w);
    set_fg(w, DIM_GRAY);
    let _ = write!(w, "[");
    reset_style(w);

    for (index, segment) in segments.iter().enumerate() {
        let segment = segment.as_ref();
        if index > 0 {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " | ");
            reset_style(w);
        }
        if let Some((label, value)) = segment.split_once(':') {
            let color = telemetry_segment_color(label);
            set_bold(w);
            set_fg(w, color);
            let _ = write!(w, "{label}:");
            reset_style(w);
            set_fg(w, color);
            let _ = write!(w, "{value}");
            reset_style(w);
        } else {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, "{segment}");
            reset_style(w);
        }
    }

    set_dim(w);
    set_fg(w, DIM_GRAY);
    let _ = write!(w, "]");
    reset_style(w);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_prefixed_disclosure_line(
    w: &mut dyn Write,
    indent: &str,
    marker: Option<&str>,
    text: &str,
    cols: u16,
    accent: u8,
    body: u8,
    dim: bool,
) {
    if dim {
        set_dim(w);
    }
    set_fg(w, accent);
    let _ = write!(w, "{indent}");
    if let Some(marker) = marker {
        let _ = write!(w, "{marker}");
    }
    set_fg(w, body);
    let used = display_width(indent) + marker.map(display_width).unwrap_or_default();
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(used));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}
pub(crate) fn draw_nested_bar_line(
    w: &mut dyn Write,
    indent: &str,
    text: &str,
    cols: u16,
    dim: bool,
) {
    if dim {
        set_dim(w);
    }
    set_fg(w, DIM_GRAY);
    let _ = write!(w, "{indent}\u{2502} ");
    set_fg(w, GRAY);
    let used = display_width(indent) + 2;
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(used));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

pub(crate) fn draw_nested_rule(w: &mut dyn Write, indent: &str, cols: u16) {
    set_dim(w);
    set_fg(w, DIM_GRAY);
    let _ = write!(w, "{indent}");
    draw_thin_rule(w, (cols as usize).saturating_sub(display_width(indent)), 80);
    reset_style(w);
}

pub(crate) fn write_disclosure_prefix(
    w: &mut dyn Write,
    indent: &str,
    marker: Option<&str>,
    accent: u8,
    dim: bool,
) -> usize {
    if dim {
        set_dim(w);
    }
    set_fg(w, accent);
    let _ = write!(w, "{indent}");
    if let Some(marker) = marker {
        let _ = write!(w, "{marker}");
    }
    display_width(indent) + marker.map(display_width).unwrap_or_default()
}

pub(crate) fn looks_like_field_label(label: &str) -> bool {
    let trimmed = label.trim();
    !trimmed.is_empty()
        && display_width(trimmed) <= 24
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || ch == ' ' || ch == '/')
}

pub(crate) fn split_field_value(text: &str) -> Option<(&str, &str)> {
    let (label, value) = text.split_once(": ")?;
    looks_like_field_label(label).then_some((label.trim(), value))
}

pub(crate) fn field_label_color(label: &str) -> u8 {
    match label {
        "Scope" => CYAN,
        "Command" => MAGENTA,
        "Input" => YELLOW,
        "Exit" => YELLOW,
        "Status" | "Status note" => DIM_GRAY,
        _ => WHITE,
    }
}

pub(crate) fn field_value_color(label: &str, value: &str, default: u8) -> u8 {
    let trimmed = value.trim();
    if let Some(status) = status_color(trimmed) {
        return status;
    }
    match label {
        "Command" => MAGENTA,
        "Scope" => CYAN,
        "Input" => YELLOW,
        "Exit" => match trimmed.parse::<i32>() {
            Ok(0) => GREEN,
            Ok(_) => RED,
            Err(_) => default,
        },
        _ => default,
    }
}

pub(crate) fn draw_field_label(w: &mut dyn Write, label: &str, used: &mut usize, cols: u16) {
    let field = format!("{label}: ");
    set_fg(w, field_label_color(label));
    let truncated = truncate_to_width(&field, (cols as usize).saturating_sub(*used));
    let _ = write!(w, "{truncated}");
    *used += display_width(&truncated);
}

pub(crate) fn draw_json_tail(w: &mut dyn Write, text: &str, cols: u16, mut used: usize) {
    let mut in_string = false;
    let mut escape = false;
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() && used < cols as usize {
        let ch = chars[index];
        let next = chars.get(index + 1).copied();
        if in_string {
            set_fg(w, GREEN);
        } else {
            match ch {
                '{' | '}' | '[' | ']' | ':' | ',' => set_fg(w, DIM_GRAY),
                't' | 'f' | 'n'
                    if starts_with_literal(&chars[index..], &["true", "false", "null"]) =>
                {
                    set_fg(w, MAGENTA)
                }
                '-' | '0'..='9' => set_fg(w, YELLOW),
                _ => set_fg(w, CYAN),
            }
        }
        if ch == '"' && !escape {
            if !in_string {
                in_string = true;
                if next == Some(':') {
                    set_fg(w, CYAN);
                } else {
                    set_fg(w, GREEN);
                }
            } else {
                set_fg(w, GREEN);
            }
        }
        let ch_width = display_width(&ch.to_string());
        if used + ch_width > cols as usize {
            break;
        }
        let _ = write!(w, "{ch}");
        used += ch_width;
        if in_string && ch == '"' && !escape {
            in_string = false;
        }
        escape = in_string && ch == '\\' && !escape;
        if ch != '\\' {
            escape = false;
        }
        index += 1;
    }
}

pub(crate) fn draw_json_line(
    w: &mut dyn Write,
    indent: &str,
    marker: Option<&str>,
    text: &str,
    cols: u16,
) {
    let used = write_disclosure_prefix(w, indent, marker, DIM_GRAY, true);
    draw_json_tail(w, text, cols, used);
    reset_style(w);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_labeled_disclosure_line(
    w: &mut dyn Write,
    indent: &str,
    marker: Option<&str>,
    label: &str,
    value: &str,
    cols: u16,
    accent: u8,
    body: u8,
    dim: bool,
) {
    let mut used = write_disclosure_prefix(w, indent, marker, accent, dim);
    draw_field_label(w, label, &mut used, cols);
    set_fg(w, field_value_color(label, value, body));
    let truncated = truncate_to_width(value, (cols as usize).saturating_sub(used));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_labeled_json_line(
    w: &mut dyn Write,
    indent: &str,
    marker: Option<&str>,
    label: &str,
    value: &str,
    cols: u16,
    accent: u8,
) {
    let mut used = write_disclosure_prefix(w, indent, marker, accent, true);
    draw_field_label(w, label, &mut used, cols);
    draw_json_tail(w, value, cols, used);
    reset_style(w);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_labeled_diff_line(
    w: &mut dyn Write,
    indent: &str,
    marker: Option<&str>,
    label: &str,
    value: &str,
    cols: u16,
    accent: u8,
    color: u8,
    bold: bool,
) {
    let mut used = write_disclosure_prefix(w, indent, marker, accent, true);
    draw_field_label(w, label, &mut used, cols);
    if bold {
        set_bold(w);
    }
    set_fg(w, color);
    let truncated = truncate_to_width(value, (cols as usize).saturating_sub(used));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

pub(crate) fn starts_with_literal(chars: &[char], literals: &[&str]) -> bool {
    literals.iter().any(|literal| {
        literal
            .chars()
            .eq(chars.iter().copied().take(literal.len()))
    })
}

/// Detect an inline telemetry summary like `[ttft:0.3s | ↑:2.5s (2641 tok) | total:7.7s]`.
pub(crate) fn is_telemetry_summary(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('[')
        && trimmed.ends_with(']')
        && (trimmed.contains("total:") || trimmed.contains("ttft:"))
}

/// Render a telemetry summary line with per-segment coloring:
///   `ttft` → cyan, `↑` (read) → magenta, `↓` (generate) → green, `total` → yellow,
///   token counts → dim gray.
pub(crate) fn draw_telemetry_summary(w: &mut dyn Write, line: &str, cols: u16) {
    let trimmed = line.trim();
    // Strip outer brackets.
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);

    set_dim(w);
    set_fg(w, DIM_GRAY);
    let _ = write!(w, " [");
    let mut used: usize = 2; // " ["
    let max = cols as usize;

    for (i, segment) in inner.split(" | ").enumerate() {
        if i > 0 {
            if used + 3 >= max {
                break;
            }
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " | ");
            used += 3;
        }
        let color = if segment.starts_with("ttft:") {
            CYAN
        } else if segment.starts_with("read:") || segment.starts_with("\u{2191}:") {
            MAGENTA
        } else if segment.starts_with("generate:") || segment.starts_with("\u{2193}:") {
            GREEN
        } else if segment.starts_with("total:") {
            YELLOW
        } else {
            GRAY
        };

        // Split segment into label:value and optional (N tok) suffix.
        if let Some((label, rest)) = segment.split_once(':') {
            // Rewrite verbose labels to compact arrows.
            let display_label = rewrite_telemetry_label(label);
            // Label
            set_fg(w, DIM_GRAY);
            let lbl = format!("{display_label}:");
            let trun = truncate_to_width(&lbl, max.saturating_sub(used));
            let _ = write!(w, "{trun}");
            used += display_width(&trun);

            // Value (timing)
            if let Some((timing, tok_part)) = rest.split_once(" (") {
                set_fg(w, color);
                let trun = truncate_to_width(timing, max.saturating_sub(used));
                let _ = write!(w, "{trun}");
                used += display_width(&trun);

                // Token count suffix
                set_fg(w, DIM_GRAY);
                let suffix = format!(" ({tok_part}");
                let trun = truncate_to_width(&suffix, max.saturating_sub(used));
                let _ = write!(w, "{trun}");
                used += display_width(&trun);
            } else {
                set_fg(w, color);
                let trun = truncate_to_width(rest, max.saturating_sub(used));
                let _ = write!(w, "{trun}");
                used += display_width(&trun);
            }
        } else {
            set_fg(w, color);
            let trun = truncate_to_width(segment, max.saturating_sub(used));
            let _ = write!(w, "{trun}");
            used += display_width(&trun);
        }
    }

    set_fg(w, DIM_GRAY);
    if used < max {
        let _ = write!(w, "]");
    }
    reset_style(w);
}

pub(crate) fn looks_like_json_line(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('}')
        || trimmed.starts_with(']')
        || trimmed.contains("\":")
}

pub(crate) fn diff_style(line: &str) -> Option<(u8, bool)> {
    if line.starts_with('+') && !line.starts_with("+++") {
        Some((GREEN, false))
    } else if line.starts_with('-') && !line.starts_with("---") {
        Some((RED, false))
    } else if line.starts_with("@@")
        || line.starts_with("diff --git")
        || line.starts_with("index ")
        || line.starts_with("+++ ")
        || line.starts_with("--- ")
    {
        Some((CYAN, true))
    } else {
        None
    }
}

pub(crate) fn parse_command_session_started(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.strip_prefix("[command session started")?;
    let (meta, command) = rest.split_once("] ")?;
    let pid = meta
        .strip_prefix(" pid=")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Some((command.to_string(), pid))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_nested_disclosure_line(
    this: &mut TaskDraw,
    w: &mut dyn Write,
    text: &str,
    cols: u16,
    indent: &str,
    marker: Option<&str>,
    body: u8,
    accent: u8,
) {
    let trimmed = text.trim_start();
    if trimmed.starts_with("```") {
        this.in_code_block = !this.in_code_block;
        draw_prefixed_disclosure_line(w, indent, marker, trimmed, cols, accent, DIM_GRAY, true);
        return;
    }
    if this.in_code_block {
        let nested_indent = format!("{indent}{}", marker.unwrap_or_default());
        draw_nested_bar_line(w, &nested_indent, text, cols, false);
        return;
    }
    if let Some((label, value)) = split_field_value(trimmed) {
        if let Some((color, bold)) = diff_style(value) {
            draw_labeled_diff_line(w, indent, marker, label, value, cols, accent, color, bold);
            return;
        }
        if looks_like_json_line(value) {
            draw_labeled_json_line(w, indent, marker, label, value, cols, accent);
            return;
        }
        draw_labeled_disclosure_line(w, indent, marker, label, value, cols, accent, body, true);
        return;
    }
    if let Some((color, bold)) = diff_style(trimmed) {
        if bold {
            set_bold(w);
        }
        draw_prefixed_disclosure_line(w, indent, marker, trimmed, cols, accent, color, true);
        if bold {
            reset_style(w);
        }
        return;
    }
    if looks_like_json_line(trimmed) {
        draw_json_line(w, indent, marker, trimmed, cols);
        return;
    }
    if let Some(rest) = trimmed.strip_prefix("> ") {
        let nested_indent = format!("{indent}{}", marker.unwrap_or_default());
        draw_nested_bar_line(w, &nested_indent, rest, cols, true);
        return;
    }
    if is_horizontal_rule(trimmed) {
        let nested_indent = format!("{indent}{}", marker.unwrap_or_default());
        draw_nested_rule(w, &nested_indent, cols);
        return;
    }
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        if let Some(task_rest) = rest
            .strip_prefix("[x] ")
            .or_else(|| rest.strip_prefix("[X] "))
        {
            draw_prefixed_disclosure_line(
                w,
                indent,
                marker,
                &format!("☑ {task_rest}"),
                cols,
                accent,
                GREEN,
                true,
            );
            return;
        }
        if let Some(task_rest) = rest.strip_prefix("[ ] ") {
            draw_prefixed_disclosure_line(
                w,
                indent,
                marker,
                &format!("☐ {task_rest}"),
                cols,
                accent,
                DIM_GRAY,
                true,
            );
            return;
        }
        draw_prefixed_disclosure_line(
            w,
            indent,
            marker,
            &format!("• {rest}"),
            cols,
            accent,
            body,
            true,
        );
        return;
    }
    if let Some((prefix, rest, _)) = parse_numbered_list_item(trimmed) {
        draw_prefixed_disclosure_line(
            w,
            indent,
            marker,
            &format!("{prefix}{rest}"),
            cols,
            accent,
            body,
            true,
        );
        return;
    }
    if let Some(rest) = trimmed.strip_prefix("### ") {
        draw_prefixed_disclosure_line(w, indent, marker, rest, cols, accent, YELLOW, false);
        return;
    }
    if let Some(rest) = trimmed.strip_prefix("## ") {
        draw_prefixed_disclosure_line(w, indent, marker, rest, cols, accent, YELLOW, false);
        return;
    }
    if let Some(rest) = trimmed.strip_prefix("# ") {
        draw_prefixed_disclosure_line(w, indent, marker, rest, cols, accent, WHITE, false);
        return;
    }
    draw_prefixed_disclosure_line(w, indent, marker, trimmed, cols, accent, body, true);
}

/// Write a thin horizontal rule of `─` characters.
pub(crate) fn draw_thin_rule(w: &mut dyn Write, width: usize, max: usize) {
    for _ in 0..width.min(max) {
        let _ = write!(w, "\u{2500}");
    }
}
/// Parse a numbered list item like "1. foo" or "12. bar".
/// Returns (prefix_with_dot, rest_text, prefix_display_width).
pub(crate) fn parse_numbered_list_item(line: &str) -> Option<(&str, &str, usize)> {
    let bytes = line.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() {
        return None;
    }
    // Find the dot-space after digits: "N. "
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len().saturating_sub(1) {
        return None;
    }
    if bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b' ' {
        let prefix = &line[..i + 2]; // e.g. "1. "
        let rest = &line[i + 2..];
        Some((prefix, rest, display_width(prefix)))
    } else {
        None
    }
}

/// Check if a line is a markdown horizontal rule (---, ***, or ___).
pub(crate) fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let ch = trimmed.as_bytes()[0];
    if ch != b'-' && ch != b'*' && ch != b'_' {
        return false;
    }
    // Must be at least 3 of the same character, optionally with spaces.
    let count = trimmed.chars().filter(|c| *c as u8 == ch).count();
    let space_count = trimmed.chars().filter(|c| *c == ' ').count();
    count >= 3 && count + space_count == trimmed.len()
}

// ── Delta-native compact paragraph formatting ──────────────────────

/// Format a structured transcript delta into compact paragraph lines.
///
/// Applies uniform prefix and width-safe truncation regardless of
/// block kind. Used by the delta-native rendering path to bypass
/// prefix-marker parsing.
pub(crate) fn format_compact_paragraph(
    text: &str,
    block_kind: crate::state::TranscriptBlockKind,
    width: usize,
) -> String {
    use crate::state::TranscriptBlockKind;

    let prefix = match block_kind {
        TranscriptBlockKind::ToolCall => "\u{25b6} ",   // ▶
        TranscriptBlockKind::ToolResult => "  \u{21b3} ", // ↳
        TranscriptBlockKind::Thinking => "",
        TranscriptBlockKind::FinalText => "",
    };

    let prefix_width = display_width(prefix);
    let mut formatted = String::with_capacity(text.len() + prefix.len() * 4);
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let trimmed = truncate_to_width(line, width.saturating_sub(prefix_width));
        formatted.push_str(prefix);
        formatted.push_str(&trimmed);
        formatted.push('\n');
    }
    formatted
}
