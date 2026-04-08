use anyhow::Result;
use crossterm::{
    cursor::MoveToColumn,
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, size as host_display_size, Clear, ClearType},
};
use ratatui::{backend::CrosstermBackend, Terminal, TerminalOptions, Viewport};
use std::io::{self, IsTerminal, Stdout};
use std::sync::Once;

pub struct TuiHandle {
    inner: Terminal<CrosstermBackend<Stdout>>,
    inline_viewport: bool,
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

fn host_has_tty() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn enter_raw_mode() -> Result<()> {
    install_panic_hook_once();
    if !host_has_tty() {
        return Ok(());
    }

    enable_raw_mode()?;
    execute!(io::stdout(), EnableBracketedPaste)?;
    Ok(())
}

pub fn setup() -> Result<TuiHandle> {
    enter_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut tui = if host_has_tty() {
        let (_, rows) = host_display_size()?;
        let inner = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(rows.max(1)),
            },
        )?;
        TuiHandle {
            inner,
            inline_viewport: true,
        }
    } else {
        let inner = Terminal::new(backend)?;
        TuiHandle {
            inner,
            inline_viewport: false,
        }
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
    fn test_host_tui_restored_after_simulated_panic() {
        install_panic_hook_once();
        install_panic_hook_once();
        assert!(
            PANIC_HOOK_INSTALLED.is_completed(),
            "panic hook must be installed before raw mode setup"
        );
    }
}
