//! Statistics dashboard panel.
//!
//! Renders a table showing per-table statistics including row counts,
//! staleness indicators with color coding, and confidence levels.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::timeline::TableStatEntry;

/// Render the statistics panel into the given area.
pub fn render(
    frame: &mut Frame,
    stats: &[TableStatEntry],
    area: Rect,
    focused: bool,
    _scroll: u16,
) {
    let border_color = border_style(focused);

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

    frame.render_widget(table, area);
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

#[cfg(test)]
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
}
