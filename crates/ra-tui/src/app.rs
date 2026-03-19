//! Application state machine for the TUI.
//!
//! [`App`] owns the timeline data, current playback position,
//! panel focus, and speed settings. It processes [`Action`]s
//! from the event handler and exposes state for the renderer.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::DisableMouseCapture;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::event::{Action, EventHandler, TuiEvent, key_action};
use crate::layout::LayoutMode;
use crate::panels::sql_editor::{
    EditorMode, KeybindingMode, SqlEditor, ViSubMode,
};
use crate::timeline::Timeline;
use crate::ui;

/// Errors from the TUI application.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// IO error from terminal operations.
    #[error("terminal IO error: {0}")]
    Io(#[from] io::Error),
    /// Timeline is empty.
    #[error("timeline has no snapshots to display")]
    EmptyTimeline,
}

/// Which panel is currently focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// Statistics panel (top-left).
    Stats,
    /// Plan tree panel (top-right).
    Plan,
    /// Cost evolution chart (bottom-left).
    Evolution,
    /// Execution feedback (bottom-right).
    Feedback,
    /// SQL editor panel (editor layout only).
    SqlEditor,
}

impl Panel {
    fn next(self, layout: LayoutMode) -> Self {
        match layout {
            LayoutMode::Classic => match self {
                Self::Stats => Self::Plan,
                Self::Plan => Self::Evolution,
                Self::Evolution => Self::Feedback,
                Self::Feedback | Self::SqlEditor => {
                    Self::Stats
                }
            },
            LayoutMode::Editor => match self {
                Self::SqlEditor => Self::Plan,
                Self::Plan => Self::Stats,
                Self::Stats => Self::Feedback,
                Self::Feedback | Self::Evolution => {
                    Self::SqlEditor
                }
            },
        }
    }

    fn prev(self, layout: LayoutMode) -> Self {
        match layout {
            LayoutMode::Classic => match self {
                Self::Stats | Self::SqlEditor => {
                    Self::Feedback
                }
                Self::Plan => Self::Stats,
                Self::Evolution => Self::Plan,
                Self::Feedback => Self::Evolution,
            },
            LayoutMode::Editor => match self {
                Self::SqlEditor => Self::Feedback,
                Self::Plan => Self::SqlEditor,
                Self::Stats => Self::Plan,
                Self::Feedback | Self::Evolution => {
                    Self::Stats
                }
            },
        }
    }
}

/// Playback speed presets as tick multipliers.
const SPEEDS: &[(&str, u64)] = &[
    ("0.25x", 4000),
    ("0.5x", 2000),
    ("1x", 1000),
    ("2x", 500),
    ("4x", 250),
];

/// The TUI application state.
pub struct App {
    /// The timeline being played back.
    pub timeline: Timeline,
    /// Current snapshot index.
    pub current_step: usize,
    /// Whether auto-play is active.
    pub playing: bool,
    /// Index into the `SPEEDS` array.
    pub speed_index: usize,
    /// Which panel is focused.
    pub focused: Panel,
    /// Scroll offset for the focused panel.
    pub scroll_offset: u16,
    /// Whether to show the help overlay.
    pub show_help: bool,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Current layout mode.
    pub layout_mode: LayoutMode,
    /// SQL editor state.
    pub sql_editor: SqlEditor,
    /// Last auto-advance time.
    last_advance: Instant,
}

impl App {
    /// Create a new app from a timeline.
    ///
    /// # Errors
    ///
    /// Returns an error if the timeline is empty.
    pub fn new(
        timeline: Timeline,
    ) -> Result<Self, AppError> {
        if timeline.is_empty() {
            return Err(AppError::EmptyTimeline);
        }
        let sql_editor = SqlEditor::new(&timeline.query);
        Ok(Self {
            timeline,
            current_step: 0,
            playing: false,
            speed_index: 2, // 1x
            focused: Panel::Plan,
            scroll_offset: 0,
            show_help: false,
            should_quit: false,
            layout_mode: LayoutMode::Classic,
            sql_editor,
            last_advance: Instant::now(),
        })
    }

    /// Create a new app with config-driven keybinding mode.
    ///
    /// Loads editor mode from `config.editor.mode` and applies
    /// it to the SQL editor.
    ///
    /// # Errors
    ///
    /// Returns an error if the timeline is empty.
    pub fn with_config(
        timeline: Timeline,
        config: &ra_config::RaConfig,
    ) -> Result<Self, AppError> {
        if timeline.is_empty() {
            return Err(AppError::EmptyTimeline);
        }
        let kb_mode: KeybindingMode =
            config.editor.mode.into();
        let sql_editor = SqlEditor::with_keybinding_mode(
            &timeline.query,
            kb_mode,
        );
        Ok(Self {
            timeline,
            current_step: 0,
            playing: false,
            speed_index: 2,
            focused: Panel::Plan,
            scroll_offset: 0,
            show_help: false,
            should_quit: false,
            layout_mode: LayoutMode::Classic,
            sql_editor,
            last_advance: Instant::now(),
        })
    }

