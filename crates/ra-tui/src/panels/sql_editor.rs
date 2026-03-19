//! SQL editor panel for the TUI.
//!
//! Provides a text editor for viewing and editing SQL queries
//! directly within the TUI. Supports keybinding modes loaded from
//! `ra_config::EditorMode`:
//! - **Normal**: standard arrow-key navigation and typing
//! - **Vi**: modal editing with h/j/k/l, i/a, dd, 0/$, Esc
//! - **Nano**: Ctrl-K cut line, Ctrl-U paste line

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// Editor mode for the SQL panel (view vs edit state).
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

/// Keybinding mode loaded from config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingMode {
    /// Standard arrow-key navigation.
    Normal,
    /// Vi-style modal keybindings.
    Vi,
    /// Nano-style keybindings with Ctrl shortcuts.
    Nano,
}

impl From<ra_config::EditorMode> for KeybindingMode {
    fn from(mode: ra_config::EditorMode) -> Self {
        match mode {
            ra_config::EditorMode::Normal => Self::Normal,
            ra_config::EditorMode::Vi => Self::Vi,
            ra_config::EditorMode::Nano => Self::Nano,
        }
    }
}

/// Vi sub-mode: whether typing inserts text or is interpreted
/// as commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViSubMode {
    /// Command mode: h/j/k/l move, dd deletes, etc.
    Command,
    /// Insert mode: characters are typed into the buffer.
    Insert,
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
    /// Current editor mode (view vs edit).
    mode: EditorMode,
    /// Keybinding mode from config.
    keybinding_mode: KeybindingMode,
    /// Vi sub-mode (only relevant when Vi keybindings).
    vi_mode: ViSubMode,
    /// Clipboard for nano Ctrl-K / Ctrl-U (cut/uncut line).
    clipboard: Option<String>,
    /// Vertical scroll offset.
    scroll_offset: usize,
    /// Whether the last key was 'd' (for Vi dd command).
    pending_d: bool,
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
            keybinding_mode: KeybindingMode::Normal,
            vi_mode: ViSubMode::Command,
            clipboard: None,
            scroll_offset: 0,
            pending_d: false,
        }
    }

    /// Create a new SQL editor with a specific keybinding mode.
    #[must_use]
    pub fn with_keybinding_mode(
        text: &str,
        mode: KeybindingMode,
    ) -> Self {
        let mut editor = Self::new(text);
        editor.keybinding_mode = mode;
        editor
    }

    /// Get the current editor mode.
    #[must_use]
    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    /// Get the keybinding mode.
    #[must_use]
    pub fn keybinding_mode(&self) -> KeybindingMode {
        self.keybinding_mode
    }

    /// Set the keybinding mode.
    pub fn set_keybinding_mode(&mut self, mode: KeybindingMode) {
        self.keybinding_mode = mode;
        self.vi_mode = ViSubMode::Command;
        self.pending_d = false;
    }

    /// Get the Vi sub-mode (only meaningful when Vi keybindings).
    #[must_use]
    pub fn vi_mode(&self) -> ViSubMode {
        self.vi_mode
    }

    /// Toggle the editor mode between View and Edit.
    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.toggle();
        if self.mode == EditorMode::View {
            self.vi_mode = ViSubMode::Command;
            self.pending_d = false;
        }
    }

    /// Set the editor mode.
    pub fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
        if mode == EditorMode::View {
            self.vi_mode = ViSubMode::Command;
            self.pending_d = false;
        }
    }

    /// Whether the editor accepts text insertion right now.
    #[must_use]
    pub fn is_inserting(&self) -> bool {
        if self.mode != EditorMode::Edit {
            return false;
        }
        match self.keybinding_mode {
            KeybindingMode::Normal | KeybindingMode::Nano => true,
            KeybindingMode::Vi => {
                self.vi_mode == ViSubMode::Insert
            }
        }
    }

    /// Enter Vi insert mode (from command mode).
    pub fn vi_enter_insert(&mut self) {
        if self.mode == EditorMode::Edit
            && self.keybinding_mode == KeybindingMode::Vi
        {
            self.vi_mode = ViSubMode::Insert;
            self.pending_d = false;
        }
    }

    /// Enter Vi insert mode after current character ('a').
    pub fn vi_enter_append(&mut self) {
        if self.mode == EditorMode::Edit
            && self.keybinding_mode == KeybindingMode::Vi
        {
            self.vi_mode = ViSubMode::Insert;
            self.pending_d = false;
            let line_len = self.current_line_len();
            if self.cursor_col < line_len {
                self.cursor_col += 1;
            }
        }
    }

    /// Return to Vi command mode from insert mode.
    pub fn vi_escape(&mut self) {
        if self.keybinding_mode == KeybindingMode::Vi {
            self.vi_mode = ViSubMode::Command;
            self.pending_d = false;
        }
    }

    /// Handle Vi 'd' key press (first of 'dd' sequence).
    /// Returns true if 'dd' was completed and a line was deleted.
    pub fn vi_press_d(&mut self) -> bool {
        if self.mode != EditorMode::Edit
            || self.keybinding_mode != KeybindingMode::Vi
            || self.vi_mode != ViSubMode::Command
        {
            return false;
        }
        if self.pending_d {
            self.pending_d = false;
            self.delete_line();
            return true;
        }
        self.pending_d = true;
        false
    }

    /// Cancel any pending Vi command.
    pub fn vi_cancel_pending(&mut self) {
        self.pending_d = false;
    }

    /// Delete the current line (for Vi dd and nano Ctrl-K).
    pub fn delete_line(&mut self) {
        if self.mode != EditorMode::Edit {
            return;
        }
        let row = self.cursor_row;
        let removed = self.lines[row].clone();
        self.clipboard = Some(removed);
        if self.lines.len() > 1 {
            self.lines.remove(row);
            if self.cursor_row >= self.lines.len() {
                self.cursor_row = self.lines.len() - 1;
            }
        } else {
            self.lines[0].clear();
        }
        self.clamp_col();
        self.scroll_into_view();
    }

    /// Paste (uncut) the clipboard contents as a new line below
    /// the cursor. Used by nano Ctrl-U and Vi 'p'.
    pub fn paste_line(&mut self) {
        if self.mode != EditorMode::Edit {
            return;
        }
        if let Some(ref text) = self.clipboard.clone() {
            let insert_at = self.cursor_row + 1;
            self.lines.insert(insert_at, text.clone());
            self.cursor_row = insert_at;
            self.cursor_col = 0;
            self.scroll_into_view();
        }
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
        if self.cursor_row
            < self.lines.len().saturating_sub(1)
        {
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
            let col =
                self.cursor_col.min(self.lines[row].len());
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

    /// Build the mode label for the panel border title.
    #[must_use]
    pub fn mode_label(&self) -> String {
        match self.mode {
            EditorMode::View => match self.keybinding_mode {
                KeybindingMode::Normal => {
                    "SQL [View]".to_owned()
                }
                KeybindingMode::Vi => {
                    "SQL [Vi:View]".to_owned()
                }
                KeybindingMode::Nano => {
                    "SQL [Nano:View]".to_owned()
                }
            },
            EditorMode::Edit => match self.keybinding_mode {
                KeybindingMode::Normal => {
                    "SQL [Edit]".to_owned()
                }
                KeybindingMode::Vi => {
                    match self.vi_mode {
                        ViSubMode::Command => {
                            "SQL [Vi:Cmd]".to_owned()
                        }
                        ViSubMode::Insert => {
                            "SQL [Vi:Ins]".to_owned()
                        }
                    }
                }
                KeybindingMode::Nano => {
                    "SQL [Nano:Edit]".to_owned()
                }
            },
        }
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
            offset =
                self.cursor_row.saturating_sub(height - 1);
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

    let mode_label = editor.mode_label();

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
    let end =
        (scroll + visible_height).min(editor.lines.len());

    let show_cursor = editor.mode == EditorMode::Edit;

    for (idx, line) in
        editor.lines[scroll..end].iter().enumerate()
    {
        let abs_row = scroll + idx;
        let line_num = format!("{:>3} ", abs_row + 1);

        if show_cursor && abs_row == editor.cursor_row {
            render_cursor_line(
                &mut text_lines,
                &line_num,
                line,
                editor.cursor_col,
            );
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

fn render_cursor_line(
    text_lines: &mut Vec<Line<'_>>,
    line_num: &str,
    line: &str,
    cursor_col: usize,
) {
    let col = cursor_col.min(line.len());
    let before = &line[..col];
    let cursor_ch = if col < line.len() {
        &line[col..=col]
    } else {
        " "
    };
    let after =
        if col < line.len() { &line[col + 1..] } else { "" };

    text_lines.push(Line::from(vec![
        Span::styled(
            line_num.to_owned(),
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
        let editor =
            SqlEditor::new("SELECT\n  *\nFROM t");
        assert_eq!(editor.line_count(), 3);
    }

    #[test]
    fn default_mode_is_view() {
        let editor = SqlEditor::new("SELECT 1");
        assert_eq!(editor.mode(), EditorMode::View);
    }

    #[test]
    fn default_keybinding_is_normal() {
        let editor = SqlEditor::new("SELECT 1");
        assert_eq!(
            editor.keybinding_mode(),
            KeybindingMode::Normal
        );
    }

    #[test]
    fn with_keybinding_mode_vi() {
        let editor = SqlEditor::with_keybinding_mode(
            "SELECT 1",
            KeybindingMode::Vi,
        );
        assert_eq!(
            editor.keybinding_mode(),
            KeybindingMode::Vi
        );
        assert_eq!(editor.vi_mode(), ViSubMode::Command);
    }

    #[test]
    fn keybinding_mode_from_config() {
        let mode: KeybindingMode =
            ra_config::EditorMode::Vi.into();
        assert_eq!(mode, KeybindingMode::Vi);

        let mode: KeybindingMode =
            ra_config::EditorMode::Nano.into();
        assert_eq!(mode, KeybindingMode::Nano);

        let mode: KeybindingMode =
            ra_config::EditorMode::Normal.into();
        assert_eq!(mode, KeybindingMode::Normal);
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
        editor.move_down();
        assert_eq!(editor.cursor_row, 2);
        editor.move_up();
        assert_eq!(editor.cursor_row, 1);
        editor.move_up();
        assert_eq!(editor.cursor_row, 0);
        editor.move_up();
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

    // -- Vi mode tests --

    #[test]
    fn vi_enter_insert_mode() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "SELECT 1",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);
        assert_eq!(editor.vi_mode(), ViSubMode::Command);
        assert!(!editor.is_inserting());

        editor.vi_enter_insert();
        assert_eq!(editor.vi_mode(), ViSubMode::Insert);
        assert!(editor.is_inserting());
    }

    #[test]
    fn vi_append_advances_cursor() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "ABCD",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);
        editor.cursor_col = 2;
        editor.vi_enter_append();
        assert_eq!(editor.vi_mode(), ViSubMode::Insert);
        assert_eq!(editor.cursor_col, 3);
    }

    #[test]
    fn vi_append_at_end_of_line() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "AB",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);
        editor.cursor_col = 2;
        editor.vi_enter_append();
        assert_eq!(editor.cursor_col, 2);
    }

    #[test]
    fn vi_escape_returns_to_command() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "AB",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);
        editor.vi_enter_insert();
        assert_eq!(editor.vi_mode(), ViSubMode::Insert);
        editor.vi_escape();
        assert_eq!(editor.vi_mode(), ViSubMode::Command);
    }

    #[test]
    fn vi_dd_deletes_line() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "line1\nline2\nline3",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);
        editor.cursor_row = 1;

        let completed = editor.vi_press_d();
        assert!(!completed);
        let completed = editor.vi_press_d();
        assert!(completed);
        assert_eq!(editor.text(), "line1\nline3");
        assert_eq!(editor.clipboard, Some("line2".to_owned()));
    }

    #[test]
    fn vi_d_cancel_on_other_key() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "line1\nline2",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);

        let completed = editor.vi_press_d();
        assert!(!completed);
        editor.vi_cancel_pending();
        assert_eq!(editor.text(), "line1\nline2");
    }

    #[test]
    fn vi_dd_on_single_line_clears() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "only line",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);

        editor.vi_press_d();
        editor.vi_press_d();
        assert_eq!(editor.text(), "");
        assert_eq!(editor.line_count(), 1);
    }

    #[test]
    fn vi_mode_label() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "x",
            KeybindingMode::Vi,
        );
        assert_eq!(editor.mode_label(), "SQL [Vi:View]");

        editor.set_mode(EditorMode::Edit);
        assert_eq!(editor.mode_label(), "SQL [Vi:Cmd]");

        editor.vi_enter_insert();
        assert_eq!(editor.mode_label(), "SQL [Vi:Ins]");
    }

    #[test]
    fn vi_set_view_resets_submode() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "x",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);
        editor.vi_enter_insert();
        assert_eq!(editor.vi_mode(), ViSubMode::Insert);

        editor.set_mode(EditorMode::View);
        assert_eq!(editor.vi_mode(), ViSubMode::Command);
    }

    // -- Nano mode tests --

    #[test]
    fn nano_cut_line() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "line1\nline2\nline3",
            KeybindingMode::Nano,
        );
        editor.set_mode(EditorMode::Edit);
        editor.cursor_row = 1;

        editor.delete_line();
        assert_eq!(editor.text(), "line1\nline3");
        assert_eq!(editor.clipboard, Some("line2".to_owned()));
    }

    #[test]
    fn nano_paste_line() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "line1\nline2\nline3",
            KeybindingMode::Nano,
        );
        editor.set_mode(EditorMode::Edit);
        editor.cursor_row = 0;

        editor.delete_line();
        assert_eq!(editor.text(), "line2\nline3");

        editor.cursor_row = 1;
        editor.paste_line();
        assert_eq!(editor.text(), "line2\nline3\nline1");
        assert_eq!(editor.cursor_row, 2);
    }

    #[test]
    fn nano_cut_single_line_clears() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "only",
            KeybindingMode::Nano,
        );
        editor.set_mode(EditorMode::Edit);
        editor.delete_line();
        assert_eq!(editor.text(), "");
        assert_eq!(editor.line_count(), 1);
        assert_eq!(editor.clipboard, Some("only".to_owned()));
    }

    #[test]
    fn nano_mode_label() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "x",
            KeybindingMode::Nano,
        );
        assert_eq!(editor.mode_label(), "SQL [Nano:View]");

        editor.set_mode(EditorMode::Edit);
        assert_eq!(editor.mode_label(), "SQL [Nano:Edit]");
    }

    #[test]
    fn nano_is_inserting_when_edit() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "x",
            KeybindingMode::Nano,
        );
        assert!(!editor.is_inserting());
        editor.set_mode(EditorMode::Edit);
        assert!(editor.is_inserting());
    }

    #[test]
    fn normal_mode_label() {
        let editor = SqlEditor::new("x");
        assert_eq!(editor.mode_label(), "SQL [View]");
    }

    #[test]
    fn set_keybinding_mode_resets_state() {
        let mut editor = SqlEditor::with_keybinding_mode(
            "x",
            KeybindingMode::Vi,
        );
        editor.set_mode(EditorMode::Edit);
        editor.vi_enter_insert();
        editor.vi_press_d();

        editor.set_keybinding_mode(KeybindingMode::Nano);
        assert_eq!(
            editor.keybinding_mode(),
            KeybindingMode::Nano
        );
        assert_eq!(editor.vi_mode(), ViSubMode::Command);
    }

    #[test]
    fn paste_in_view_mode_noop() {
        let mut editor = SqlEditor::new("AB\nCD");
        editor.clipboard = Some("X".to_owned());
        editor.paste_line();
        assert_eq!(editor.text(), "AB\nCD");
    }

    #[test]
    fn delete_line_in_view_mode_noop() {
        let mut editor = SqlEditor::new("AB\nCD");
        editor.delete_line();
        assert_eq!(editor.text(), "AB\nCD");
    }
}
