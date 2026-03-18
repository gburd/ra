//! Plan tree visualization panel.
//!
//! Renders the physical plan tree as color-coded text, with
//! different colors for different operator types (scan, join,
//! filter, aggregate, etc.).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render the plan tree panel into the given area.
pub fn render(
    frame: &mut Frame,
    plan_text: &str,
    area: Rect,
    focused: bool,
    scroll: u16,
) {
    let border_color = border_style(focused);

    let lines: Vec<Line> = plan_text
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

/// Map a plan tree line to a color based on operator type.
#[must_use]
pub fn plan_node_color(line: &str) -> Color {
    let trimmed = line.trim();
    if trimmed.starts_with("Scan")
        || trimmed.starts_with("Index Scan")
        || trimmed.starts_with("Seq Scan")
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
        || trimmed.starts_with("Group")
    {
        Color::Cyan
    } else if trimmed.starts_with("Sort")
        || trimmed.starts_with("Limit")
    {
        Color::Blue
    } else if trimmed.starts_with("Project")
        || trimmed.starts_with("Projection")
    {
        Color::White
    } else {
        Color::DarkGray
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
    fn color_scan() {
        assert_eq!(plan_node_color("Scan(orders)"), Color::Green);
    }

    #[test]
    fn color_index_scan() {
        assert_eq!(
            plan_node_color("Index Scan(orders, idx_pk)"),
            Color::Green,
        );
    }

    #[test]
    fn color_seq_scan() {
        assert_eq!(
            plan_node_color("Seq Scan on lineitem"),
            Color::Green,
        );
    }

    #[test]
    fn color_join() {
        assert_eq!(
            plan_node_color("Hash Join [a.id = b.id]"),
            Color::Magenta,
        );
    }

    #[test]
    fn color_inner_join() {
        assert_eq!(
            plan_node_color("Inner Join [a = b]"),
            Color::Magenta,
        );
    }

    #[test]
    fn color_filter() {
        assert_eq!(
            plan_node_color("Filter [x > 10]"),
            Color::Yellow,
        );
    }

    #[test]
    fn color_filter_detail() {
        assert_eq!(
            plan_node_color("filter: status = 'active'"),
            Color::Yellow,
        );
    }

    #[test]
    fn color_aggregate() {
        assert_eq!(
            plan_node_color("Aggregate [id] SUM(amount)"),
            Color::Cyan,
        );
    }

    #[test]
    fn color_topn() {
        assert_eq!(
            plan_node_color("TopN [cost DESC, n=10]"),
            Color::Cyan,
        );
    }

    #[test]
    fn color_group() {
        assert_eq!(
            plan_node_color("GroupBy [region]"),
            Color::Cyan,
        );
    }

    #[test]
    fn color_sort() {
        assert_eq!(
            plan_node_color("Sort [cost DESC]"),
            Color::Blue,
        );
    }

    #[test]
    fn color_limit() {
        assert_eq!(
            plan_node_color("Limit(count=10)"),
            Color::Blue,
        );
    }

    #[test]
    fn color_project() {
        assert_eq!(
            plan_node_color("Project [id, name]"),
            Color::White,
        );
    }

    #[test]
    fn color_projection() {
        assert_eq!(
            plan_node_color("Projection [a, b, c]"),
            Color::White,
        );
    }

    #[test]
    fn color_unknown() {
        assert_eq!(
            plan_node_color("UnknownOperator"),
            Color::DarkGray,
        );
    }

    #[test]
    fn color_indented_scan() {
        assert_eq!(
            plan_node_color("    Scan(orders)"),
            Color::Green,
        );
    }

    #[test]
    fn color_empty_line() {
        assert_eq!(plan_node_color(""), Color::DarkGray);
    }
}
