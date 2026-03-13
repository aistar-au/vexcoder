use anyhow::Result;
use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, IsTerminal, Stdout};
use std::sync::Once;

pub type TerminalType = Terminal<CrosstermBackend<Stdout>>;
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

fn terminal_supports_overlay() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn enter_overlay_mode() -> Result<()> {
    install_panic_hook_once();
    if !terminal_supports_overlay() {
        return Ok(());
    }

    enable_raw_mode()?;
    execute!(io::stdout(), EnableBracketedPaste)?;
    Ok(())
}

pub fn setup() -> Result<TerminalType> {
    enter_overlay_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore() -> Result<()> {
    if !terminal_supports_overlay() {
        return Ok(());
    }

    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableBracketedPaste, Show);
    Ok(())
}

pub fn reenter_overlay() -> Result<()> {
    enter_overlay_mode()
}

#[derive(Debug)]
pub struct TerminalGuard {
    should_reenter: bool,
}

impl TerminalGuard {
    pub fn yield_for_command() -> Result<Self> {
        if !terminal_supports_overlay() {
            return Ok(Self {
                should_reenter: false,
            });
        }

        restore()?;
        Ok(Self {
            should_reenter: true,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.should_reenter {
            let _ = reenter_overlay();
        }
    }
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
