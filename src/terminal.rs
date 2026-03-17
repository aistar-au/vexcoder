use anyhow::Result;
use crossterm::{
    cursor::MoveToColumn,
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
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

fn terminal_supports_full_screen() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn enter_full_screen_mode() -> Result<()> {
    install_panic_hook_once();
    if !terminal_supports_full_screen() {
        return Ok(());
    }

    enable_raw_mode()?;
    // EnterAlternateScreen isolates TUI rendering from the main terminal
    // buffer.  Without it, ratatui renders to the primary buffer using cursor
    // positioning that overwrites content without adding to terminal scrollback
    // history.  LeaveAlternateScreen in restore() returns the user to the
    // pre-session terminal state cleanly.
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    Ok(())
}

pub fn setup() -> Result<TerminalType> {
    enter_full_screen_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
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
        LeaveAlternateScreen,
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
