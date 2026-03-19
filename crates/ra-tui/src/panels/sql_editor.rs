//! SQL editor panel with configurable keybinding modes.
//!
//! Renders a text editor area with mode indicator in the border.
//! Supports Normal, Vi, and Nano keybinding modes loaded from
//! the RA configuration.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use ra_config::EditorMode;

/// State for the SQL editor panel.
#[derive(Debug)]
pub struct SqlEditor {
    /// Current editor content (lines).
    lines: Vec<String>,
    /// Cursor row position.
    cursor_row: usize,
    /// Cursor column position.
    cursor_col: usize,
    /// Active editor mode.
    mode: EditorMode,
    /// Vi sub-mode (only relevant when mode == Vi).
    vi_state: ViState,
    /// Vertical scroll offset.
    scroll: u16,
}

/// Vi-specific modal states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViState {
    /// Normal/command mode.
    Normal,
    /// Insert mode.
    Insert,
    /// Visual/selection mode.
    Visual,
}

impl SqlEditor {
    /// Create a new SQL editor with the given editor mode.
    #[must_use]
    pub fn new(mode: EditorMode) -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            mode,
            vi_state: ViState::Normal,
            scroll: 0,
        }
    }

    /// Create an editor pre-loaded with SQL text.
    #[must_use]
    pub fn with_content(
        mode: EditorMode,
        content: &str,
    ) -> Self {
        let lines: Vec<String> =
            content.lines().map(String::from).collect();
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            mode,
            vi_state: ViState::Normal,
            scroll: 0,
        }
    }

    /// Get the current editor content as a single string.
    #[must_use]
    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    /// Get the current editor mode.
    #[must_use]
    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    /// Set the editor mode.
    pub fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
        self.vi_state = ViState::Normal;
    }

    /// Get the Vi sub-state (only meaningful when mode is Vi).
    #[must_use]
    pub fn vi_state(&self) -> ViState {
        self.vi_state
    }

    /// Get the mode indicator text for display in borders.
    #[must_use]
    pub fn mode_indicator(&self) -> &'static str {
        match self.mode {
            EditorMode::Normal => "NORMAL",
            EditorMode::Vi => match self.vi_state {
                ViState::Normal => "VI:CMD",
                ViState::Insert => "VI:INS",
                ViState::Visual => "VI:VIS",
            },
            EditorMode::Nano => "NANO",
        }
    }

    /// Handle a character input in the current mode.
    pub fn handle_char(&mut self, ch: char) {
        match self.mode {
            EditorMode::Normal | EditorMode::Nano => {
                self.insert_char(ch);
            }
            EditorMode::Vi => match self.vi_state {
                ViState::Insert => self.insert_char(ch),
                ViState::Normal | ViState::Visual => {
                    self.handle_vi_normal(ch);
                }
            },
        }
    }

    /// Handle Enter key.
    pub fn handle_enter(&mut self) {
        let should_insert = match self.mode {
            EditorMode::Normal | EditorMode::Nano => true,
            EditorMode::Vi => {
                self.vi_state == ViState::Insert
            }
        };

        if should_insert {
            let rest = self.lines[self.cursor_row]
                [self.cursor_col..]
                .to_owned();
            self.lines[self.cursor_row]
                .truncate(self.cursor_col);
            self.cursor_row += 1;
            self.lines.insert(self.cursor_row, rest);
            self.cursor_col = 0;
        }
    }

    /// Handle Backspace key.
    pub fn handle_backspace(&mut self) {
        let should_delete = match self.mode {
            EditorMode::Normal | EditorMode::Nano => true,
            EditorMode::Vi => {
                self.vi_state == ViState::Insert
            }
        };

        if !should_delete {
            return;
        }

        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            self.lines[self.cursor_row]
                .remove(self.cursor_col);
        } else if self.cursor_row > 0 {
            let removed =
                self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col =
                self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&removed);
        }
    }

    /// Handle Escape key.
    pub fn handle_escape(&mut self) {
        if self.mode == EditorMode::Vi {
            self.vi_state = ViState::Normal;
            if self.cursor_col > 0 {
                self.cursor_col -= 1;
            }
        }
    }

    /// Move cursor up.
    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_cursor_col();
        }
    }

    /// Move cursor down.
    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.clamp_cursor_col();
        }
    }

    /// Move cursor left.
    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    /// Move cursor right.
    pub fn move_right(&mut self) {
        let line_len = self.lines[self.cursor_row].len();
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        }
    }

    /// Render the editor into a frame region.
    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focused: bool,
    ) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let mode_style = match self.mode {
            EditorMode::Vi => match self.vi_state {
                ViState::Normal => {
                    Style::default().fg(Color::Green)
                }
                ViState::Insert => {
                    Style::default().fg(Color::Yellow)
                }
                ViState::Visual => {
                    Style::default().fg(Color::Magenta)
                }
            },
            EditorMode::Nano => {
                Style::default().fg(Color::Cyan)
            }
            EditorMode::Normal => {
                Style::default().fg(Color::White)
            }
        };

        let title = Line::from(vec![
            Span::styled(" SQL Editor ", border_style),
            Span::styled(
                format!("[{}]", self.mode_indicator()),
                mode_style.add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let visible_height = inner.height as usize;
        let start = self.scroll as usize;
        let end =
            (start + visible_height).min(self.lines.len());

        let text_lines: Vec<Line> = self.lines
            [start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let row = start + i;
                let line_num = format!("{:>3} ", row + 1);
                let mut spans = vec![Span::styled(
                    line_num,
                    Style::default()
                        .fg(Color::DarkGray),
                )];

                if row == self.cursor_row && focused {
                    // Highlight cursor line
                    spans.push(Span::styled(
                        line.clone(),
                        Style::default()
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    spans.push(Span::raw(line.clone()));
                }

                Line::from(spans)
            })
            .collect();

        let paragraph = Paragraph::new(text_lines)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, inner);
    }

    fn insert_char(&mut self, ch: char) {
        self.lines[self.cursor_row]
            .insert(self.cursor_col, ch);
        self.cursor_col += 1;
    }

    fn handle_vi_normal(&mut self, ch: char) {
        match ch {
            'i' => self.vi_state = ViState::Insert,
            'a' => {
                self.vi_state = ViState::Insert;
                let line_len =
                    self.lines[self.cursor_row].len();
                if self.cursor_col < line_len {
                    self.cursor_col += 1;
                }
            }
            'o' => {
                self.vi_state = ViState::Insert;
                self.cursor_row += 1;
                self.lines.insert(
                    self.cursor_row,
                    String::new(),
                );
                self.cursor_col = 0;
            }
            'h' => self.move_left(),
            'j' => self.move_down(),
            'k' => self.move_up(),
            'l' => self.move_right(),
            '0' => self.cursor_col = 0,
            '$' => {
                self.cursor_col = self.lines
                    [self.cursor_row]
                    .len()
                    .saturating_sub(1);
            }
            'v' => self.vi_state = ViState::Visual,
            'x' => {
                let line_len =
                    self.lines[self.cursor_row].len();
                if self.cursor_col < line_len {
                    self.lines[self.cursor_row]
                        .remove(self.cursor_col);
                    if self.cursor_col >= line_len.saturating_sub(1)
                        && self.cursor_col > 0
                    {
                        self.cursor_col -= 1;
                    }
                }
            }
            'G' => {
                self.cursor_row =
                    self.lines.len().saturating_sub(1);
                self.clamp_cursor_col();
            }
            _ => {}
        }
    }

    fn clamp_cursor_col(&mut self) {
        let max_col = self.lines[self.cursor_row].len();
        if self.cursor_col > max_col {
            self.cursor_col = max_col;
        }
    }
}

/// Load the editor mode from the RA config system.
///
/// Falls back to `Normal` if the config cannot be loaded.
#[must_use]
pub fn load_editor_mode() -> EditorMode {
    ra_config::ConfigLoader::new()
        .load()
        .map_or(EditorMode::Normal, |c| c.editor.mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_editor_has_empty_line() {
        let editor = SqlEditor::new(EditorMode::Normal);
        assert_eq!(editor.content(), "");
        assert_eq!(editor.lines.len(), 1);
    }

    #[test]
    fn with_content_loads_lines() {
        let editor = SqlEditor::with_content(
            EditorMode::Normal,
            "SELECT *\nFROM t",
        );
        assert_eq!(editor.lines.len(), 2);
        assert_eq!(editor.content(), "SELECT *\nFROM t");
    }

    #[test]
    fn insert_char_normal_mode() {
        let mut editor = SqlEditor::new(EditorMode::Normal);
        editor.handle_char('S');
        editor.handle_char('Q');
        editor.handle_char('L');
        assert_eq!(editor.content(), "SQL");
        assert_eq!(editor.cursor_col, 3);
    }

    #[test]
    fn vi_starts_in_normal_mode() {
        let editor = SqlEditor::new(EditorMode::Vi);
        assert_eq!(editor.vi_state(), ViState::Normal);
        assert_eq!(editor.mode_indicator(), "VI:CMD");
    }

    #[test]
    fn vi_insert_mode_toggle() {
        let mut editor = SqlEditor::new(EditorMode::Vi);
        // 'i' in normal mode enters insert
        editor.handle_char('i');
        assert_eq!(editor.vi_state(), ViState::Insert);
        assert_eq!(editor.mode_indicator(), "VI:INS");

        // typing inserts characters
        editor.handle_char('X');
        assert_eq!(editor.content(), "X");

        // Escape returns to normal
        editor.handle_escape();
        assert_eq!(editor.vi_state(), ViState::Normal);
    }

    #[test]
    fn vi_hjkl_navigation() {
        let mut editor = SqlEditor::with_content(
            EditorMode::Vi,
            "abc\ndef\nghi",
        );
        // j = move down
        editor.handle_char('j');
        assert_eq!(editor.cursor_row, 1);
        // l = move right
        editor.handle_char('l');
        assert_eq!(editor.cursor_col, 1);
        // k = move up
        editor.handle_char('k');
        assert_eq!(editor.cursor_row, 0);
        // h = move left
        editor.handle_char('h');
        assert_eq!(editor.cursor_col, 0);
    }

    #[test]
    fn vi_does_not_insert_in_normal_mode() {
        let mut editor = SqlEditor::new(EditorMode::Vi);
        // Characters in normal mode are commands, not inserts
        editor.handle_char('z');
        assert_eq!(editor.content(), "");
    }

    #[test]
    fn handle_enter_splits_line() {
        let mut editor = SqlEditor::new(EditorMode::Normal);
        editor.handle_char('A');
        editor.handle_char('B');
        editor.cursor_col = 1;
        editor.handle_enter();
        assert_eq!(editor.lines.len(), 2);
        assert_eq!(editor.lines[0], "A");
        assert_eq!(editor.lines[1], "B");
    }

    #[test]
    fn handle_backspace_deletes_char() {
        let mut editor = SqlEditor::new(EditorMode::Normal);
        editor.handle_char('A');
        editor.handle_char('B');
        editor.handle_backspace();
        assert_eq!(editor.content(), "A");
    }

    #[test]
    fn handle_backspace_merges_lines() {
        let mut editor = SqlEditor::with_content(
            EditorMode::Normal,
            "AB\nCD",
        );
        editor.cursor_row = 1;
        editor.cursor_col = 0;
        editor.handle_backspace();
        assert_eq!(editor.lines.len(), 1);
        assert_eq!(editor.content(), "ABCD");
    }

    #[test]
    fn nano_mode_indicator() {
        let editor = SqlEditor::new(EditorMode::Nano);
        assert_eq!(editor.mode_indicator(), "NANO");
    }

    #[test]
    fn set_mode_resets_vi_state() {
        let mut editor = SqlEditor::new(EditorMode::Vi);
        editor.vi_state = ViState::Insert;
        editor.set_mode(EditorMode::Normal);
        assert_eq!(editor.mode(), EditorMode::Normal);
        assert_eq!(editor.vi_state(), ViState::Normal);
    }

    #[test]
    fn vi_x_deletes_char_at_cursor() {
        let mut editor = SqlEditor::with_content(
            EditorMode::Vi,
            "abc",
        );
        editor.handle_char('x');
        assert_eq!(editor.content(), "bc");
    }

    #[test]
    fn vi_zero_goes_to_line_start() {
        let mut editor = SqlEditor::with_content(
            EditorMode::Vi,
            "abc",
        );
        editor.cursor_col = 2;
        editor.handle_char('0');
        assert_eq!(editor.cursor_col, 0);
    }

    #[test]
    fn vi_g_goes_to_last_line() {
        let mut editor = SqlEditor::with_content(
            EditorMode::Vi,
            "a\nb\nc",
        );
        editor.handle_char('G');
        assert_eq!(editor.cursor_row, 2);
    }

    #[test]
    fn vi_o_opens_new_line_below() {
        let mut editor = SqlEditor::with_content(
            EditorMode::Vi,
            "abc",
        );
        editor.handle_char('o');
        assert_eq!(editor.vi_state(), ViState::Insert);
        assert_eq!(editor.cursor_row, 1);
        assert_eq!(editor.lines.len(), 2);
    }

    #[test]
    fn cursor_col_clamps_on_vertical_move() {
        let mut editor = SqlEditor::with_content(
            EditorMode::Normal,
            "long line here\na",
        );
        editor.cursor_col = 10;
        editor.move_down();
        assert_eq!(editor.cursor_col, 1);
    }
}
