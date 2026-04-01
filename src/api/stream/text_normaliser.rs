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

        for line in text.split('\n') {
            let trimmed = line.trim();

            if let Some(tool_name) = parse_embedded_function_open(trimmed) {
                if self.in_tool_block {
                    self.flush_stale_tool_block(&mut output);
                }
                self.in_tool_block = true;
                self.current_tool_name = Some(tool_name.clone());
                self.current_param_name = None;
                self.current_param_value.clear();
                output.push(NormalisedChunk::TranscriptLine(format!(
                    "[tool] {tool_name} · processing"
                )));
                continue;
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
                continue;
            }

            if self.in_tool_block {
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
                    continue;
                }
                if is_embedded_parameter_close(trimmed) {
                    if let Some(param_name) = self.current_param_name.take() {
                        let value = std::mem::take(&mut self.current_param_value);
                        let compact = compact_param_value(&value);
                        output.push(NormalisedChunk::TranscriptLine(format!(
                            "[detail] {param_name}: {compact}"
                        )));
                    }
                    continue;
                }
                if self.current_param_name.is_some() {
                    if !self.current_param_value.is_empty() {
                        self.current_param_value.push('\n');
                    }
                    self.current_param_value.push_str(line);
                    continue;
                }
                if !trimmed.is_empty() {
                    output.push(NormalisedChunk::Text(line.to_string()));
                }
                continue;
            }

            if trimmed.is_empty() {
                self.consecutive_blanks += 1;
                if self.consecutive_blanks <= 1 {
                    output.push(NormalisedChunk::Text(String::new()));
                }
                continue;
            }
            self.consecutive_blanks = 0;
            output.push(NormalisedChunk::Text(line.to_string()));
        }

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
        if self.in_tool_block {
            self.flush_stale_tool_block(&mut output);
        }
        self.consecutive_blanks = 0;
        output
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
    let trimmed = text.trim().trim_end_matches('>');
    if let Some(name) = trimmed.strip_prefix("function=") {
        let name = name.trim().trim_matches(|c| c == '<' || c == '>');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    if let Some(rest) = trimmed.strip_prefix("<function=") {
        let name = rest.trim().trim_matches(|c| c == '<' || c == '>');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    None
}

fn is_embedded_function_close(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed == "function>"
        || trimmed == "</function>"
        || trimmed == "</function"
        || trimmed == "function"
}

fn parse_embedded_parameter_open(text: &str) -> Option<String> {
    let trimmed = text.trim().trim_end_matches('>');
    if let Some(name) = trimmed.strip_prefix("parameter=") {
        let name = name.trim().trim_matches(|c| c == '<' || c == '>');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    if let Some(rest) = trimmed.strip_prefix("<parameter=") {
        let name = rest.trim().trim_matches(|c| c == '<' || c == '>');
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(name.to_string());
        }
    }
    None
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
