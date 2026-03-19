//! Rendering logic for the TUI layout.
//!
//! Supports both the classic 4-panel layout and the editor 3-panel
//! layout. Delegates to individual panel modules for rendering and
//! uses the layout module for responsive panel positioning.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{App, Panel};
use crate::layout::{self, LayoutMode};
use crate::panels::{
    evolution, feedback, plan_tree, sql_editor, statistics,
};

/// Render the full TUI frame.
pub fn render(frame: &mut Frame, app: &App) {
    let fl = layout::frame_layout(frame.area());

    render_status_bar(frame, app, fl.status_bar);

    match app.layout_mode {
        LayoutMode::Classic => {
            render_classic_panels(frame, app, fl.content);
        }
        LayoutMode::Editor => {
            render_editor_panels(frame, app, fl.content);
        }
    }

    render_keybindings_bar(frame, app, fl.keybindings_bar);

    if app.show_help {
        render_help_overlay(frame);
    }
}

/// Top status bar: query, step counter, playback state.
fn render_status_bar(
    frame: &mut Frame,
    app: &App,
    area: Rect,
) {
    let step_total = app.timeline.len();
    let step_cur = app.current_step + 1;

    let snapshot =
        &app.timeline.snapshots[app.current_step];
    let play_indicator =
        if app.playing { "PLAY" } else { "PAUSE" };

    let query_display =
        truncate_str(&app.timeline.query, 60);

    let layout_label = match app.layout_mode {
        LayoutMode::Classic => "4-Panel",
        LayoutMode::Editor => "Editor",
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" {play_indicator} "),
            Style::default()
                .fg(if app.playing {
                    Color::Green
                } else {
                    Color::Yellow
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("Step {step_cur}/{step_total}"),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" | "),
        Span::styled(
            snapshot.label.clone(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Cost: {:.0}", snapshot.cost),
            Style::default()
                .fg(cost_color(snapshot.cost, app)),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Speed: {}", app.speed_label()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("[{layout_label}]"),
            Style::default().fg(Color::Magenta),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("SQL: {query_display}"),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));
    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);
}

/// Classic 4-panel layout using dedicated panel modules.
fn render_classic_panels(
    frame: &mut Frame,
    app: &App,
    area: Rect,
) {
    let pl = layout::panel_layout(area);
    let snapshot =
        &app.timeline.snapshots[app.current_step];

    statistics::render(
        frame,
        &snapshot.table_stats,
        pl.stats,
        app.focused == Panel::Stats,
        panel_scroll(app, Panel::Stats),
    );

    plan_tree::render(
        frame,
        &snapshot.plan_text,
        pl.plan,
        app.focused == Panel::Plan,
        panel_scroll(app, Panel::Plan),
    );

    let costs: Vec<f64> = app
        .timeline
        .snapshots
        .iter()
        .map(|s| s.cost)
        .collect();

    evolution::render(
        frame,
        &costs,
        app.current_step,
        pl.evolution,
        app.focused == Panel::Evolution,
    );

    feedback::render(
        frame,
        snapshot,
        pl.feedback,
        app.focused == Panel::Feedback,
        panel_scroll(app, Panel::Feedback),
    );
}

/// Editor 3-panel layout: SQL editor + plan + stats/feedback.
fn render_editor_panels(
    frame: &mut Frame,
    app: &App,
    area: Rect,
) {
    let el = layout::editor_layout(area);
    let snapshot =
        &app.timeline.snapshots[app.current_step];

    sql_editor::render(
        frame,
        &app.sql_editor,
        el.sql_editor,
        app.focused == Panel::SqlEditor,
    );

    plan_tree::render(
        frame,
        &snapshot.plan_text,
        el.plan,
        app.focused == Panel::Plan,
        panel_scroll(app, Panel::Plan),
    );

    statistics::render(
        frame,
        &snapshot.table_stats,
        el.stats,
        app.focused == Panel::Stats,
        panel_scroll(app, Panel::Stats),
    );

    feedback::render(
        frame,
        snapshot,
        el.feedback,
        app.focused == Panel::Feedback,
        panel_scroll(app, Panel::Feedback),
    );
}

