//! Key presses in watch mode, without taking over the terminal like the TUI does: log and counter lines keep scrolling.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::io::IsTerminal;
use std::time::Duration;

/// Watches for `q` (or Esc) on stdin.
///
/// On Unix only line buffering and echo are turned off, unlike raw mode, which would also stop `\n` from returning to
/// the start of the line and Ctrl+C from sending SIGINT.
pub struct QuitKey(());

impl QuitKey {
    /// `None` if stdin is not a terminal.
    pub fn new() -> anyhow::Result<Option<Self>> {
        if !std::io::stdin().is_terminal() {
            return Ok(None);
        }
        term::enable()?;
        Ok(Some(QuitKey(())))
    }

    /// Wait for up to `timeout`, `true` if quit was pressed.
    pub fn wait(&mut self, timeout: Duration) -> anyhow::Result<bool> {
        if !event::poll(timeout)? {
            return Ok(false);
        }
        Ok(match event::read()? {
            Event::Key(KeyEvent {
                code,
                modifiers,
                kind: KeyEventKind::Press,
                ..
            }) => match code {
                KeyCode::Char('q') | KeyCode::Esc => true,
                // Arrives as a key on Windows, where raw mode turns off processed input
                KeyCode::Char('c') => modifiers.contains(KeyModifiers::CONTROL),
                _ => false,
            },
            _ => false,
        })
    }
}

impl Drop for QuitKey {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// Restore the terminal mode changed by [`QuitKey`], also for exiting without unwinding.
pub fn restore_terminal() {
    term::restore();
}

#[cfg(unix)]
mod term {
    use rustix::termios::{self, LocalModes, OptionalActions, SpecialCodeIndex, Termios};
    use std::sync::Mutex;

    static ORIGINAL: Mutex<Option<Termios>> = Mutex::new(None);

    pub fn enable() -> anyhow::Result<()> {
        let stdin = std::io::stdin();
        let original = termios::tcgetattr(&stdin)?;
        let mut mode = original.clone();
        mode.local_modes
            .remove(LocalModes::ICANON | LocalModes::ECHO);
        mode.special_codes[SpecialCodeIndex::VMIN] = 1;
        mode.special_codes[SpecialCodeIndex::VTIME] = 0;
        termios::tcsetattr(&stdin, OptionalActions::Now, &mode)?;
        *ORIGINAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(original);
        Ok(())
    }

    pub fn restore() {
        if let Some(original) = ORIGINAL.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = termios::tcsetattr(std::io::stdin(), OptionalActions::Now, &original);
        }
    }
}

/// Raw mode on Windows only changes how input is read, output is unaffected.
#[cfg(not(unix))]
mod term {
    pub fn enable() -> anyhow::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(())
    }

    pub fn restore() {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}
