// Detects and strips embedded tool call markup from server text content
// that arrives outside the structured `tool_calls` field. Some local
// inference servers emit tool invocations inline as XML-like tags within
// the assistant text response. This normaliser converts those fragments
// into structured transcript rows so the TUI never renders raw markup.

/// State machine for detecting embedded tool call markup in text deltas.
#[derive(Default)]
pub struct StreamTextNormaliser {
    /// Accumulated text buffer for detecting multi-line tool call patterns.
    pub(super) pending: String,
    /// Whether we are currently inside an embedded tool call block.
    pub(super) in_tool_block: bool,
    /// Tool name captured from `function=<name>` open tag.
    pub(super) current_tool_name: Option<String>,
    /// Parameter name captured from `parameter=<name>` open tag.
    pub(super) current_param_name: Option<String>,
    /// Accumulated parameter value lines.
    pub(super) current_param_value: String,
    /// Count of consecutive blank lines emitted (for collapsing).
    pub(super) consecutive_blanks: usize,
}

/// Result of normalising a text delta.
pub enum NormalisedChunk {
    /// Clean text to pass through to the TUI as a stream delta.
    Text(String),
    /// A structured transcript line (e.g. `[tool] ...`, `[detail] ...`).
    TranscriptLine(String),
}

impl StreamTextNormaliser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a text delta and return normalised output chunks.
    pub fn normalise(&mut self, text: &str) -> Vec<NormalisedChunk> {
        let mut output = Vec::new();
        self.pending.push_str(text);
        self.drain_pending_lines(&mut output, false);
        while self.drain_complete_control_fragment(&mut output)
            || self.emit_safe_pending_text(&mut output)
        {}

        output
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.in_tool_block = false;
        self.current_tool_name = None;
        self.current_param_name = None;
        self.current_param_value.clear();
        self.consecutive_blanks = 0;
    }

    pub fn flush(&mut self) -> Vec<NormalisedChunk> {
        let mut output = Vec::new();
        self.drain_pending_lines(&mut output, true);
        while self.drain_complete_control_fragment(&mut output)
            || self.emit_safe_pending_text(&mut output)
        {}
        if self.in_tool_block {
            self.flush_stale_tool_block(&mut output);
        }
        self.pending.clear();
        self.consecutive_blanks = 0;
        output
    }

    fn drain_pending_lines(&mut self, output: &mut Vec<NormalisedChunk>, flush_tail_line: bool) {
        let mut consumed = 0usize;
        while let Some(rel) = self.pending[consumed..].find('\n') {
            let line_end = consumed + rel;
            let line = self.pending[consumed..line_end]
                .strip_suffix('\r')
                .unwrap_or(&self.pending[consumed..line_end])
                .to_string();
            self.process_line(&line, output);
            consumed = line_end + 1;
        }

        if consumed > 0 {
            self.pending.drain(..consumed);
        }

        if flush_tail_line && !self.pending.is_empty() {
            let tail = std::mem::take(&mut self.pending);
            self.process_line(tail.strip_suffix('\r').unwrap_or(&tail), output);
        }
    }

    fn drain_complete_control_fragment(&mut self, output: &mut Vec<NormalisedChunk>) -> bool {
        let Some(fragment_len) = complete_control_fragment_len(&self.pending) else {
            return false;
        };
        let fragment = self.pending[..fragment_len].to_string();
        self.pending.drain(..fragment_len);
        self.process_line(fragment.trim(), output);
        true
    }

    fn emit_safe_pending_text(&mut self, output: &mut Vec<NormalisedChunk>) -> bool {
        if self.pending.is_empty() || self.in_tool_block || self.current_param_name.is_some() {
            return false;
        }

        let emit_len = match pending_tool_tag_suffix_start(&self.pending) {
            Some(0) => return false,
            Some(index) => index,
            None => self.pending.len(),
        };

        if emit_len == 0 {
            return false;
        }

        let safe_text = self.pending[..emit_len].to_string();
        self.pending.drain(..emit_len);
        output.push(NormalisedChunk::Text(safe_text));
        self.consecutive_blanks = 0;
        true
    }

    fn process_line(&mut self, line: &str, output: &mut Vec<NormalisedChunk>) {
        let trimmed = line.trim();

        if is_tool_call_wrapper_line(trimmed) {
            return;
        }

        if let Some(tool_name) = parse_embedded_function_open(trimmed) {
            if self.in_tool_block {
                self.flush_stale_tool_block(output);
            }
            self.in_tool_block = true;
            self.current_tool_name = Some(tool_name.clone());
            self.current_param_name = None;
            self.current_param_value.clear();
            output.push(NormalisedChunk::TranscriptLine(format!(
                "[tool] {tool_name} · processing"
            )));
            return;
        }

        if self.in_tool_block && is_embedded_function_close(trimmed) {
            let tool_name = self
                .current_tool_name
                .take()
                .unwrap_or_else(|| "unknown".to_string());
            if let Some(param_name) = self.current_param_name.take() {
                let value = std::mem::take(&mut self.current_param_value);
                let compact = compact_param_value(&value);
                output.push(NormalisedChunk::TranscriptLine(format!(
                    "[detail] {param_name}: {compact}"
                )));
            }
            output.push(NormalisedChunk::TranscriptLine(format!(
                "[tool] {tool_name} · dispatched"
            )));
            self.in_tool_block = false;
            return;
        }

        if self.in_tool_block {
            if self.current_param_name.is_some() {
                if let Some(index) = function_open_transition_index(line).filter(|idx| *idx > 0) {
                    let prefix = &line[..index];
                    if !prefix.is_empty() {
                        self.current_param_value.push_str(prefix);
                    }
                    if let Some(param_name) = self.current_param_name.take() {
                        let value = std::mem::take(&mut self.current_param_value);
                        let compact = compact_param_value(&value);
                        output.push(NormalisedChunk::TranscriptLine(format!(
                            "[detail] {param_name}: {compact}"
                        )));
                    }
                    self.flush_stale_tool_block(output);
                    self.process_line(&line[index..], output);
                    return;
                }
            }
            if let Some(param_name) = parse_embedded_parameter_open(trimmed) {
                if let Some(prev_name) = self.current_param_name.take() {
                    let value = std::mem::take(&mut self.current_param_value);
                    let compact = compact_param_value(&value);
                    output.push(NormalisedChunk::TranscriptLine(format!(
                        "[detail] {prev_name}: {compact}"
                    )));
                }
                self.current_param_name = Some(param_name);
                self.current_param_value.clear();
                return;
            }
            if is_embedded_parameter_close(trimmed) {
                if let Some(param_name) = self.current_param_name.take() {
                    let value = std::mem::take(&mut self.current_param_value);
                    let compact = compact_param_value(&value);
                    output.push(NormalisedChunk::TranscriptLine(format!(
                        "[detail] {param_name}: {compact}"
                    )));
                }
                return;
            }
            if self.current_param_name.is_some() {
                if !self.current_param_value.is_empty() {
                    self.current_param_value.push('\n');
                }
                self.current_param_value.push_str(line);
                return;
            }
            if !trimmed.is_empty() {
                output.push(NormalisedChunk::Text(line.to_string()));
            }
            return;
        }

        if trimmed.is_empty() {
            self.consecutive_blanks += 1;
            if self.consecutive_blanks <= 1 {
                output.push(NormalisedChunk::Text(String::new()));
            }
            return;
        }

        self.consecutive_blanks = 0;
        output.push(NormalisedChunk::Text(line.to_string()));
    }

    fn flush_stale_tool_block(&mut self, output: &mut Vec<NormalisedChunk>) {
        if let Some(param_name) = self.current_param_name.take() {
            let value = std::mem::take(&mut self.current_param_value);
            let compact = compact_param_value(&value);
            output.push(NormalisedChunk::TranscriptLine(format!(
                "[detail] {param_name}: {compact}"
            )));
        }
        let tool_name = self
            .current_tool_name
            .take()
            .unwrap_or_else(|| "unknown".to_string());
        output.push(NormalisedChunk::TranscriptLine(format!(
            "[tool] {tool_name} · dispatched"
        )));
        self.in_tool_block = false;
        self.current_param_value.clear();
    }
}