/// Bottom keybindings bar.
fn render_keybindings_bar(
    frame: &mut Frame,
    app: &App,
    area: Rect,
) {
    let mut spans = vec![
        Span::styled(
            " q",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Quit  "),
        Span::styled(
            "Space",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Play/Pause  "),
        Span::styled(
            "</>",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Step  "),
        Span::styled(
            "+/-",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Speed  "),
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Panel  "),
        Span::styled(
            "L",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Layout  "),
    ];

    if app.layout_mode == LayoutMode::Editor {
        spans.push(Span::styled(
            "E",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" Edit SQL  "));
    }

    spans.push(Span::styled(
        "?",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw(" Help"));

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Help overlay rendered on top of all panels.
fn render_help_overlay(frame: &mut Frame) {
    let area =
        layout::centered_rect(60, 70, frame.area());
    frame.render_widget(Clear, area);

    let help_text = vec![
        Line::from(Span::styled(
            "Keyboard Shortcuts",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        help_line("q / Ctrl-C", "Quit"),
        help_line("Space", "Toggle play/pause"),
        help_line("Right / l", "Next optimization step"),
        help_line("Left / h", "Previous optimization step"),
        help_line("Home", "Jump to first step"),
        help_line("End", "Jump to last step"),
        help_line("+ / =", "Increase playback speed"),
        help_line("-", "Decrease playback speed"),
        help_line("Tab", "Focus next panel"),
        help_line("Shift+Tab", "Focus previous panel"),
        help_line("j / Down", "Scroll down"),
        help_line("k / Up", "Scroll up"),
        help_line("L", "Toggle layout (4-panel/editor)"),
        help_line("E", "Toggle SQL editor mode"),
        help_line("Esc", "Exit edit mode"),
        help_line("?", "Toggle this help"),
        Line::raw(""),
        Line::from(Span::styled(
            "Press ? to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(
                    Style::default().fg(Color::Cyan),
                ),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

// -- Helpers --

/// Truncate a string, appending "..." if it exceeds `max_len`.
#[must_use]
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_owned()
    } else {
        let mut result =
            s[..max_len.saturating_sub(3)].to_owned();
        result.push_str("...");
        result
    }
}

fn panel_scroll(app: &App, panel: Panel) -> u16 {
    if app.focused == panel {
        app.scroll_offset
    } else {
        0
    }
}

/// Map cost to a color relative to the timeline range.
#[must_use]
pub fn cost_color(cost: f64, app: &App) -> Color {
    let costs: Vec<f64> = app
        .timeline
        .snapshots
        .iter()
        .map(|s| s.cost)
        .collect();
    let max = costs
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let min = costs
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);

    if max <= min {
        return Color::White;
    }

    let fraction = (cost - min) / (max - min);
    if fraction < 0.33 {
        Color::Green
    } else if fraction < 0.66 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn help_line<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {key:<16}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(desc),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Timeline;

    #[test]
    fn truncate_short_string() {
        let result = truncate_str("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn truncate_long_string() {
        let result =
            truncate_str("hello world this is long", 10);
        assert_eq!(result, "hello w...");
    }

    #[test]
    fn truncate_exact_length() {
        let result = truncate_str("12345", 5);
        assert_eq!(result, "12345");
    }

    #[test]
    fn truncate_empty() {
        let result = truncate_str("", 10);
        assert_eq!(result, "");
    }

    #[test]
    fn cost_color_single_cost() {
        let tl = Timeline::demo();
        let app = App::new(tl).unwrap_or_else(|e| {
            panic!("app creation failed: {e}");
        });
        let color = cost_color(50_000.0, &app);
        assert!(matches!(
            color,
            Color::Green | Color::Yellow | Color::Red
        ));
    }

    #[test]
    fn cost_color_equal_costs_is_white() {
        use crate::timeline::Snapshot;
        let mut tl = Timeline::new("SELECT 1", "auto");
        tl.push(Snapshot {
            label: "s1".into(),
            step: 0,
            plan_text: "Scan".into(),
            cost: 100.0,
            rules_applied: vec![],
            table_stats: vec![],
            diagnostics: vec![],
        });
        tl.push(Snapshot {
            label: "s2".into(),
            step: 1,
            plan_text: "Scan".into(),
            cost: 100.0,
            rules_applied: vec![],
            table_stats: vec![],
            diagnostics: vec![],
        });
        let app = App::new(tl).unwrap_or_else(|e| {
            panic!("app creation failed: {e}");
        });
        let color = cost_color(100.0, &app);
        assert_eq!(color, Color::White);
    }
}
