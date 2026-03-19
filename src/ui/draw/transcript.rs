use super::*;
use std::io::Write;

impl TaskDraw {
    /// Render a single transcript line with semantic and markdown-aware styling.
    pub(super) fn draw_transcript_line(&mut self, w: &mut dyn Write, line: &str, cols: u16) {
        // ── Tool status markers ────────────────────────────────────
        if let Some(rest) = line.strip_prefix("[ok] ") {
            set_bold(w);
            set_fg(w, GREEN);
            let _ = write!(w, " \u{2605} "); // ★
            reset_style(w);
            set_fg(w, WHITE);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }
        if let Some(rest) = line.strip_prefix("[!] ") {
            set_bold(w);
            set_fg(w, RED);
            let _ = write!(w, " \u{2716} "); // ✖
            reset_style(w);
            set_fg(w, WHITE);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Code block fence detection ─────────────────────────────
        if line.starts_with("```") {
            self.in_code_block = !self.in_code_block;
            set_dim(w);
            set_fg(w, DIM_GRAY);
            if self.in_code_block {
                // Opening fence — show language tag if present.
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
                let remaining = (cols as usize).saturating_sub(used);
                for _ in 0..remaining.min(60) {
                    let _ = write!(w, "\u{2500}"); // ─
                }
            } else {
                // Closing fence — thin rule.
                let _ = write!(w, " ");
                for _ in 0..(cols as usize).saturating_sub(1).min(60) {
                    let _ = write!(w, "\u{2500}");
                }
            }
            reset_style(w);
            return;
        }

        // ── Inside code block — monospace with left bar ────────────
        if self.in_code_block {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{2502} "); // │
            set_fg(w, GRAY);
            let truncated = truncate_to_width(line, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Markdown headers ───────────────────────────────────────
        if let Some(rest) = line.strip_prefix("### ") {
            set_bold(w);
            set_fg(w, YELLOW);
            let _ = write!(w, " ");
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(1));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            set_bold(w);
            set_fg(w, YELLOW);
            let _ = write!(w, " ");
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(1));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            set_bold(w);
            set_fg(w, WHITE);
            let _ = write!(w, " ");
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(1));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Blockquotes ────────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("> ") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{2502} "); // │
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Bullet lists ───────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            // Task checklist items: - [x] or - [ ] patterns.
            if let Some(task_rest) = rest
                .strip_prefix("[x] ")
                .or_else(|| rest.strip_prefix("[X] "))
            {
                set_fg(w, GREEN);
                let _ = write!(w, " \u{2611} "); // ☑
                reset_style(w);
                set_fg(w, GRAY);
                let truncated = truncate_to_width(task_rest, (cols as usize).saturating_sub(3));
                let _ = write!(w, "{truncated}");
                reset_style(w);
                return;
            }
            if let Some(task_rest) = rest.strip_prefix("[ ] ") {
                set_fg(w, DIM_GRAY);
                let _ = write!(w, " \u{2610} "); // ☐
                reset_style(w);
                set_fg(w, GRAY);
                let truncated = truncate_to_width(task_rest, (cols as usize).saturating_sub(3));
                let _ = write!(w, "{truncated}");
                reset_style(w);
                return;
            }
            set_fg(w, YELLOW);
            let _ = write!(w, " \u{2022} "); // •
            reset_style(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
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
            let rule_width = (cols as usize).min(80);
            let _ = write!(w, " ");
            for _ in 0..rule_width.saturating_sub(1) {
                let _ = write!(w, "\u{2500}"); // ─
            }
            reset_style(w);
            return;
        }

        // ── Indented detail text ───────────────────────────────────
        if line.starts_with("    ") {
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
                for _ in 0..remaining.min(40) {
                    let _ = write!(w, "\u{2500}");
                }
            } else {
                let _ = write!(w, " ");
                let remaining = (cols as usize).saturating_sub(prefix_used + 1);
                for _ in 0..remaining.min(40) {
                    let _ = write!(w, "\u{2500}");
                }
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
pub(super) fn parse_numbered_list_item(line: &str) -> Option<(&str, &str, usize)> {
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
