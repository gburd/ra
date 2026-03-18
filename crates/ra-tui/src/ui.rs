//! Rendering logic for the 4-panel TUI layout.
//!
//! Uses ratatui widgets to draw statistics, plan trees, cost
//! evolution sparklines, and execution feedback.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap,
};

use crate::app::{App, Panel};
use crate::timeline::Snapshot;

/// Render the full TUI frame.
pub fn render(frame: &mut Frame, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // status bar
            Constraint::Min(10),   // main content
            Constraint::Length(1), // keybindings bar
        ])
        .split(frame.area());

    render_status_bar(frame, app, outer[0]);
    render_panels(frame, app, outer[1]);
    render_keybindings_bar(frame, app, outer[2]);

    if app.show_help {
        render_help_overlay(frame);
    }
}

/// Top status bar: query, step counter, playback state.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let step_total = app.timeline.len();
    let step_cur = app.current_step + 1;

    let snapshot = &app.timeline.snapshots[app.current_step];
    let play_indicator = if app.playing { "PLAY" } else { "PAUSE" };

    let query_display = truncate_str(&app.query_display(), 60);

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
            &snapshot.label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Cost: {:.0}", snapshot.cost),
            Style::default().fg(cost_color(snapshot.cost, app)),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Speed: {}", app.speed_label()),
            Style::default().fg(Color::DarkGray),
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

/// Main 4-panel layout.
fn render_panels(frame: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(rows[0]);

    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    let snapshot = &app.timeline.snapshots[app.current_step];

    render_stats_panel(
        frame,
        snapshot,
        top_cols[0],
        app.focused == Panel::Stats,
        panel_scroll(app, Panel::Stats),
    );
    render_plan_panel(
        frame,
        snapshot,
        top_cols[1],
        app.focused == Panel::Plan,
        panel_scroll(app, Panel::Plan),
    );
    render_evolution_panel(
        frame,
        app,
        bottom_cols[0],
        app.focused == Panel::Evolution,
    );
    render_feedback_panel(
        frame,
        snapshot,
        bottom_cols[1],
        app.focused == Panel::Feedback,
        panel_scroll(app, Panel::Feedback),
    );
}

