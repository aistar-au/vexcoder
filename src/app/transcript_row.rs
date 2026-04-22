#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptRow {
    UserInput(String),

    AssistantText { text: String, streaming: bool },

    ToolHeader(String),

    ToolDetail(String),

    Evidence(String),

    Error(String),

    WaitingPlaceholder(String),

    Plain(String),
}

impl TranscriptRow {
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
