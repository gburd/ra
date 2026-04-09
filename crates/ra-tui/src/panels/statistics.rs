//! Statistics dashboard panel.
//!
//! Renders a table showing per-table statistics including row counts,
//! staleness indicators with color coding, and confidence levels.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect, Direction};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, Paragraph};

#[cfg(feature = "timeline")]
use crate::timeline::{Change, ChangeSeverity, ChangeKind, TableStatEntry};

/// Render the statistics panel into the given area.
#[cfg(feature = "timeline")]
pub fn render(
    frame: &mut Frame,
    stats: &[TableStatEntry],
    changes: &[Change],
    area: Rect,
    focused: bool,
    _scroll: u16,
) {
    let border_color = border_style(focused);

    // Split area into statistics table and changes section
    let chunks = if changes.is_empty() {
        vec![area]
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                Constraint::Length(changes.len() as u16 + 3),
            ])
            .split(area)
            .to_vec()
    };

    let header = Row::new(vec![
        Cell::from("Table")
            .style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Rows")
            .style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Staleness")
            .style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Conf")
            .style(Style::default().add_modifier(Modifier::BOLD)),
    ])
    .height(1);

    let rows: Vec<Row> = stats
        .iter()
        .map(|ts| {
            let staleness_color = staleness_color(&ts.staleness);
            let conf_color = confidence_color(ts.confidence);
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
        .row_highlight_style(
            Style::default().bg(Color::DarkGray),
        );

    frame.render_widget(table, chunks[0]);

    // Render changes section if present
    if changes.len() > 0 {
        render_changes(frame, changes, chunks[1], focused);
    }
}

/// Map staleness label to a display color.
#[must_use]
pub fn staleness_color(staleness: &str) -> Color {
    match staleness {
        "Fresh" => Color::Green,
        "SlightlyStale" => Color::Yellow,
        "ModeratelyStale" => Color::Rgb(255, 165, 0),
        _ => Color::Red,
    }
}

/// Map confidence level to a display color.
#[must_use]
pub fn confidence_color(confidence: f64) -> Color {
    if confidence >= 0.9 {
        Color::Green
    } else if confidence >= 0.7 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Format a row count with K/M/B suffixes.
#[must_use]
pub fn format_row_count(count: u64) -> String {
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

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Render the changes section showing differences from previous snapshot.
#[cfg(feature = "timeline")]
fn render_changes(
    frame: &mut Frame,
    changes: &[Change],
    area: Rect,
    focused: bool,
) {
    let border_color = border_style(focused);
    let mut lines = Vec::new();

    for change in changes {
        let (icon, color) = change_icon_and_color(change);
        let severity_indicator = severity_indicator(change.severity);

        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", icon),
                Style::default().fg(color),
            ),
            Span::styled(
                severity_indicator,
                Style::default().fg(severity_color(change.severity)),
            ),
            Span::raw(" "),
            Span::styled(
                change.description.clone(),
                Style::default().fg(color),
            ),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No changes from previous snapshot",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .title(" Changes Since Previous ")
            .borders(Borders::ALL)
            .border_style(border_color),
    );

    frame.render_widget(paragraph, area);
}

/// Get icon and color for a change kind.
#[cfg(feature = "timeline")]
#[must_use]
pub fn change_icon_and_color(change: &Change) -> (&'static str, Color) {
    match change.kind {
        ChangeKind::RowCount => ("📊", Color::Cyan),
        ChangeKind::Ndv => ("🔢", Color::Blue),
        ChangeKind::IndexAdded => ("➕", Color::Green),
        ChangeKind::IndexDropped => ("➖", Color::Red),
        ChangeKind::Hardware => ("🖥️", Color::Magenta),
        ChangeKind::Configuration => ("⚙️", Color::Yellow),
        ChangeKind::Schema => ("📋", Color::White),
        ChangeKind::StatsRefresh => ("🔄", Color::Cyan),
    }
}

/// Get severity indicator character.
#[cfg(feature = "timeline")]
#[must_use]
pub fn severity_indicator(severity: ChangeSeverity) -> &'static str {
    match severity {
        ChangeSeverity::Minor => "·",
        ChangeSeverity::Major => "•",
        ChangeSeverity::Critical => "◆",
    }
}

/// Get color for severity level.
#[cfg(feature = "timeline")]
#[must_use]
pub fn severity_color(severity: ChangeSeverity) -> Color {
    match severity {
        ChangeSeverity::Minor => Color::Green,
        ChangeSeverity::Major => Color::Yellow,
        ChangeSeverity::Critical => Color::Red,
    }
}

#[cfg(all(test, feature = "timeline"))]
mod tests {
    use super::*;

    #[test]
    fn format_row_count_small() {
        assert_eq!(format_row_count(500), "500");
    }

    #[test]
    fn format_row_count_thousands() {
        assert_eq!(format_row_count(1_500), "1.5K");
    }

    #[test]
    fn format_row_count_millions() {
        assert_eq!(format_row_count(2_500_000), "2.5M");
    }

    #[test]
    fn format_row_count_billions() {
        assert_eq!(format_row_count(3_500_000_000), "3.5B");
    }

    #[test]
    fn format_row_count_zero() {
        assert_eq!(format_row_count(0), "0");
    }

    #[test]
    fn format_row_count_exact_thousand() {
        assert_eq!(format_row_count(1_000), "1.0K");
    }

    #[test]
    fn format_row_count_exact_million() {
        assert_eq!(format_row_count(1_000_000), "1.0M");
    }

    #[test]
    fn staleness_fresh_is_green() {
        assert_eq!(staleness_color("Fresh"), Color::Green);
    }

    #[test]
    fn staleness_slightly_is_yellow() {
        assert_eq!(
            staleness_color("SlightlyStale"),
            Color::Yellow
        );
    }

    #[test]
    fn staleness_moderately_is_orange() {
        assert_eq!(
            staleness_color("ModeratelyStale"),
            Color::Rgb(255, 165, 0)
        );
    }

    #[test]
    fn staleness_unknown_is_red() {
        assert_eq!(staleness_color("VeryStale"), Color::Red);
    }

    #[test]
    fn confidence_high_is_green() {
        assert_eq!(confidence_color(0.95), Color::Green);
    }

    #[test]
    fn confidence_medium_is_yellow() {
        assert_eq!(confidence_color(0.75), Color::Yellow);
    }

    #[test]
    fn confidence_low_is_red() {
        assert_eq!(confidence_color(0.5), Color::Red);
    }

    #[test]
    fn confidence_boundary_90() {
        assert_eq!(confidence_color(0.9), Color::Green);
    }

    #[test]
    fn confidence_boundary_70() {
        assert_eq!(confidence_color(0.7), Color::Yellow);
    }

    #[test]
    fn change_icon_row_count() {
        let change = Change {
            kind: ChangeKind::RowCount,
            description: "test".into(),
            severity: ChangeSeverity::Minor,
        };
        let (icon, color) = change_icon_and_color(&change);
        assert_eq!(icon, "📊");
        assert_eq!(color, Color::Cyan);
    }

    #[test]
    fn change_icon_index_added() {
        let change = Change {
            kind: ChangeKind::IndexAdded,
            description: "test".into(),
            severity: ChangeSeverity::Major,
        };
        let (icon, color) = change_icon_and_color(&change);
        assert_eq!(icon, "➕");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn change_icon_hardware() {
        let change = Change {
            kind: ChangeKind::Hardware,
            description: "test".into(),
            severity: ChangeSeverity::Critical,
        };
        let (icon, color) = change_icon_and_color(&change);
        assert_eq!(icon, "🖥️");
        assert_eq!(color, Color::Magenta);
    }

    #[test]
    fn severity_minor_is_dot() {
        assert_eq!(severity_indicator(ChangeSeverity::Minor), "·");
    }

    #[test]
    fn severity_major_is_bullet() {
        assert_eq!(severity_indicator(ChangeSeverity::Major), "•");
    }

    #[test]
    fn severity_critical_is_diamond() {
        assert_eq!(severity_indicator(ChangeSeverity::Critical), "◆");
    }

    #[test]
    fn severity_color_minor_is_green() {
        assert_eq!(severity_color(ChangeSeverity::Minor), Color::Green);
    }

    #[test]
    fn severity_color_major_is_yellow() {
        assert_eq!(severity_color(ChangeSeverity::Major), Color::Yellow);
    }

    #[test]
    fn severity_color_critical_is_red() {
        assert_eq!(severity_color(ChangeSeverity::Critical), Color::Red);
    }
}
