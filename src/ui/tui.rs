pub mod backend {
    pub use ratatui::backend::{CrosstermBackend, TestBackend};
}

pub mod input {
    pub use crossterm::event::{
        DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent as KeyStroke,
        KeyEventKind as KeyStrokeKind, KeyModifiers, poll, read,
    };
}

pub mod layout {
    pub use ratatui::layout::HorizontalAlignment as Alignment;
    pub use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
}

pub mod style {
    pub use ratatui::style::{Color, Modifier, Style};
}

pub mod text {
    pub use ratatui::text::{Line, Span, Text};
}

pub mod widgets {
    pub use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
}

pub mod terminal {
    pub use crossterm::cursor::{MoveToColumn, Show};
    pub use crossterm::terminal::{Clear, ClearType, size as host_display_size};
}

pub use crossterm::execute;
pub use ratatui::{DefaultTerminal, Frame, Terminal, TerminalOptions, Viewport};
pub use ratatui::{try_init_with_options, try_restore};
pub use ratatui_macros::{line, span};
