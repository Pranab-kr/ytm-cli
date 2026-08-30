mod config;
mod logging;

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout};

/// Restores the terminal on drop, including during a panic unwind (NFR-5).
// Constructed by the event loop in Task 22; nothing enters the alternate
// screen yet.
#[allow(dead_code)]
pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

#[allow(dead_code)]
impl TerminalGuard {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, crossterm::cursor::Hide)?;
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(out))?,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    // Held for the process lifetime; dropping it loses buffered log lines.
    let _log_guard = logging::init(&config::paths::log_dir())?;
    let cfg = config::Config::load(None)?;
    tracing::info!(
        auth = ?cfg.auth.kind,
        volume = cfg.playback.volume,
        vim_keys = cfg.ui.vim_keys,
        "config loaded"
    );
    Ok(())
}