/// Statistics panel: table stats, staleness, confidence.
fn render_stats_panel(
    frame: &mut Frame,
    snapshot: &Snapshot,
    area: Rect,
    focused: bool,
    scroll: u16,
) {
    let border_color = border_style(focused);

    let header = Row::new(vec![
        Cell::from("Table").style(
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Cell::from("Rows").style(
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Cell::from("Staleness").style(
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Cell::from("Conf").style(
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])
    .height(1);

    let rows: Vec<Row> = snapshot
        .table_stats
        .iter()
        .map(|ts| {
            let staleness_color = match ts.staleness.as_str() {
                "Fresh" => Color::Green,
                "SlightlyStale" => Color::Yellow,
                "ModeratelyStale" => Color::Rgb(255, 165, 0),
                _ => Color::Red,
            };
            let conf_color = if ts.confidence >= 0.9 {
                Color::Green
            } else if ts.confidence >= 0.7 {
                Color::Yellow
            } else {
                Color::Red
            };
            Row::new(vec![
                Cell::from(ts.table.as_str()),
                Cell::from(format_row_count(ts.row_count)),
                Cell::from(Span::styled(
                    ts.staleness.as_str(),
                    Style::default().fg(staleness_color),
                )),
                Cell::from(Span::styled(
                    format!("{:.0}%", ts.confidence * 100.0),
                    Style::default().fg(conf_color),
                )),
            ])
        })
        .collect();

    let widths = [
        Constraint::Min(12),
        Constraint::Min(10),
        Constraint::Min(14),
        Constraint::Min(6),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(" Statistics ")
                .borders(Borders::ALL)
                .border_style(border_color),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_widget(table, area);
    let _ = scroll; // table doesn't use scroll offset
}

/// Plan tree panel: the formatted plan.
fn render_plan_panel(
    frame: &mut Frame,
    snapshot: &Snapshot,
    area: Rect,
    focused: bool,
    scroll: u16,
) {
    let border_color = border_style(focused);

    let lines: Vec<Line> = snapshot
        .plan_text
        .lines()
        .map(|line| {
            let color = plan_node_color(line);
            Line::from(Span::styled(
                line.to_owned(),
                Style::default().fg(color),
            ))
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Plan Tree ")
                .borders(Borders::ALL)
                .border_style(border_color),
        )
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

/// Cost evolution panel: text-based sparkline of cost across steps.
fn render_evolution_panel(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    focused: bool,
) {
    let border_color = border_style(focused);

    let costs: Vec<f64> =
        app.timeline.snapshots.iter().map(|s| s.cost).collect();
    let max_cost = costs
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let min_cost = costs
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);

    let _inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = Vec::new();

    // Cost summary line
    let reduction = if max_cost > 0.0 {
        ((max_cost - min_cost) / max_cost) * 100.0
    } else {
        0.0
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("Cost: {:.0}", costs.last().copied().unwrap_or(0.0)),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("Reduction: {reduction:.1}%"),
            Style::default().fg(Color::Green),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "Range: {:.0} - {:.0}",
                min_cost, max_cost
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    lines.push(Line::raw(""));

    // Bar chart: one bar per step
    let bar_width = if costs.is_empty() {
        0
    } else {
        inner_width.saturating_sub(20).max(1)
    };

    let range = max_cost - min_cost;

    for (i, cost) in costs.iter().enumerate() {
        let fraction = if range > 0.0 {
            (cost - min_cost) / range
        } else {
            1.0
        };

        let bar_len =
            (fraction * bar_width as f64).round() as usize;
        let bar_str: String =
            "\u{2588}".repeat(bar_len);
        let pad: String =
            " ".repeat(bar_width.saturating_sub(bar_len));

        let color = if i == app.current_step {
            Color::Cyan
        } else {
            Color::Blue
        };

        let marker = if i == app.current_step {
            ">"
        } else {
            " "
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("{marker}S{i:<2}"),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(bar_str, Style::default().fg(color)),
            Span::raw(pad),
            Span::styled(
                format!(" {cost:.0}"),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .title(" Cost Evolution ")
            .borders(Borders::ALL)
            .border_style(border_color),
    );

    frame.render_widget(paragraph, area);
}

/// Execution feedback panel: rules applied, diagnostics.
fn render_feedback_panel(
    frame: &mut Frame,
    snapshot: &Snapshot,
    area: Rect,
    focused: bool,
    scroll: u16,
) {
    let border_color = border_style(focused);

    let mut lines: Vec<Line> = Vec::new();

    if !snapshot.rules_applied.is_empty() {
        lines.push(Line::from(Span::styled(
            "Rules Applied:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        for rule in &snapshot.rules_applied {
            lines.push(Line::from(vec![
                Span::styled(
                    "  + ",
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    rule.as_str(),
                    Style::default().fg(Color::Cyan),
                ),
            ]));
        }
        lines.push(Line::raw(""));
    }

    if !snapshot.diagnostics.is_empty() {
        lines.push(Line::from(Span::styled(
            "Diagnostics:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        for diag in &snapshot.diagnostics {
            let (color, prefix) =
                if diag.contains("stale") || diag.contains("warning")
                {
                    (Color::Yellow, "!")
                } else {
                    (Color::DarkGray, "-")
                };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {prefix} "),
                    Style::default().fg(color),
                ),
                Span::styled(
                    diag.as_str(),
                    Style::default().fg(color),
                ),
            ]));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No feedback for this step.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Execution Feedback ")
                .borders(Borders::ALL)
                .border_style(border_color),
        )
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Bottom keybindings bar.
fn render_keybindings_bar(
    frame: &mut Frame,
    _app: &App,
    area: Rect,
) {
    let line = Line::from(vec![
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
            "j/k",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Scroll  "),
        Span::styled(
            "?",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Help"),
    ]);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Help overlay rendered on top of all panels.
fn render_help_overlay(frame: &mut Frame) {
    let area = centered_rect(60, 70, frame.area());
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
        help_line("j / Down", "Scroll down in focused panel"),
        help_line("k / Up", "Scroll up in focused panel"),
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
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

// -- Helpers --

impl App {
    /// Truncated display of the SQL query.
    fn query_display(&self) -> String {
        self.timeline.query.clone()
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_owned()
    } else {
        let mut result = s[..max_len.saturating_sub(3)].to_owned();
        result.push_str("...");
        result
    }
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
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
fn cost_color(cost: f64, app: &App) -> Color {
    let costs: Vec<f64> =
        app.timeline.snapshots.iter().map(|s| s.cost).collect();
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

/// Format a row count with K/M/B suffixes.
fn format_row_count(count: u64) -> String {
    if count >= 1_000_000_000 {
        format!("{:.1}B", count as f64 / 1_000_000_000.0)
    } else if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Color plan tree nodes by operator type.
fn plan_node_color(line: &str) -> Color {
    let trimmed = line.trim();
    if trimmed.starts_with("Scan")
        || trimmed.starts_with("Index Scan")
    {
        Color::Green
    } else if trimmed.contains("Join") {
        Color::Magenta
    } else if trimmed.starts_with("Filter")
        || trimmed.starts_with("filter:")
    {
        Color::Yellow
    } else if trimmed.starts_with("Aggregate")
        || trimmed.starts_with("TopN")
    {
        Color::Cyan
    } else if trimmed.starts_with("Sort")
        || trimmed.starts_with("Limit")
    {
        Color::Blue
    } else if trimmed.starts_with("Project") {
        Color::White
    } else {
        Color::DarkGray
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

/// Create a centered rectangle of the given percentage.
fn centered_rect(
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
