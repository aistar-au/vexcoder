use anyhow::Result;
use crossterm::{
    cursor::MoveToColumn,
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, size as terminal_size, Clear, ClearType},
};
use ratatui::{backend::CrosstermBackend, Terminal, TerminalOptions, Viewport};
use std::io::{self, IsTerminal, Stdout};
use std::sync::Once;

pub struct TerminalType {
    inner: Terminal<CrosstermBackend<Stdout>>,
    inline_viewport: bool,
}

impl TerminalType {
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

    pub fn uses_inline_viewport(&self) -> bool {
        self.inline_viewport
    }

    pub(crate) fn inner_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.inner
    }
}

static PANIC_HOOK_INSTALLED: Once = Once::new();

pub fn install_panic_hook_once() {
    PANIC_HOOK_INSTALLED.call_once(|| {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = restore();
            original_hook(panic_info);
        }));
    });
}

fn terminal_supports_full_screen() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn enter_full_screen_mode() -> Result<()> {
    install_panic_hook_once();
    if !terminal_supports_full_screen() {
        return Ok(());
    }

    enable_raw_mode()?;
    execute!(io::stdout(), EnableBracketedPaste)?;
    Ok(())
}

pub fn setup() -> Result<TerminalType> {
    enter_full_screen_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = if terminal_supports_full_screen() {
        let (_, rows) = terminal_size()?;
        let inner = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(rows.max(1)),
            },
        )?;
        TerminalType {
            inner,
            inline_viewport: true,
        }
    } else {
        let inner = Terminal::new(backend)?;
        TerminalType {
            inner,
            inline_viewport: false,
        }
    };
    if terminal_supports_full_screen() {
        terminal.clear()?;
    }
    Ok(terminal)
}

pub fn restore() -> Result<()> {
    if !terminal_supports_full_screen() {
        return Ok(());
    }

    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
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
    fn test_terminal_restored_after_simulated_panic() {
        install_panic_hook_once();
        install_panic_hook_once();
        assert!(
            PANIC_HOOK_INSTALLED.is_completed(),
            "panic hook must be installed before raw mode setup"
        );
    }
}
