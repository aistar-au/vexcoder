// Keep ratatui imports and version-sensitive type names behind one local facade.
// Future ratatui upgrades should mostly land here instead of leaking across UI code.

pub mod backend {
    pub use ratatui::backend::{CrosstermBackend, TestBackend};
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

pub use ratatui::{DefaultTerminal, Frame, Terminal, TerminalOptions, Viewport};
pub use ratatui::{try_init_with_options, try_restore};
pub use ratatui_macros::{line, span};
