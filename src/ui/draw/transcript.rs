use super::*;
use std::io::Write;

// ── Shared transcript rendering helpers ────────────────────────────

/// Write a styled prefix icon followed by truncated content text.
///
/// This consolidates the repeated pattern:
///   set_fg(icon_color) → write(icon) → reset → set_fg(text_color) → truncate+write → reset
fn draw_icon_line(
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
fn draw_bar_line(w: &mut dyn Write, text: &str, cols: u16, dim: bool) {
    if dim {
        set_dim(w);
    }
    set_fg(w, DIM_GRAY);
    let _ = write!(w, " \u{2502} "); // │
    set_fg(w, GRAY);
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(3));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

/// Write a bold header line (used for markdown # headings).
fn draw_heading(w: &mut dyn Write, text: &str, color: u8, cols: u16) {
    set_bold(w);
    set_fg(w, color);
    let _ = write!(w, " ");
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(1));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

/// Write a tool paragraph header line at 2-space disclosure level.
///
/// Renders with a ✦ cosmic accent marker in bold cyan followed by the
/// tool summary text with a brighter tool name, a dimmer target hint,
/// and a status accent when present:
///
/// ```text
///   ✦ read_file src/main.rs
/// ```
fn draw_tool_paragraph_header(w: &mut dyn Write, text: &str, cols: u16) {
    let segments: Vec<&str> = text.split(" \u{00b7} ").collect();
    set_bold(w);
    set_fg(w, CYAN);
    let _ = write!(w, "  \u{2726} "); // 2-space indent + ✦
    reset_style(w);
    if let Some((status, leading)) = segments.split_last() {
        if !leading.is_empty() && matches!(*status, "completed" | "failed" | "running") {
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
            set_fg(
                w,
                match *status {
                    "completed" => GREEN,
                    "failed" => RED,
                    "running" => CYAN,
                    _ => WHITE,
                },
            );
            let _ = write!(w, "{status}");
            reset_style(w);
            return;
        }
    }
    set_fg(w, WHITE);
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(4));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

/// Write a tool phase detail line at 4-space disclosure level.
///
/// Renders the detail text dimmed at the nested indent level:
///
/// ```text
///     Status: completed, 42 lines
/// ```
fn draw_tool_detail_line(w: &mut dyn Write, text: &str, cols: u16) {
    set_dim(w);
    set_fg(w, GRAY);
    let _ = write!(w, "    ");
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(4));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

/// Write a tool evidence line at 6-space disclosure level.
///
/// Renders with a ✧ accent marker in dim styling for enriched evidence
fn draw_status_paragraph_header(
    w: &mut dyn Write,
    accent: u8,
    marker: &str,
    text: &str,
    cols: u16,
) {
    set_bold(w);
    set_fg(w, accent);
    let _ = write!(w, "  {marker} ");
    reset_style(w);
    set_fg(w, WHITE);
    let truncated = truncate_to_width(text, (cols as usize).saturating_sub(4));
    let _ = write!(w, "{truncated}");
    reset_style(w);
}

#[allow(clippy::too_many_arguments)]
fn draw_prefixed_disclosure_line(
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
fn draw_nested_bar_line(w: &mut dyn Write, indent: &str, text: &str, cols: u16, dim: bool) {
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

fn draw_nested_rule(w: &mut dyn Write, indent: &str, cols: u16) {
    set_dim(w);
    set_fg(w, DIM_GRAY);
    let _ = write!(w, "{indent}");
    draw_thin_rule(w, (cols as usize).saturating_sub(display_width(indent)), 80);
    reset_style(w);
}

fn draw_json_line(w: &mut dyn Write, indent: &str, marker: Option<&str>, text: &str, cols: u16) {
    set_dim(w);
    set_fg(w, DIM_GRAY);
    let _ = write!(w, "{indent}");
    if let Some(marker) = marker {
        let _ = write!(w, "{marker}");
    }
    let mut used = display_width(indent) + marker.map(display_width).unwrap_or_default();
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
    reset_style(w);
}

fn starts_with_literal(chars: &[char], literals: &[&str]) -> bool {
    literals.iter().any(|literal| {
        literal
            .chars()
            .eq(chars.iter().copied().take(literal.len()))
    })
}

fn looks_like_json_line(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('}')
        || trimmed.starts_with(']')
        || trimmed.contains("\":")
}

fn diff_style(line: &str) -> Option<(u8, bool)> {
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

fn parse_command_session_started(line: &str) -> Option<(String, Option<String>)> {
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
fn draw_nested_disclosure_line(
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
fn draw_thin_rule(w: &mut dyn Write, width: usize, max: usize) {
    for _ in 0..width.min(max) {
        let _ = write!(w, "\u{2500}");
    }
}

impl TaskDraw {
    /// Render a single transcript line with semantic and markdown-aware styling.
    ///
    /// Tool activity renders as paragraph-style transcript blocks with stable
    /// 2/4/6-space disclosure levels:
    ///
    /// ```text
    ///   ✦ read_file src/main.rs              ← 2-space: tool activity summary
    ///     Status: completed, 42 lines        ← 4-space: phase detail
    ///       ✧ fn main() { … }                ← 6-space: evidence snippet
    /// ```
    pub(super) fn draw_transcript_line(&mut self, w: &mut dyn Write, line: &str, cols: u16) {
        if let Some(rest) = line.strip_prefix("[turn] ") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " ");
            draw_thin_rule(w, 3, 3);
            let _ = write!(w, " ");
            reset_style(w);
            draw_status_paragraph_header(w, YELLOW, "\u{2726}", rest, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[thinking] ") {
            draw_status_paragraph_header(w, BLUE, "\u{22ef}", rest, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[thinking_detail] ") {
            draw_prefixed_disclosure_line(w, "    ", None, rest, cols, BLUE, GRAY, true);
            return;
        }
        if let Some(rest) = line.strip_prefix("[approval] ") {
            draw_status_paragraph_header(w, YELLOW, "\u{2753}", rest, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[approval_detail] ") {
            draw_prefixed_disclosure_line(w, "    ", None, rest, cols, YELLOW, GRAY, true);
            return;
        }
        if let Some(rest) = line.strip_prefix("[error] ") {
            draw_status_paragraph_header(w, RED, "\u{2716}", rest, cols);
            return;
        }

        // ── Tool paragraph markers (2/4/6-space disclosure) ────────
        if let Some(rest) = line.strip_prefix("[tool] ") {
            draw_tool_paragraph_header(w, rest, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[detail] ") {
            draw_nested_disclosure_line(self, w, rest, cols, "    ", None, GRAY, DIM_GRAY);
            return;
        }
        if let Some(rest) = line.strip_prefix("[evidence] ") {
            draw_nested_disclosure_line(
                self,
                w,
                rest,
                cols,
                "      ",
                Some("\u{2727} "),
                GRAY,
                DIM_GRAY,
            );
            return;
        }

        if let Some((command, pid)) = parse_command_session_started(line) {
            let summary = pid
                .map(|pid| format!("command session \u{00b7} {command} \u{00b7} pid {pid}"))
                .unwrap_or_else(|| format!("command session \u{00b7} {command} \u{00b7} running"));
            draw_tool_paragraph_header(w, &summary, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[command session exit: ") {
            let exit = rest.trim_end_matches(']');
            draw_tool_detail_line(w, &format!("Exit: {exit}"), cols);
            return;
        }
        if line == "[command session cancelled]" {
            draw_tool_detail_line(w, "Status: cancelled", cols);
            return;
        }
        if line == "[command session cancellation requested]" {
            draw_tool_detail_line(w, "Status: cancellation requested", cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[command session] error: ") {
            draw_status_paragraph_header(w, RED, "\u{2716}", rest, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[stderr] ") {
            draw_prefixed_disclosure_line(
                w,
                "      ",
                Some("\u{2727} "),
                rest,
                cols,
                RED,
                RED,
                true,
            );
            return;
        }

        // ── Tool status markers ────────────────────────────────────
        if let Some(rest) = line.strip_prefix("[ok] ") {
            draw_icon_line(w, GREEN, "\u{2605}", WHITE, rest, cols, true);
            return;
        }
        if let Some(rest) = line.strip_prefix("[!] ") {
            draw_icon_line(w, RED, "\u{2716}", WHITE, rest, cols, true);
            return;
        }

        // ── Code block fence detection ─────────────────────────────
        if line.starts_with("```") {
            self.in_code_block = !self.in_code_block;
            set_dim(w);
            set_fg(w, DIM_GRAY);
            if self.in_code_block {
                let lang = line.trim_start_matches('`').trim();
                let _ = write!(w, " \u{2500}\u{2500} ");
                if !lang.is_empty() {
                    set_fg(w, BLUE);
                    let _ = write!(w, "{lang} ");
                    set_fg(w, DIM_GRAY);
                }
                let used = 4 + if lang.is_empty() {
                    0
                } else {
                    display_width(lang) + 1
                };
                draw_thin_rule(w, (cols as usize).saturating_sub(used), 60);
            } else {
                let _ = write!(w, " ");
                draw_thin_rule(w, (cols as usize).saturating_sub(1), 60);
            }
            reset_style(w);
            return;
        }

        // ── Inside code block — monospace with left bar ────────────
        if self.in_code_block {
            draw_bar_line(w, line, cols, false);
            return;
        }

        // ── Markdown headers ───────────────────────────────────────
        if let Some(rest) = line.strip_prefix("### ") {
            draw_heading(w, rest, YELLOW, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            draw_heading(w, rest, YELLOW, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            draw_heading(w, rest, WHITE, cols);
            return;
        }

        // ── Blockquotes ────────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("> ") {
            draw_bar_line(w, rest, cols, true);
            return;
        }

        // ── Bullet lists ───────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            if let Some(task_rest) = rest
                .strip_prefix("[x] ")
                .or_else(|| rest.strip_prefix("[X] "))
            {
                draw_icon_line(w, GREEN, "\u{2611}", GRAY, task_rest, cols, false);
                return;
            }
            if let Some(task_rest) = rest.strip_prefix("[ ] ") {
                draw_icon_line(w, DIM_GRAY, "\u{2610}", GRAY, task_rest, cols, false);
                return;
            }
            draw_icon_line(w, YELLOW, "\u{2022}", GRAY, rest, cols, false);
            return;
        }

        // ── Numbered lists ─────────────────────────────────────────
        if let Some(num_rest) = parse_numbered_list_item(line) {
            set_fg(w, YELLOW);
            let _ = write!(w, " {}", num_rest.0);
            reset_style(w);
            set_fg(w, GRAY);
            let truncated =
                truncate_to_width(num_rest.1, (cols as usize).saturating_sub(num_rest.2 + 1));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Horizontal rules ───────────────────────────────────────
        if is_horizontal_rule(line) {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " ");
            draw_thin_rule(w, (cols as usize).saturating_sub(1), 80);
            reset_style(w);
            return;
        }

        // ── Indented disclosure (6-space evidence, 4-space detail) ─
        // Must be checked before the 4-space handler since 6 spaces
        // also starts with 4 spaces.
        if line.starts_with("      ") {
            // 6-space: evidence-level — dimmer than detail.
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let truncated = truncate_to_width(line, cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }
        // ── Indented detail text (4-space) ────────────────────────
        if line.starts_with("    ") {
            // 4-space: detail-level disclosure.
            set_dim(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(line, cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Section separator ──────────────────────────────────────
        if line.starts_with("--- ") && line.ends_with(" ---") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{2500}\u{2500}\u{2500} "); // ───
            set_fg(w, YELLOW);
            let _ = write!(w, "\u{2726}"); // ✦
            set_fg(w, DIM_GRAY);
            let label = line.trim_start_matches('-').trim_end_matches('-').trim();
            let prefix_used: usize = 5;
            if !label.is_empty() {
                let max_label = (cols as usize).saturating_sub(prefix_used + 4);
                let safe_label = truncate_to_width(label, max_label);
                let _ = write!(w, " {safe_label} ");
                let label_display_w = display_width(&safe_label);
                let remaining = (cols as usize).saturating_sub(prefix_used + 2 + label_display_w);
                draw_thin_rule(w, remaining, 40);
            } else {
                let _ = write!(w, " ");
                let remaining = (cols as usize).saturating_sub(prefix_used + 1);
                draw_thin_rule(w, remaining, 40);
            }
            reset_style(w);
            return;
        }

        // ── Hint text ──────────────────────────────────────────────
        if line == "Turn completed." || line.starts_with("Type a prompt") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let truncated = truncate_to_width(line, cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Awaiting indicator ─────────────────────────────────────
        if line == "[awaiting model response]" {
            let idx = (self.frame_counter as usize) % SPINNER_FRAMES.len();
            set_fg(w, CYAN);
            let _ = write!(w, " {} awaiting response", SPINNER_FRAMES[idx]);
            reset_style(w);
            return;
        }

        // ── Regular text with inline bold detection ────────────────
        set_fg(w, GRAY);
        self.draw_inline_markdown(w, line, cols);
        reset_style(w);
    }

    /// Render inline markdown: **bold**, *italic*, `code`, and ~~strikethrough~~ spans.
    fn draw_inline_markdown(&self, w: &mut dyn Write, line: &str, cols: u16) {
        let max_w = cols as usize;
        let mut used: usize = 0;
        let mut chars = line.chars().peekable();
        let mut buf = String::new();

        while let Some(ch) = chars.next() {
            if used >= max_w {
                break;
            }
            if ch == '*' && chars.peek() == Some(&'*') {
                // Flush buffer.
                if !buf.is_empty() {
                    let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    buf.clear();
                }
                chars.next(); // consume second *
                              // Collect bold text until **
                let mut bold = String::new();
                while let Some(bc) = chars.next() {
                    if bc == '*' && chars.peek() == Some(&'*') {
                        chars.next();
                        break;
                    }
                    bold.push(bc);
                }
                if !bold.is_empty() {
                    set_bold(w);
                    set_fg(w, WHITE);
                    let truncated = truncate_to_width(&bold, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    reset_style(w);
                    set_fg(w, GRAY);
                }
            } else if ch == '*' || ch == '_' {
                // Single delimiter italic — only if the next char is not a space.
                let is_word_boundary = chars.peek().is_none_or(|c| c.is_whitespace());
                if is_word_boundary {
                    buf.push(ch);
                    continue;
                }
                // Flush buffer.
                if !buf.is_empty() {
                    let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    buf.clear();
                }
                let mut italic = String::new();
                for ic in chars.by_ref() {
                    if ic == ch {
                        break;
                    }
                    italic.push(ic);
                }
                if !italic.is_empty() {
                    set_italic(w);
                    set_fg(w, GRAY);
                    let truncated = truncate_to_width(&italic, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    reset_style(w);
                    set_fg(w, GRAY);
                }
            } else if ch == '~' && chars.peek() == Some(&'~') {
                // Flush buffer.
                if !buf.is_empty() {
                    let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    buf.clear();
                }
                chars.next(); // consume second ~
                let mut struck = String::new();
                while let Some(sc) = chars.next() {
                    if sc == '~' && chars.peek() == Some(&'~') {
                        chars.next();
                        break;
                    }
                    struck.push(sc);
                }
                if !struck.is_empty() {
                    set_dim(w);
                    set_fg(w, DIM_GRAY);
                    let truncated = truncate_to_width(&struck, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    reset_style(w);
                    set_fg(w, GRAY);
                }
            } else if ch == '`' {
                // Flush buffer.
                if !buf.is_empty() {
                    let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    buf.clear();
                }
                // Collect code span until `
                let mut code = String::new();
                for cc in chars.by_ref() {
                    if cc == '`' {
                        break;
                    }
                    code.push(cc);
                }
                if !code.is_empty() {
                    set_fg(w, CYAN);
                    let truncated = truncate_to_width(&code, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    set_fg(w, GRAY);
                }
            } else {
                buf.push(ch);
            }
        }
        // Flush remaining buffer.
        if !buf.is_empty() {
            let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
            let _ = write!(w, "{truncated}");
        }
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
pub(super) fn is_horizontal_rule(line: &str) -> bool {
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
