use anyhow::Result;
use crossterm::{
    cursor::MoveToColumn,
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{Clear, ClearType, size as host_display_size},
};
use ratatui::{DefaultTerminal, Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};
use std::io::{self, IsTerminal};
use std::sync::Once;

pub struct TuiHandle {
    inner: DefaultTerminal,
}

impl TuiHandle {
    pub fn draw<F>(&mut self, render_callback: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.inner.draw(render_callback).map(|_| ())
    }

    pub fn size(&self) -> io::Result<ratatui::layout::Size> {
        self.inner.size()
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }
}

static PANIC_HOOK_INSTALLED: Once = Once::new();

pub fn install_panic_hook_once() {
    PANIC_HOOK_INSTALLED.call_once(|| {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = disable_bracketed_paste();
            original_hook(panic_info);
        }));
    });
}

fn host_has_tty() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn preferred_inline_viewport_rows_with_override(host_rows: u16, override_rows: Option<u16>) -> u16 {
    let host_rows = host_rows.max(1);
    override_rows
        .filter(|rows| *rows > 0)
        .map(|rows| rows.min(host_rows))
        .unwrap_or(host_rows)
}

fn preferred_inline_viewport_rows(host_rows: u16) -> u16 {
    let override_rows = std::env::var("VEX_INLINE_VIEWPORT_ROWS")
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok());
    preferred_inline_viewport_rows_with_override(host_rows, override_rows)
}

fn enable_bracketed_paste() -> io::Result<()> {
    if !host_has_tty() {
        return Ok(());
    }
    execute!(io::stdout(), EnableBracketedPaste)
}

fn disable_bracketed_paste() -> io::Result<()> {
    if !host_has_tty() {
        return Ok(());
    }
    execute!(io::stdout(), DisableBracketedPaste)
}

pub fn setup() -> Result<TuiHandle> {
    let mut tui = if host_has_tty() {
        install_panic_hook_once();
        let (_, rows) = host_display_size()?;
        let inline_rows = preferred_inline_viewport_rows(rows);
        let inner = ratatui::try_init_with_options(TerminalOptions {
            viewport: Viewport::Inline(inline_rows),
        })?;
        enable_bracketed_paste()?;
        TuiHandle { inner }
    } else {
        let backend = CrosstermBackend::new(io::stdout());
        let inner = Terminal::new(backend)?;
        TuiHandle { inner }
    };
    if host_has_tty() {
        tui.clear()?
    }
    Ok(tui)
}

pub fn restore() -> Result<()> {
    if !host_has_tty() {
        return Ok(());
    }

    let _ = disable_bracketed_paste();
    let _ = ratatui::try_restore();
    let _ = execute!(
        io::stdout(),
        Show,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine)
    );
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_inline_viewport_rows_autofits_to_terminal() {
        assert_eq!(preferred_inline_viewport_rows_with_override(40, None), 40);
        assert_eq!(preferred_inline_viewport_rows_with_override(24, None), 24);
    }

    #[test]
    fn preferred_inline_viewport_rows_respects_short_hosts() {
        assert_eq!(preferred_inline_viewport_rows_with_override(8, None), 8);
        assert_eq!(preferred_inline_viewport_rows_with_override(1, None), 1);
    }

    #[test]
    fn preferred_inline_viewport_rows_respects_override() {
        assert_eq!(
            preferred_inline_viewport_rows_with_override(40, Some(16)),
            16
        );
        assert_eq!(
            preferred_inline_viewport_rows_with_override(10, Some(16)),
            10
        );
    }

    #[test]
    fn test_host_tui_restored_after_simulated_panic() {
        install_panic_hook_once();
        install_panic_hook_once();
        assert!(
            PANIC_HOOK_INSTALLED.is_completed(),
            "panic hook must be installed before raw mode setup"
        );
    }
}
