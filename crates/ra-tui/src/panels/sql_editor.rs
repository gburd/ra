//! SQL editor panel for the TUI.
//!
//! Provides a basic text editor for viewing and editing SQL queries
//! directly within the TUI. Supports two modes:
//! - **View**: read-only display of the current query
//! - **Edit**: basic text editing with cursor movement

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// Editor mode for the SQL panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    /// Read-only view of the query.
    View,
    /// Editable mode with cursor.
    Edit,
}

impl EditorMode {
    /// Toggle between View and Edit modes.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::View => Self::Edit,
            Self::Edit => Self::View,
        }
    }
}

/// State for the SQL editor panel.
#[derive(Debug, Clone)]
pub struct SqlEditor {
    /// Lines of text in the editor buffer.
    lines: Vec<String>,
    /// Cursor row (0-indexed).
    cursor_row: usize,
    /// Cursor column (0-indexed).
    cursor_col: usize,
    /// Current editor mode.
    mode: EditorMode,
    /// Vertical scroll offset.
    scroll_offset: usize,
}

impl SqlEditor {
    /// Create a new SQL editor with the given initial text.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let lines: Vec<String> = if text.is_empty() {
            vec![String::new()]
        } else {
            text.lines().map(String::from).collect()
        };
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            mode: EditorMode::View,
            scroll_offset: 0,
        }
    }

    /// Get the current editor mode.
    #[must_use]
    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    /// Toggle the editor mode between View and Edit.
    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.toggle();
    }

    /// Set the editor mode.
    pub fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
    }

    /// Get the full text content of the editor.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Set the editor content from a string.
    pub fn set_text(&mut self, text: &str) {
        self.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.lines().map(String::from).collect()
        };
        self.clamp_cursor();
    }

    /// Get the cursor position as (row, col).
    #[must_use]
    pub fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    /// Move cursor up one line.
    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_col();
            self.scroll_into_view();
        }
    }

    /// Move cursor down one line.
    pub fn move_down(&mut self) {
        if self.cursor_row < self.lines.len().saturating_sub(1) {
            self.cursor_row += 1;
            self.clamp_col();
            self.scroll_into_view();
        }
    }

    /// Move cursor left one character.
    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_len();
            self.scroll_into_view();
        }
    }

    /// Move cursor right one character.
    pub fn move_right(&mut self) {
        let line_len = self.current_line_len();
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_row
            < self.lines.len().saturating_sub(1)
        {
            self.cursor_row += 1;
            self.cursor_col = 0;
            self.scroll_into_view();
        }
    }

    /// Move cursor to the start of the current line.
    pub fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    /// Move cursor to the end of the current line.
    pub fn move_end(&mut self) {
        self.cursor_col = self.current_line_len();
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, ch: char) {
        if self.mode != EditorMode::Edit {
            return;
        }
        if ch == '\n' {
            self.insert_newline();
            return;
        }
        let row = self.cursor_row;
        let col = self.cursor_col.min(self.lines[row].len());
        self.lines[row].insert(col, ch);
        self.cursor_col = col + 1;
    }

    /// Insert a newline, splitting the current line.
    fn insert_newline(&mut self) {
        let row = self.cursor_row;
        let col = self.cursor_col.min(self.lines[row].len());
        let rest = self.lines[row][col..].to_owned();
        self.lines[row].truncate(col);
        self.lines.insert(row + 1, rest);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.scroll_into_view();
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.mode != EditorMode::Edit {
            return;
        }
        if self.cursor_col > 0 {
            let row = self.cursor_row;
            let col = self.cursor_col.min(self.lines[row].len());
            self.lines[row].remove(col - 1);
            self.cursor_col = col - 1;
        } else if self.cursor_row > 0 {
            let row = self.cursor_row;
            let prev_len = self.lines[row - 1].len();
            let removed = self.lines.remove(row);
            self.lines[row - 1].push_str(&removed);
            self.cursor_row -= 1;
            self.cursor_col = prev_len;
            self.scroll_into_view();
        }
    }

    /// Delete the character at the cursor position (delete key).
    pub fn delete_char(&mut self) {
        if self.mode != EditorMode::Edit {
            return;
        }
        let row = self.cursor_row;
        let line_len = self.lines[row].len();
        if self.cursor_col < line_len {
            self.lines[row].remove(self.cursor_col);
        } else if row < self.lines.len().saturating_sub(1) {
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
        }
    }

    /// Number of lines in the editor buffer.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Length of the current line.
    fn current_line_len(&self) -> usize {
        self.lines[self.cursor_row].len()
    }

    /// Clamp column to the current line length.
    fn clamp_col(&mut self) {
        let len = self.current_line_len();
        if self.cursor_col > len {
            self.cursor_col = len;
        }
    }

    /// Clamp cursor to valid bounds after text changes.
    fn clamp_cursor(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len() - 1;
        }
        self.clamp_col();
    }

    /// Scroll so the cursor is visible.
    fn scroll_into_view(&mut self) {
        if self.cursor_row < self.scroll_offset {
            self.scroll_offset = self.cursor_row;
        }
    }

    /// Adjust scroll offset for rendering within a given height.
    fn visible_scroll(&self, height: usize) -> usize {
        let mut offset = self.scroll_offset;
        if self.cursor_row >= offset + height {
            offset = self.cursor_row.saturating_sub(height - 1);
        }
        if self.cursor_row < offset {
            offset = self.cursor_row;
        }
        offset
    }
}

