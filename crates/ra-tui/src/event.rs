//! Terminal event handling for the TUI.
//!
//! Bridges crossterm input events into [`TuiEvent`] values that the
//! application state machine consumes.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};

/// Events consumed by the TUI application.
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// A terminal key press.
    Key(KeyEvent),
    /// A periodic tick for animations and auto-play.
    Tick,
    /// Terminal resize.
    Resize(u16, u16),
}

/// Spawns a background thread that polls crossterm and sends
/// [`TuiEvent`] values over a channel.
pub struct EventHandler {
    rx: mpsc::Receiver<TuiEvent>,
    _handle: thread::JoinHandle<()>,
}

impl EventHandler {
    /// Start the event loop with the given tick interval.
    #[must_use]
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    match event::read() {
                        Ok(CrosstermEvent::Key(key)) => {
                            if tx.send(TuiEvent::Key(key)).is_err() {
                                return;
                            }
                        }
                        Ok(CrosstermEvent::Resize(w, h)) => {
                            if tx.send(TuiEvent::Resize(w, h)).is_err() {
                                return;
                            }
                        }
                        _ => {}
                    }
                } else if tx.send(TuiEvent::Tick).is_err() {
                    return;
                }
            }
        });

        Self {
            rx,
            _handle: handle,
        }
    }

    /// Receive the next event, blocking until one arrives.
    ///
    /// # Errors
    ///
    /// Returns an error if the event channel is disconnected.
    pub fn next(&self) -> anyhow::Result<TuiEvent> {
        self.rx
            .recv()
            .map_err(|e| anyhow::anyhow!("event channel closed: {e}"))
    }
}

/// Map a key event to a named action for the app state machine.
#[must_use]
pub fn key_action(key: &KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c'))
        | (_, KeyCode::Char('q')) => Action::Quit,
        (_, KeyCode::Right | KeyCode::Char('l')) => Action::NextStep,
        (_, KeyCode::Left | KeyCode::Char('h')) => Action::PrevStep,
        (_, KeyCode::Char(' ')) => Action::TogglePlay,
        (_, KeyCode::Char('+' | '=')) => {
            Action::SpeedUp
        }
        (_, KeyCode::Char('-')) => Action::SlowDown,
        (_, KeyCode::Tab) => Action::NextPanel,
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            Action::PrevPanel
        }
        (_, KeyCode::Up | KeyCode::Char('k')) => Action::ScrollUp,
        (_, KeyCode::Down | KeyCode::Char('j')) => {
            Action::ScrollDown
        }
        (_, KeyCode::Home) => Action::FirstStep,
        (_, KeyCode::End) => Action::LastStep,
        (_, KeyCode::Char('?')) => Action::ToggleHelp,
        _ => Action::None,
    }
}

/// Named actions that the app state machine handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Exit the application.
    Quit,
    /// Advance to the next optimization step.
    NextStep,
    /// Go back to the previous step.
    PrevStep,
    /// Jump to the first step.
    FirstStep,
    /// Jump to the last step.
    LastStep,
    /// Toggle auto-play.
    TogglePlay,
    /// Increase playback speed.
    SpeedUp,
    /// Decrease playback speed.
    SlowDown,
    /// Focus the next panel.
    NextPanel,
    /// Focus the previous panel.
    PrevPanel,
    /// Scroll the focused panel up.
    ScrollUp,
    /// Scroll the focused panel down.
    ScrollDown,
    /// Toggle the help overlay.
    ToggleHelp,
    /// No-op.
    None,
}