    /// Current playback speed label.
    #[must_use]
    pub fn speed_label(&self) -> &str {
        SPEEDS[self.speed_index].0
    }

    /// Tick duration for the current speed.
    fn tick_duration(&self) -> Duration {
        Duration::from_millis(SPEEDS[self.speed_index].1)
    }

    /// Process an action from the event handler.
    pub fn handle_action(&mut self, action: Action) {
        if self.sql_editor.mode() == EditorMode::Edit {
            match action {
                Action::ToggleEditor => {
                    self.sql_editor.toggle_mode();
                }
                Action::Quit => self.should_quit = true,
                _ => {}
            }
            return;
        }

        match action {
            Action::Quit => self.should_quit = true,
            Action::NextStep => self.step_forward(),
            Action::PrevStep => self.step_backward(),
            Action::FirstStep => {
                self.current_step = 0;
                self.scroll_offset = 0;
            }
            Action::LastStep => {
                self.current_step =
                    self.timeline.len().saturating_sub(1);
                self.scroll_offset = 0;
            }
            Action::TogglePlay => {
                self.playing = !self.playing;
            }
            Action::SpeedUp => {
                if self.speed_index < SPEEDS.len() - 1 {
                    self.speed_index += 1;
                }
            }
            Action::SlowDown => {
                if self.speed_index > 0 {
                    self.speed_index -= 1;
                }
            }
            Action::NextPanel => {
                self.focused =
                    self.focused.next(self.layout_mode);
                self.scroll_offset = 0;
            }
            Action::PrevPanel => {
                self.focused =
                    self.focused.prev(self.layout_mode);
                self.scroll_offset = 0;
            }
            Action::ScrollUp => {
                self.scroll_offset =
                    self.scroll_offset.saturating_sub(1);
            }
            Action::ScrollDown => {
                self.scroll_offset =
                    self.scroll_offset.saturating_add(1);
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Action::ToggleLayout => {
                self.layout_mode = self.layout_mode.toggle();
                if self.layout_mode == LayoutMode::Classic
                    && self.focused == Panel::SqlEditor
                {
                    self.focused = Panel::Plan;
                }
                if self.layout_mode == LayoutMode::Editor
                    && self.focused == Panel::Evolution
                {
                    self.focused = Panel::Stats;
                }
            }
            Action::ToggleEditor => {
                if self.layout_mode == LayoutMode::Editor {
                    self.focused = Panel::SqlEditor;
                    self.sql_editor.toggle_mode();
                }
            }
            Action::None => {}
        }
    }

    /// Advance one step forward.
    fn step_forward(&mut self) {
        let max = self.timeline.len().saturating_sub(1);
        if self.current_step < max {
            self.current_step += 1;
            self.scroll_offset = 0;
        } else {
            self.playing = false;
        }
    }

    /// Go one step backward.
    fn step_backward(&mut self) {
        if self.current_step > 0 {
            self.current_step -= 1;
            self.scroll_offset = 0;
        }
    }

    /// Handle auto-play tick.
    fn tick(&mut self) -> bool {
        if self.playing
            && self.last_advance.elapsed()
                >= self.tick_duration()
        {
            self.step_forward();
            self.last_advance = Instant::now();
            return true;
        }
        false
    }

    /// Handle a raw key event, routing based on keybinding
    /// mode and editor state.
    fn handle_key(
        &mut self,
        key: &crossterm::event::KeyEvent,
    ) {
        if self.sql_editor.mode() == EditorMode::Edit {
            match self.sql_editor.keybinding_mode() {
                KeybindingMode::Vi => {
                    self.handle_key_vi_edit(key);
                }
                KeybindingMode::Nano => {
                    self.handle_key_nano_edit(key);
                }
                KeybindingMode::Normal => {
                    self.handle_key_normal_edit(key);
                }
            }
        } else {
            let action = key_action(key);
            self.handle_action(action);
        }
    }

    /// Key handling for Normal keybindings in edit mode.
    fn handle_key_normal_edit(
        &mut self,
        key: &crossterm::event::KeyEvent,
    ) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.sql_editor
                    .set_mode(EditorMode::View);
            }
            (_, KeyCode::Up) => self.sql_editor.move_up(),
            (_, KeyCode::Down) => {
                self.sql_editor.move_down();
            }
            (_, KeyCode::Left) => {
                self.sql_editor.move_left();
            }
            (_, KeyCode::Right) => {
                self.sql_editor.move_right();
            }
            (_, KeyCode::Home) => {
                self.sql_editor.move_home();
            }
            (_, KeyCode::End) => {
                self.sql_editor.move_end();
            }
            (_, KeyCode::Backspace) => {
                self.sql_editor.backspace();
            }
            (_, KeyCode::Delete) => {
                self.sql_editor.delete_char();
            }
            (_, KeyCode::Enter) => {
                self.sql_editor.insert_char('\n');
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                self.should_quit = true;
            }
            (_, KeyCode::Char(ch)) => {
                self.sql_editor.insert_char(ch);
            }
            _ => {}
        }
    }

    /// Key handling for Vi keybindings in edit mode.
    fn handle_key_vi_edit(
        &mut self,
        key: &crossterm::event::KeyEvent,
    ) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.sql_editor.vi_mode() == ViSubMode::Insert {
            self.handle_key_vi_insert(key);
            return;
        }

        // Vi command mode
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                self.should_quit = true;
            }
            (_, KeyCode::Esc) => {
                self.sql_editor
                    .set_mode(EditorMode::View);
            }
            (_, KeyCode::Char('h') | KeyCode::Left) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.move_left();
            }
            (_, KeyCode::Char('j') | KeyCode::Down) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.move_down();
            }
            (_, KeyCode::Char('k') | KeyCode::Up) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.move_up();
            }
            (_, KeyCode::Char('l') | KeyCode::Right) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.move_right();
            }
            (_, KeyCode::Char('0') | KeyCode::Home) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.move_home();
            }
            (_, KeyCode::Char('$') | KeyCode::End) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.move_end();
            }
            (_, KeyCode::Char('i')) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.vi_enter_insert();
            }
            (_, KeyCode::Char('a')) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.vi_enter_append();
            }
            (_, KeyCode::Char('d')) => {
                self.sql_editor.vi_press_d();
            }
            (_, KeyCode::Char('p')) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.paste_line();
            }
            (_, KeyCode::Char('x')) => {
                self.sql_editor.vi_cancel_pending();
                self.sql_editor.delete_char();
            }
            _ => {
                self.sql_editor.vi_cancel_pending();
            }
        }
    }

    /// Key handling for Vi insert sub-mode.
    fn handle_key_vi_insert(
        &mut self,
        key: &crossterm::event::KeyEvent,
    ) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.sql_editor.vi_escape();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                self.should_quit = true;
            }
            (_, KeyCode::Up) => self.sql_editor.move_up(),
            (_, KeyCode::Down) => {
                self.sql_editor.move_down();
            }
            (_, KeyCode::Left) => {
                self.sql_editor.move_left();
            }
            (_, KeyCode::Right) => {
                self.sql_editor.move_right();
            }
            (_, KeyCode::Backspace) => {
                self.sql_editor.backspace();
            }
            (_, KeyCode::Delete) => {
                self.sql_editor.delete_char();
            }
            (_, KeyCode::Enter) => {
                self.sql_editor.insert_char('\n');
            }
            (_, KeyCode::Char(ch)) => {
                self.sql_editor.insert_char(ch);
            }
            _ => {}
        }
    }

    /// Key handling for Nano keybindings in edit mode.
    fn handle_key_nano_edit(
        &mut self,
        key: &crossterm::event::KeyEvent,
    ) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.sql_editor
                    .set_mode(EditorMode::View);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                self.should_quit = true;
            }
            // Nano Ctrl-K: cut line
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                self.sql_editor.delete_line();
            }
            // Nano Ctrl-U: uncut (paste) line
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.sql_editor.paste_line();
            }
            (_, KeyCode::Up) => self.sql_editor.move_up(),
            (_, KeyCode::Down) => {
                self.sql_editor.move_down();
            }
            (_, KeyCode::Left) => {
                self.sql_editor.move_left();
            }
            (_, KeyCode::Right) => {
                self.sql_editor.move_right();
            }
            (_, KeyCode::Home) => {
                self.sql_editor.move_home();
            }
            (_, KeyCode::End) => {
                self.sql_editor.move_end();
            }
            (_, KeyCode::Backspace) => {
                self.sql_editor.backspace();
            }
            (_, KeyCode::Delete) => {
                self.sql_editor.delete_char();
            }
            (_, KeyCode::Enter) => {
                self.sql_editor.insert_char('\n');
            }
            (_, KeyCode::Char(ch)) => {
                self.sql_editor.insert_char(ch);
            }
            _ => {}
        }
    }

    /// Run the interactive TUI event loop.
    ///
    /// # Errors
    ///
    /// Returns an error on terminal IO failure.
    pub fn run(&mut self) -> Result<(), AppError> {
        enable_raw_mode()?;
        let mut stdout = io::stderr();
        execute!(stdout, EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let events =
            EventHandler::new(Duration::from_millis(50));

        loop {
            terminal
                .draw(|frame| ui::render(frame, self))?;

            match events.next() {
                Ok(TuiEvent::Key(key)) => {
                    self.handle_key(&key);
                }
                Ok(TuiEvent::Tick) => {
                    self.tick();
                }
                Ok(TuiEvent::Resize(_, _)) => {}
                Err(_) => self.should_quit = true,
            }

            if self.should_quit {
                break;
            }
        }

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    /// Run in headless mode: advance through all steps and return
    /// the final snapshot's cost.
    ///
    /// # Errors
    ///
    /// Returns an error if the timeline is empty (already checked
    /// at construction).
    pub fn run_headless(&mut self) -> Result<f64, AppError> {
        while self.current_step
            < self.timeline.len().saturating_sub(1)
        {
            self.step_forward();
        }

        let final_cost = self
            .timeline
            .snapshots
            .last()
            .map_or(0.0, |s| s.cost);

        Ok(final_cost)
    }
}