/// Render the SQL editor panel.
pub fn render(
    frame: &mut Frame,
    editor: &SqlEditor,
    area: Rect,
    focused: bool,
) {
    let border_color = if focused {
        match editor.mode {
            EditorMode::Edit => Color::Green,
            EditorMode::View => Color::Cyan,
        }
    } else {
        Color::DarkGray
    };

    let mode_label = match editor.mode {
        EditorMode::View => "SQL [View]",
        EditorMode::Edit => "SQL [Edit]",
    };

    let block = Block::default()
        .title(format!(" {mode_label} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let visible_height = inner.height as usize;
    let scroll = editor.visible_scroll(visible_height);

    let mut text_lines: Vec<Line<'_>> = Vec::new();
    let end = (scroll + visible_height).min(editor.lines.len());

    for (idx, line) in
        editor.lines[scroll..end].iter().enumerate()
    {
        let abs_row = scroll + idx;
        let line_num = format!("{:>3} ", abs_row + 1);

        if editor.mode == EditorMode::Edit
            && abs_row == editor.cursor_row
        {
            let col =
                editor.cursor_col.min(line.len());
            let before = &line[..col];
            let cursor_ch = if col < line.len() {
                &line[col..=col]
            } else {
                " "
            };
            let after = if col < line.len() {
                &line[col + 1..]
            } else {
                ""
            };

            text_lines.push(Line::from(vec![
                Span::styled(
                    line_num,
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(before.to_owned()),
                Span::styled(
                    cursor_ch.to_owned(),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(after.to_owned()),
            ]));
        } else {
            text_lines.push(Line::from(vec![
                Span::styled(
                    line_num,
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    line.clone(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    }

    let paragraph = Paragraph::new(text_lines)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_editor_has_text() {
        let editor = SqlEditor::new("SELECT 1");
        assert_eq!(editor.text(), "SELECT 1");
    }

    #[test]
    fn new_editor_empty_string() {
        let editor = SqlEditor::new("");
        assert_eq!(editor.line_count(), 1);
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn new_editor_multiline() {
        let editor = SqlEditor::new("SELECT\n  *\nFROM t");
        assert_eq!(editor.line_count(), 3);
    }

    #[test]
    fn default_mode_is_view() {
        let editor = SqlEditor::new("SELECT 1");
        assert_eq!(editor.mode(), EditorMode::View);
    }

    #[test]
    fn toggle_mode() {
        let mut editor = SqlEditor::new("SELECT 1");
        editor.toggle_mode();
        assert_eq!(editor.mode(), EditorMode::Edit);
        editor.toggle_mode();
        assert_eq!(editor.mode(), EditorMode::View);
    }

    #[test]
    fn insert_char_in_edit_mode() {
        let mut editor = SqlEditor::new("AB");
        editor.set_mode(EditorMode::Edit);
        editor.cursor_col = 1;
        editor.insert_char('X');
        assert_eq!(editor.text(), "AXB");
        assert_eq!(editor.cursor_col, 2);
    }

    #[test]
    fn insert_char_in_view_mode_noop() {
        let mut editor = SqlEditor::new("AB");
        editor.insert_char('X');
        assert_eq!(editor.text(), "AB");
    }

    #[test]
    fn backspace_middle() {
        let mut editor = SqlEditor::new("ABC");
        editor.set_mode(EditorMode::Edit);
        editor.cursor_col = 2;
        editor.backspace();
        assert_eq!(editor.text(), "AC");
        assert_eq!(editor.cursor_col, 1);
    }

    #[test]
    fn backspace_at_line_start_merges() {
        let mut editor = SqlEditor::new("AB\nCD");
        editor.set_mode(EditorMode::Edit);
        editor.cursor_row = 1;
        editor.cursor_col = 0;
        editor.backspace();
        assert_eq!(editor.text(), "ABCD");
        assert_eq!(editor.cursor_row, 0);
        assert_eq!(editor.cursor_col, 2);
    }

    #[test]
    fn backspace_at_start_noop() {
        let mut editor = SqlEditor::new("AB");
        editor.set_mode(EditorMode::Edit);
        editor.cursor_col = 0;
        editor.backspace();
        assert_eq!(editor.text(), "AB");
    }

    #[test]
    fn delete_char_middle() {
        let mut editor = SqlEditor::new("ABC");
        editor.set_mode(EditorMode::Edit);
        editor.cursor_col = 1;
        editor.delete_char();
        assert_eq!(editor.text(), "AC");
    }

    #[test]
    fn delete_at_end_merges_next_line() {
        let mut editor = SqlEditor::new("AB\nCD");
        editor.set_mode(EditorMode::Edit);
        editor.cursor_col = 2;
        editor.delete_char();
        assert_eq!(editor.text(), "ABCD");
    }

    #[test]
    fn insert_newline() {
        let mut editor = SqlEditor::new("ABCD");
        editor.set_mode(EditorMode::Edit);
        editor.cursor_col = 2;
        editor.insert_char('\n');
        assert_eq!(editor.line_count(), 2);
        assert_eq!(editor.text(), "AB\nCD");
        assert_eq!(editor.cursor_row, 1);
        assert_eq!(editor.cursor_col, 0);
    }

    #[test]
    fn move_up_down() {
        let mut editor = SqlEditor::new("A\nB\nC");
        editor.move_down();
        assert_eq!(editor.cursor_row, 1);
        editor.move_down();
        assert_eq!(editor.cursor_row, 2);
        editor.move_down(); // at bottom, stays
        assert_eq!(editor.cursor_row, 2);
        editor.move_up();
        assert_eq!(editor.cursor_row, 1);
        editor.move_up();
        assert_eq!(editor.cursor_row, 0);
        editor.move_up(); // at top, stays
        assert_eq!(editor.cursor_row, 0);
    }

    #[test]
    fn move_left_wraps_to_prev_line() {
        let mut editor = SqlEditor::new("AB\nCD");
        editor.cursor_row = 1;
        editor.cursor_col = 0;
        editor.move_left();
        assert_eq!(editor.cursor_row, 0);
        assert_eq!(editor.cursor_col, 2);
    }

    #[test]
    fn move_right_wraps_to_next_line() {
        let mut editor = SqlEditor::new("AB\nCD");
        editor.cursor_col = 2;
        editor.move_right();
        assert_eq!(editor.cursor_row, 1);
        assert_eq!(editor.cursor_col, 0);
    }

    #[test]
    fn move_home_end() {
        let mut editor = SqlEditor::new("ABCDEF");
        editor.cursor_col = 3;
        editor.move_home();
        assert_eq!(editor.cursor_col, 0);
        editor.move_end();
        assert_eq!(editor.cursor_col, 6);
    }

    #[test]
    fn set_text_resets_cursor() {
        let mut editor = SqlEditor::new("ABCDEF");
        editor.cursor_row = 0;
        editor.cursor_col = 5;
        editor.set_text("AB");
        assert_eq!(editor.cursor_col, 2);
    }

    #[test]
    fn cursor_position() {
        let mut editor = SqlEditor::new("A\nB");
        editor.cursor_row = 1;
        editor.cursor_col = 1;
        assert_eq!(editor.cursor_position(), (1, 1));
    }

    #[test]
    fn editor_mode_toggle_values() {
        assert_eq!(
            EditorMode::View.toggle(),
            EditorMode::Edit
        );
        assert_eq!(
            EditorMode::Edit.toggle(),
            EditorMode::View
        );
    }

    #[test]
    fn visible_scroll_adjusts_for_cursor() {
        let mut editor = SqlEditor::new(
            &(0..50)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        editor.cursor_row = 30;
        let scroll = editor.visible_scroll(10);
        assert!(scroll <= 30);
        assert!(scroll + 10 > 30);
    }

    #[test]
    fn backspace_view_mode_noop() {
        let mut editor = SqlEditor::new("ABC");
        editor.cursor_col = 2;
        editor.backspace();
        assert_eq!(editor.text(), "ABC");
    }

    #[test]
    fn delete_view_mode_noop() {
        let mut editor = SqlEditor::new("ABC");
        editor.cursor_col = 1;
        editor.delete_char();
        assert_eq!(editor.text(), "ABC");
    }
}
