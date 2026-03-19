//! Responsive layout system for the TUI.
//!
//! Supports two layout modes:
//! - **Classic**: 4-panel 2x2 grid (statistics, plan, evolution, feedback)
//! - **Editor**: 3-panel with SQL editor on the left, plan tree top-right,
//!   and statistics/feedback split on the bottom
//!
//! Press 'L' to toggle between layouts at runtime.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Which layout mode is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Classic 4-panel 2x2 grid layout.
    Classic,
    /// Editor layout with SQL editor panel.
    Editor,
}

impl LayoutMode {
    /// Toggle to the next layout mode.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Classic => Self::Editor,
            Self::Editor => Self::Classic,
        }
    }
}

/// Layout regions for the full TUI frame.
pub struct FrameLayout {
    /// Top status bar area.
    pub status_bar: Rect,
    /// Main content area (holds the panels).
    pub content: Rect,
    /// Bottom keybindings bar area.
    pub keybindings_bar: Rect,
}

/// Layout regions for the four panels within the content area (classic mode).
pub struct PanelLayout {
    /// Top-left: statistics dashboard.
    pub stats: Rect,
    /// Top-right: plan tree visualization.
    pub plan: Rect,
    /// Bottom-left: cost evolution chart.
    pub evolution: Rect,
    /// Bottom-right: execution feedback log.
    pub feedback: Rect,
}

/// Layout regions for the editor layout (3-panel mode).
pub struct EditorLayout {
    /// Left: SQL editor panel (35% width).
    pub sql_editor: Rect,
    /// Top-right: plan tree (65% width, 55% height).
    pub plan: Rect,
    /// Bottom-left of right side: statistics (50%).
    pub stats: Rect,
    /// Bottom-right of right side: feedback (50%).
    pub feedback: Rect,
}

/// Compute the outer frame layout from the terminal area.
#[must_use]
pub fn frame_layout(area: Rect) -> FrameLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(area);

    FrameLayout {
        status_bar: chunks[0],
        content: chunks[1],
        keybindings_bar: chunks[2],
    }
}

/// Compute the four-panel layout within a content area.
///
/// The layout is responsive: for narrow terminals (< 80 columns),
/// panels stack vertically. Otherwise, a 2x2 grid is used.
#[must_use]
pub fn panel_layout(area: Rect) -> PanelLayout {
    if area.width < 80 {
        stacked_layout(area)
    } else {
        grid_layout(area)
    }
}

/// Compute the editor 3-panel layout within a content area.
///
/// Left side: SQL editor (35% width).
/// Right side split vertically: plan tree (55% top), then
/// bottom split horizontally into stats (50%) and feedback (50%).
#[must_use]
pub fn editor_layout(area: Rect) -> EditorLayout {
    if area.width < 80 {
        return editor_stacked_layout(area);
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(65),
        ])
        .split(area);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ])
        .split(cols[1]);

    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(right_rows[1]);

    EditorLayout {
        sql_editor: cols[0],
        plan: right_rows[0],
        stats: bottom_cols[0],
        feedback: bottom_cols[1],
    }
}

/// Standard 2x2 grid layout for wide terminals.
fn grid_layout(area: Rect) -> PanelLayout {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ])
        .split(area);

    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(65),
        ])
        .split(rows[0]);

    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(rows[1]);

    PanelLayout {
        stats: top_cols[0],
        plan: top_cols[1],
        evolution: bottom_cols[0],
        feedback: bottom_cols[1],
    }
}

/// Stacked vertical layout for narrow terminals.
fn stacked_layout(area: Rect) -> PanelLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

    PanelLayout {
        stats: chunks[0],
        plan: chunks[1],
        evolution: chunks[2],
        feedback: chunks[3],
    }
}

/// Stacked editor layout for narrow terminals.
fn editor_stacked_layout(area: Rect) -> EditorLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(30),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(area);

    EditorLayout {
        sql_editor: chunks[0],
        plan: chunks[1],
        stats: chunks[2],
        feedback: chunks[3],
    }
}

