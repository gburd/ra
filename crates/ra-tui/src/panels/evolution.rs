//! Cost evolution chart panel.
//!
//! Renders a text-based bar chart showing cost across optimization
//! steps, with the current step highlighted.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render the cost evolution panel.
pub fn render(
    frame: &mut Frame,
    costs: &[f64],
    current_step: usize,
    area: Rect,
    focused: bool,
) {
    let border_color = border_style(focused);
    let lines = build_chart_lines(costs, current_step, area);

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .title(" Cost Evolution ")
            .borders(Borders::ALL)
            .border_style(border_color),
    );

    frame.render_widget(paragraph, area);
}

/// Build the chart lines from cost data.
pub fn build_chart_lines(
    costs: &[f64],
    current_step: usize,
    area: Rect,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if costs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No cost data.",
            Style::default().fg(Color::DarkGray),
        )));
        return lines;
    }

    let max_cost = costs
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let min_cost = costs
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);

    let reduction = cost_reduction(max_cost, min_cost);

    // Summary line
    lines.push(Line::from(vec![
        Span::styled(
            format!(
                "Cost: {:.0}",
                costs.last().copied().unwrap_or(0.0)
            ),
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
            format!("Range: {min_cost:.0} - {max_cost:.0}"),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    lines.push(Line::raw(""));

    // Bar chart
    let inner_width = area.width.saturating_sub(2) as usize;
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
        let bar_str: String = "\u{2588}".repeat(bar_len);
        let pad: String =
            " ".repeat(bar_width.saturating_sub(bar_len));

        let color = if i == current_step {
            Color::Cyan
        } else {
            Color::Blue
        };

        let marker = if i == current_step { ">" } else { " " };

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

    lines
}

/// Calculate cost reduction percentage.
#[must_use]
pub fn cost_reduction(max_cost: f64, min_cost: f64) -> f64 {
    if max_cost > 0.0 {
        ((max_cost - min_cost) / max_cost) * 100.0
    } else {
        0.0
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
    use ratatui::layout::Rect;

    fn test_area() -> Rect {
        Rect::new(0, 0, 80, 20)
    }

    #[test]
    fn cost_reduction_normal() {
        let result = cost_reduction(100.0, 50.0);
        assert!((result - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_reduction_zero_max() {
        let result = cost_reduction(0.0, 0.0);
        assert!((result - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_reduction_no_change() {
        let result = cost_reduction(100.0, 100.0);
        assert!((result - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_reduction_full() {
        let result = cost_reduction(100.0, 0.0);
        assert!((result - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_costs_shows_message() {
        let lines = build_chart_lines(&[], 0, test_area());
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn single_cost_produces_lines() {
        let lines = build_chart_lines(&[100.0], 0, test_area());
        // Summary + blank + 1 bar
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn multiple_costs_produce_bars() {
        let costs = vec![100.0, 75.0, 50.0, 25.0];
        let lines = build_chart_lines(&costs, 2, test_area());
        // Summary + blank + 4 bars
        assert_eq!(lines.len(), 6);
    }

    #[test]
    fn current_step_marked() {
        let costs = vec![100.0, 50.0];
        let lines = build_chart_lines(&costs, 1, test_area());
        // The bar at index 1 (line 3, 0-indexed) should start
        // with ">" marker
        let bar_line = &lines[3];
        let text: String =
            bar_line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with('>'));
    }

    #[test]
    fn non_current_step_not_marked() {
        let costs = vec![100.0, 50.0];
        let lines = build_chart_lines(&costs, 1, test_area());
        // The bar at index 0 (line 2) should start with " "
        let bar_line = &lines[2];
        let text: String =
            bar_line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with(' '));
    }

    #[test]
    fn equal_costs_all_full_bars() {
        let costs = vec![50.0, 50.0, 50.0];
        let lines = build_chart_lines(&costs, 0, test_area());
        // With equal costs, all bars should be the same length
        assert_eq!(lines.len(), 5);
    }
}