fn parse_embedded_function_open(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if let Some(name) = trimmed.strip_prefix("function=") {
        let name = name.strip_suffix('>')?;
        let name = name.trim().trim_matches(|c| c == '<' || c == '>');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    if let Some(rest) = trimmed.strip_prefix("<function=") {
        let name = rest.strip_suffix('>')?;
        let name = name.trim().trim_matches(|c| c == '<' || c == '>');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    None
}

fn is_tool_call_wrapper_line(text: &str) -> bool {
    matches!(text.trim(), "<tool_call>" | "</tool_call>")
}

fn complete_control_fragment_len(text: &str) -> Option<usize> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.len() != text.len() {
        return None;
    }

    if is_tool_call_wrapper_line(trimmed)
        || is_embedded_function_close(trimmed)
        || is_embedded_parameter_close(trimmed)
        || parse_embedded_function_open(trimmed).is_some()
        || parse_embedded_parameter_open(trimmed).is_some()
    {
        Some(text.len())
    } else {
        None
    }
}

fn pending_tool_tag_suffix_start(text: &str) -> Option<usize> {
    let last_open = text.rfind('<')?;
    let suffix = text[last_open..].to_ascii_lowercase();
    let tool_prefixes = [
        "<tool_call>",
        "<tool_call",
        "</tool_call>",
        "</tool_call",
        "<function=",
        "<function",
        "</function>",
        "</function",
        "<parameter=",
        "<parameter",
        "</parameter>",
        "</parameter",
    ];
    let looks_like_incomplete_named_open = (suffix.starts_with("<function=")
        || suffix.starts_with("<parameter="))
        && !suffix.contains('>');

    (tool_prefixes
        .iter()
        .any(|prefix| prefix.starts_with(&suffix))
        || looks_like_incomplete_named_open)
        .then_some(last_open)
}

fn is_embedded_function_close(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed == "function>"
        || trimmed == "</function>"
        || trimmed == "</function"
        || trimmed == "function"
}

fn parse_embedded_parameter_open(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if let Some(name) = trimmed.strip_prefix("parameter=") {
        let name = name.strip_suffix('>')?;
        let name = name.trim().trim_matches(|c| c == '<' || c == '>');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    if let Some(rest) = trimmed.strip_prefix("<parameter=") {
        let name = rest.strip_suffix('>')?;
        let name = name.trim().trim_matches(|c| c == '<' || c == '>');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    None
}

fn function_open_transition_index(text: &str) -> Option<usize> {
    text.find("<function=").or_else(|| text.find("function="))
}

fn is_embedded_parameter_close(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed == "parameter>"
        || trimmed == "</parameter>"
        || trimmed == "</parameter"
        || trimmed == "parameter"
}

pub(super) fn compact_param_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= 80 {
        return trimmed.replace('\n', " ");
    }
    let first_line = trimmed.lines().next().unwrap_or(trimmed);
    if first_line.chars().count() <= 77 {
        format!("{first_line}\u{2026}")
    } else {
        let truncated: String = first_line.chars().take(77).collect();
        format!("{truncated}\u{2026}")
    }
}
