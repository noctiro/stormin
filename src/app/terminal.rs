use crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{DefaultTerminal, Terminal, backend::CrosstermBackend};
use std::io;

/// Restores terminal state on ordinary errors and panic unwinding.
pub(super) struct TerminalGuard;

pub(super) fn setup_terminal() -> io::Result<(DefaultTerminal, TerminalGuard)> {
    enable_raw_mode()?;
    // Create the guard before any other fallible setup, including partial writes.
    let guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.clear()?;
    Ok((terminal, guard))
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        // Attempt each restoration step even if another one fails.
        let _ = disable_raw_mode();
        let _ = execute!(stdout, DisableMouseCapture);
        let _ = execute!(stdout, LeaveAlternateScreen);
        let _ = execute!(stdout, Show);
    }
}
