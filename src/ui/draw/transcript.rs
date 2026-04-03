use super::transcript_helpers::*;
use super::*;
use crate::status_contract::{
    completed_status_label, is_waiting_placeholder, pending_status_label, waiting_for_response_line,
};
use std::io::Write;

impl TaskDraw {
    /// Tool activity renders as paragraph-style transcript blocks with stable
    /// 2/4/6-space disclosure levels:
    ///
    /// ```text
    ///   ✦ read_file src/main.rs              ← 2-space: tool activity summary
    ///     Status: Response complete., 42 lines ← 4-space: phase detail
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
            draw_status_paragraph_header(w, YELLOW, "\u{2726}", rest, cols, WHITE);
            return;
        }
        if is_waiting_placeholder(line) {
            let idx = (self.frame_counter as usize) % SPINNER_FRAMES.len();
            set_fg(w, MAGENTA);
            let _ = write!(w, " {} {}", SPINNER_FRAMES[idx], pending_status_label());
            if let Some(suffix) = line.strip_prefix(waiting_for_response_line()) {
                let suffix = suffix.trim();
                if !suffix.is_empty() {
                    reset_style(w);
                    set_dim(w);
                    set_fg(w, DIM_GRAY);
                    let available = (cols as usize).saturating_sub(
                        display_width(SPINNER_FRAMES[idx])
                            + display_width(pending_status_label())
                            + 3,
                    );
                    let truncated = truncate_to_width(suffix, available);
                    let _ = write!(w, " {truncated}");
                }
            }
            reset_style(w);
            return;
        }
        if is_inline_telemetry_summary(line) {
            draw_inline_telemetry_summary(w, line, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[thinking] ") {
            draw_status_paragraph_header(w, MAGENTA, "\u{22ef}", rest, cols, MAGENTA);
            return;
        }
        if let Some(rest) = line.strip_prefix("[thinking_detail] ") {
            draw_prefixed_disclosure_line(w, "    ", None, rest, cols, MAGENTA, GRAY, true);
            return;
        }
        if let Some(rest) = line.strip_prefix("[approval] ") {
            draw_status_paragraph_header(w, YELLOW, "\u{2606}", rest, cols, YELLOW);
            return;
        }
        if let Some(rest) = line.strip_prefix("[approval_detail] ") {
            draw_nested_disclosure_line(self, w, rest, cols, "    ", None, GRAY, YELLOW);
            return;
        }
        if let Some(rest) = line.strip_prefix("[error] ") {
            draw_status_paragraph_header(w, RED, "\u{2716}", rest, cols, RED);
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
                .map(|pid| {
                    format!(
                        "command session \u{00b7} {command} \u{00b7} pid {pid} \u{00b7} {}",
                        pending_status_label()
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "command session \u{00b7} {command} \u{00b7} {}",
                        pending_status_label()
                    )
                });
            draw_tool_paragraph_header(w, &summary, cols);
            return;
        }
        if let Some(rest) = line.strip_prefix("[command session exit: ") {
            draw_nested_disclosure_line(
                self,
                w,
                &format!("Exit: {}", rest.trim_end_matches(']')),
                cols,
                "    ",
                None,
                GRAY,
                MAGENTA,
            );
            return;
        }
        if line == "[command session cancelled]" {
            draw_nested_disclosure_line(
                self,
                w,
                "Status: cancelled",
                cols,
                "    ",
                None,
                GRAY,
                MAGENTA,
            );
            return;
        }
        if line == "[command session cancellation requested]" {
            draw_nested_disclosure_line(
                self,
                w,
                "Status: cancellation requested",
                cols,
                "    ",
                None,
                GRAY,
                MAGENTA,
            );
            return;
        }
        if let Some(rest) = line.strip_prefix("[command session] error: ") {
            draw_status_paragraph_header(w, RED, "\u{2716}", rest, cols, RED);
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

        // ── Edit loop paragraph markers ──────────────────────────
        if let Some(rest) = line.strip_prefix("[edit loop turn ") {
            // Renders: "⟳ turn 2/6" with progress accent.
            let label = format!("turn {}", rest.trim_end_matches(']'));
            draw_status_paragraph_header(w, CYAN, "\u{27f3}", &label, cols, CYAN);
            return;
        }
        if line == "[edit loop: running validation]" {
            draw_status_paragraph_header(w, YELLOW, "\u{2699}", "running validation", cols, YELLOW);
            return;
        }
        if line == "[edit loop: validation passed]" {
            draw_icon_line(w, GREEN, "\u{2714}", WHITE, "validation passed", cols, true);
            return;
        }
        if line == "[edit loop: validation failed, retrying]" {
            draw_icon_line(
                w,
                RED,
                "\u{2716}",
                WHITE,
                "validation failed, retrying",
                cols,
                true,
            );
            return;
        }
        if line == "[edit loop: no patch applied, retrying]" {
            draw_icon_line(
                w,
                YELLOW,
                "\u{26a0}",
                WHITE,
                "no patch applied, retrying",
                cols,
                true,
            );
            return;
        }
        if let Some(rest) = line.strip_prefix("[edit loop complete: ") {
            let detail = rest.trim_end_matches(']');
            draw_icon_line(w, GREEN, "\u{2605}", WHITE, detail, cols, true);
            return;
        }
        if let Some(rest) = line.strip_prefix("[edit loop reached max turns") {
            let detail = rest.trim_end_matches(']').trim_start_matches(" — ");
            let text = if detail.is_empty() {
                "reached max turns".to_string()
            } else {
                format!("max turns: {detail}")
            };
            draw_icon_line(w, YELLOW, "\u{26a0}", WHITE, &text, cols, true);
            return;
        }
        if let Some(rest) = line.strip_prefix("[edit loop warning: ") {
            let detail = rest.trim_end_matches(']');
            draw_icon_line(w, YELLOW, "\u{26a0}", WHITE, detail, cols, true);
            return;
        }
        if let Some(rest) = line.strip_prefix("[edit loop turn error: ") {
            let detail = rest.trim_end_matches(']');
            draw_icon_line(w, RED, "\u{2716}", WHITE, detail, cols, true);
            return;
        }
        if line == "[edit loop aborted: approval denied]" {
            draw_icon_line(
                w,
                RED,
                "\u{2716}",
                WHITE,
                "edit loop aborted: approval denied",
                cols,
                true,
            );
            return;
        }
        if line == "[edit loop cancelled]" {
            draw_icon_line(
                w,
                YELLOW,
                "\u{26a0}",
                WHITE,
                "edit loop cancelled",
                cols,
                true,
            );
            return;
        }
        if line.starts_with("[edit loop") {
            let inner = line.trim_start_matches('[').trim_end_matches(']');
            draw_prefixed_disclosure_line(w, "  ", None, inner, cols, CYAN, GRAY, true);
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
                draw_icon_line(w, GREEN, "\u{2611}", WHITE, task_rest, cols, false);
                return;
            }
            if let Some(task_rest) = rest.strip_prefix("[ ] ") {
                draw_icon_line(w, DIM_GRAY, "\u{2610}", WHITE, task_rest, cols, false);
                return;
            }
            draw_icon_line(w, YELLOW, "\u{2022}", WHITE, rest, cols, false);
            return;
        }

        // ── Numbered lists ─────────────────────────────────────────
        if let Some(num_rest) = parse_numbered_list_item(line) {
            set_fg(w, YELLOW);
            let _ = write!(w, " {}", num_rest.0);
            reset_style(w);
            set_fg(w, WHITE);
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

        // ── Inline telemetry summary (ADR-040) ───────────────────
        // Turn completion emits: [ttft:0.3s | ↑:2.5s (2641 tok) | … | total:7.7s]
        if is_telemetry_summary(line) {
            draw_telemetry_summary(w, line, cols);
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
        if line == "Turn completed."
            || line == completed_status_label()
            || line.starts_with("Type a prompt")
        {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let truncated = truncate_to_width(line, cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Regular text with inline bold detection ────────────────
        if let Some(rest) = line.strip_suffix('▌') {
            let max_text = (cols as usize).saturating_sub(1);
            set_fg(w, WHITE);
            self.draw_inline_markdown(w, rest, max_text as u16);
            if cols > 0 {
                set_bold(w);
                set_fg(w, CYAN);
                let _ = write!(w, "\u{258c}");
            }
            reset_style(w);
            return;
        }
        set_fg(w, WHITE);
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
                    set_fg(w, WHITE);
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
                    set_fg(w, WHITE);
                    let truncated = truncate_to_width(&italic, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    reset_style(w);
                    set_fg(w, WHITE);
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
                    set_fg(w, WHITE);
                    let truncated = truncate_to_width(&struck, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    reset_style(w);
                    set_fg(w, WHITE);
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
                    set_fg(w, WHITE);
                    let truncated = truncate_to_width(&code, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    set_fg(w, WHITE);
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

// ── Delta-native transcript rendering ──────────────────────────────

#[allow(dead_code)]
impl TaskDraw {
    /// Apply a structured transcript delta directly to the draw
    /// engine's line buffer, bypassing prefix-marker parsing.
    ///
    /// Lines touched by the delta are marked dirty for the next
    /// incremental redraw pass. This is the entry point for the
    /// delta-native rendering path described in ADR-041.
    pub(super) fn apply_transcript_delta(
        &mut self,
        delta: &crate::state::TranscriptDelta,
        output_rows: &mut Vec<String>,
        cols: u16,
    ) {
        use crate::state::TranscriptBlockKind;

        let formatted = format_compact_paragraph(&delta.text, delta.block_kind, cols as usize);
        if formatted.is_empty() && !delta.is_complete {
            return;
        }

        for line in formatted.lines() {
            output_rows.push(line.to_string());
        }

        // Apply block-kind-specific styling hint for the last line.
        if delta.is_complete {
            match delta.block_kind {
                TranscriptBlockKind::ToolCall | TranscriptBlockKind::ToolResult => {
                    // Completed tool blocks get a thin separator.
                    // (The actual rendering happens in draw_transcript_line
                    // via the standard text path.)
                }
                TranscriptBlockKind::Thinking | TranscriptBlockKind::FinalText => {}
            }
        }
    }

    /// Consume a batch of transcript deltas and apply them to the
    /// output rows buffer. Returns true if any rows were added
    /// (signalling the caller to trigger a redraw).
    pub(super) fn consume_transcript_deltas(
        &mut self,
        deltas: &[crate::state::TranscriptDelta],
        output_rows: &mut Vec<String>,
        cols: u16,
    ) -> bool {
        let before = output_rows.len();
        for delta in deltas {
            self.apply_transcript_delta(delta, output_rows, cols);
        }
        output_rows.len() > before
    }
}
