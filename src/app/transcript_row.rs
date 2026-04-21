/// A typed transcript row.
///
/// Replaces the `"[marker] text"` string protocol that previously coupled
/// `transcript_projection` to `transcript.rs`.  The renderer dispatches on
/// enum variants instead of stripping marker prefixes.
///
/// The `Plain` variant carries free-form strings from runtime event notices
/// (approval, command session, timing summaries, etc.) that are not yet
/// lifted into dedicated entry types in `TurnEntry`.
#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptRow {
    /// User input echoed to the transcript (`> {text}` in string form).
    UserInput(String),
    /// Model response text, potentially with a streaming cursor already
    /// appended to the last line.
    AssistantText { text: String, streaming: bool },
    /// Tool call header: `"name · target · status"`.
    ToolHeader(String),
    /// Tool call detail line (input preview first line or result summary).
    ToolDetail(String),
    /// Tool output evidence line (dimmed, indented).
    Evidence(String),
    /// System error notice.
    Error(String),
    /// Waiting-for-response placeholder line (may include elapsed time suffix).
    WaitingPlaceholder(String),
    /// Free-form system notice.  May still carry older `[marker]` prefixes
    /// from runtime strings; the renderer handles those inside this variant
    /// until they are promoted to dedicated `TurnEntry` types in a later PR.
    Plain(String),
}

impl TranscriptRow {
    /// Text used for display-width calculations and word-wrap.
    pub fn as_display_str(&self) -> &str {
        match self {
            Self::AssistantText { text, .. } => text,
            Self::ToolHeader(s)
            | Self::ToolDetail(s)
            | Self::Evidence(s)
            | Self::Error(s)
            | Self::UserInput(s)
            | Self::WaitingPlaceholder(s)
            | Self::Plain(s) => s,
        }
    }

    /// Produce a clone that replaces the inner text with `text`.
    /// Used by `expand_rows_for_display` to propagate the row type through
    /// word-wrap expansion.
    pub fn clone_with_text(&self, text: String) -> Self {
        match self {
            Self::UserInput(_) => Self::UserInput(text),
            Self::AssistantText { streaming, .. } => Self::AssistantText {
                text,
                streaming: *streaming,
            },
            Self::ToolHeader(_) => Self::ToolHeader(text),
            Self::ToolDetail(_) => Self::ToolDetail(text),
            Self::Evidence(_) => Self::Evidence(text),
            Self::Error(_) => Self::Error(text),
            Self::WaitingPlaceholder(_) => Self::WaitingPlaceholder(text),
            Self::Plain(_) => Self::Plain(text),
        }
    }

    /// Convert back to the earlier marker-string representation.
    ///
    /// Used by `history_lines()` and `handle_copy_command` which return
    /// `Vec<String>` and are relied on by existing tests.
    pub fn to_history_string(&self) -> String {
        match self {
            Self::UserInput(s) => format!("> {s}"),
            Self::AssistantText { text, .. } => text.clone(),
            Self::ToolHeader(s) => format!("[tool] {s}"),
            Self::ToolDetail(s) => format!("[detail] {s}"),
            Self::Evidence(s) => format!("[evidence] {s}"),
            Self::Error(s) => format!("[error] {s}"),
            Self::WaitingPlaceholder(s) => s.clone(),
            Self::Plain(s) => s.clone(),
        }
    }
}
