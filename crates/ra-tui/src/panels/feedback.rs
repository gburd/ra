//! Execution feedback log panel.
//!
//! Renders rules applied, diagnostic messages, and execution
//! feedback entries with color-coded severity indicators.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::timeline::Snapshot;

/// Render the execution feedback panel.
pub fn render(
    frame: &mut Frame,
    snapshot: &Snapshot,
    area: Rect,
    focused: bool,
    scroll: u16,
) {
    let border_color = border_style(focused);
    let lines = build_feedback_lines(snapshot);

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

/// Build the feedback text lines from a snapshot.
#[must_use]
pub fn build_feedback_lines(
    snapshot: &Snapshot,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if !snapshot.rules_applied.is_empty() {
        lines.push(Line::from(Span::styled(
            "Rules Applied:".to_owned(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        for rule in &snapshot.rules_applied {
            lines.push(Line::from(vec![
                Span::styled(
                    "  + ".to_owned(),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    rule.clone(),
                    Style::default().fg(Color::Cyan),
                ),
            ]));
        }
        lines.push(Line::raw(""));
    }

    if !snapshot.diagnostics.is_empty() {
        lines.push(Line::from(Span::styled(
            "Diagnostics:".to_owned(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        for diag in &snapshot.diagnostics {
            let (color, prefix) = diagnostic_style(diag);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {prefix} "),
                    Style::default().fg(color),
                ),
                Span::styled(
                    diag.clone(),
                    Style::default().fg(color),
                ),
            ]));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No feedback for this step.".to_owned(),
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines
}

/// Determine color and prefix for a diagnostic message.
#[must_use]
pub fn diagnostic_style(diag: &str) -> (Color, &'static str) {
    let lower = diag.to_lowercase();
    if lower.contains("error") || lower.contains("failed") {
        (Color::Red, "X")
    } else if lower.contains("stale") || lower.contains("warning") {
        (Color::Yellow, "!")
    } else if lower.contains("improved") || lower.contains("success") {
        (Color::Green, "+")
    } else {
        (Color::DarkGray, "-")
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
    use crate::timeline::Snapshot;

    fn make_snapshot(
        rules: Vec<&str>,
        diagnostics: Vec<&str>,
    ) -> Snapshot {
        Snapshot {
            label: "test".into(),
            step: 0,
            plan_text: "Scan(t)".into(),
            cost: 100.0,
            rules_applied: rules
                .into_iter()
                .map(String::from)
                .collect(),
            table_stats: vec![],
            diagnostics: diagnostics
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }

    #[test]
    fn empty_feedback_shows_message() {
        let snap = make_snapshot(vec![], vec![]);
        let lines = build_feedback_lines(&snap);
        assert_eq!(lines.len(), 1);
        let text: String =
            lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("No feedback"));
    }

    #[test]
    fn rules_applied_rendered() {
        let snap = make_snapshot(
            vec!["filter-push", "join-reorder"],
            vec![],
        );
        let lines = build_feedback_lines(&snap);
        // Header + 2 rules + blank
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn diagnostics_rendered() {
        let snap = make_snapshot(
            vec![],
            vec!["Some info", "stale statistics"],
        );
        let lines = build_feedback_lines(&snap);
        // Header + 2 diagnostics
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn both_rules_and_diagnostics() {
        let snap = make_snapshot(
            vec!["rule1"],
            vec!["info1"],
        );
        let lines = build_feedback_lines(&snap);
        // Rules header + 1 rule + blank + Diag header + 1 diag
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn diagnostic_style_error() {
        let (color, prefix) = diagnostic_style("Query error occurred");
        assert_eq!(color, Color::Red);
        assert_eq!(prefix, "X");
    }

    #[test]
    fn diagnostic_style_warning() {
        let (color, prefix) = diagnostic_style("stale statistics");
        assert_eq!(color, Color::Yellow);
        assert_eq!(prefix, "!");
    }

    #[test]
    fn diagnostic_style_success() {
        let (color, prefix) =
            diagnostic_style("Plan improved by 20%");
        assert_eq!(color, Color::Green);
        assert_eq!(prefix, "+");
    }

    #[test]
    fn diagnostic_style_info() {
        let (color, prefix) = diagnostic_style("Parsed SQL");
        assert_eq!(color, Color::DarkGray);
        assert_eq!(prefix, "-");
    }

    #[test]
    fn diagnostic_style_failed() {
        let (color, prefix) =
            diagnostic_style("Optimization failed");
        assert_eq!(color, Color::Red);
        assert_eq!(prefix, "X");
    }

    #[test]
    fn diagnostic_style_case_insensitive() {
        let (color, _) = diagnostic_style("WARNING: high latency");
        assert_eq!(color, Color::Yellow);
    }
}