/// Create a centered rectangle of the given percentage
/// within the provided area.
#[must_use]
pub fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: Rect,
) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1]);

    horiz[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_area(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    #[test]
    fn frame_layout_has_three_regions() {
        let fl = frame_layout(test_area(120, 40));
        assert_eq!(fl.status_bar.height, 3);
        assert_eq!(fl.keybindings_bar.height, 1);
        assert!(fl.content.height > 10);
    }

    #[test]
    fn frame_layout_fills_width() {
        let fl = frame_layout(test_area(120, 40));
        assert_eq!(fl.status_bar.width, 120);
        assert_eq!(fl.content.width, 120);
        assert_eq!(fl.keybindings_bar.width, 120);
    }

    #[test]
    fn panel_layout_wide_uses_grid() {
        let pl = panel_layout(test_area(120, 40));
        assert_eq!(pl.stats.y, pl.plan.y);
        assert_eq!(pl.evolution.y, pl.feedback.y);
    }

    #[test]
    fn panel_layout_narrow_stacks() {
        let pl = panel_layout(test_area(60, 40));
        assert_eq!(pl.stats.width, 60);
        assert_eq!(pl.plan.width, 60);
        assert_eq!(pl.evolution.width, 60);
        assert_eq!(pl.feedback.width, 60);
        assert!(pl.plan.y > pl.stats.y);
        assert!(pl.evolution.y > pl.plan.y);
        assert!(pl.feedback.y > pl.evolution.y);
    }

    #[test]
    fn panel_layout_no_overlap() {
        let pl = panel_layout(test_area(120, 40));
        assert!(pl.stats.x + pl.stats.width <= pl.plan.x);
        assert!(
            pl.evolution.x + pl.evolution.width
                <= pl.feedback.x
        );
    }

    #[test]
    fn panel_layout_top_bottom_no_overlap() {
        let pl = panel_layout(test_area(120, 40));
        assert!(
            pl.stats.y + pl.stats.height <= pl.evolution.y
        );
        assert!(
            pl.plan.y + pl.plan.height <= pl.feedback.y
        );
    }

    #[test]
    fn centered_rect_within_bounds() {
        let area = test_area(100, 50);
        let rect = centered_rect(60, 70, area);
        assert!(rect.x >= area.x);
        assert!(rect.y >= area.y);
        assert!(rect.x + rect.width <= area.x + area.width);
        assert!(
            rect.y + rect.height <= area.y + area.height
        );
    }

    #[test]
    fn centered_rect_is_centered() {
        let area = test_area(100, 50);
        let rect = centered_rect(50, 50, area);
        let mid_x = area.x + area.width / 2;
        let rect_mid_x = rect.x + rect.width / 2;
        assert!(
            (mid_x as i32 - rect_mid_x as i32).unsigned_abs()
                <= 1
        );
    }

    #[test]
    fn frame_layout_small_terminal() {
        let fl = frame_layout(test_area(40, 15));
        assert_eq!(fl.status_bar.height, 3);
        assert_eq!(fl.keybindings_bar.height, 1);
        assert!(fl.content.height >= 10);
    }

    #[test]
    fn panel_layout_exact_boundary() {
        let pl = panel_layout(test_area(80, 40));
        assert_eq!(pl.stats.y, pl.plan.y);
    }

    #[test]
    fn panel_layout_boundary_minus_one() {
        let pl = panel_layout(test_area(79, 40));
        assert_eq!(pl.stats.width, 79);
        assert_eq!(pl.plan.width, 79);
    }

    // -- Editor layout tests --

    #[test]
    fn editor_layout_wide_has_left_panel() {
        let el = editor_layout(test_area(120, 40));
        assert_eq!(el.sql_editor.x, 0);
        assert!(el.plan.x > el.sql_editor.x);
    }

    #[test]
    fn editor_layout_no_overlap_horizontal() {
        let el = editor_layout(test_area(120, 40));
        assert!(
            el.sql_editor.x + el.sql_editor.width
                <= el.plan.x
        );
    }

    #[test]
    fn editor_layout_plan_above_bottom() {
        let el = editor_layout(test_area(120, 40));
        assert!(
            el.plan.y + el.plan.height <= el.stats.y
        );
        assert!(
            el.plan.y + el.plan.height <= el.feedback.y
        );
    }

    #[test]
    fn editor_layout_bottom_same_row() {
        let el = editor_layout(test_area(120, 40));
        assert_eq!(el.stats.y, el.feedback.y);
    }

    #[test]
    fn editor_layout_stats_feedback_no_overlap() {
        let el = editor_layout(test_area(120, 40));
        assert!(
            el.stats.x + el.stats.width <= el.feedback.x
        );
    }

    #[test]
    fn editor_layout_narrow_stacks() {
        let el = editor_layout(test_area(60, 40));
        assert_eq!(el.sql_editor.width, 60);
        assert_eq!(el.plan.width, 60);
        assert!(el.plan.y > el.sql_editor.y);
    }

    #[test]
    fn layout_mode_toggle() {
        assert_eq!(
            LayoutMode::Classic.toggle(),
            LayoutMode::Editor
        );
        assert_eq!(
            LayoutMode::Editor.toggle(),
            LayoutMode::Classic
        );
    }

    #[test]
    fn layout_mode_double_toggle_identity() {
        let mode = LayoutMode::Classic;
        assert_eq!(mode.toggle().toggle(), mode);
    }
}
