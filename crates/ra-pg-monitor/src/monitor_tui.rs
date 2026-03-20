//! TUI monitoring dashboard.
//!
//! Provides a ratatui-based real-time dashboard showing:
//! - Active queries with RA recommendations
//! - Schema issues (missing/unused indexes)
//! - Configuration warnings
//! - Statistics staleness alerts

use std::io;

use crossterm::event::{
    self, Event, KeyCode, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, Paragraph, Wrap,
};
use ratatui::Terminal;

use crate::recommendations::{Advisor, Category, Severity};

/// Which panel is currently focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Queries,
    Schema,
    Config,
    Stats,
}

impl Panel {
    fn next(self) -> Self {
        match self {
            Self::Queries => Self::Schema,
            Self::Schema => Self::Config,
            Self::Config => Self::Stats,
            Self::Stats => Self::Queries,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Queries => Self::Stats,
            Self::Schema => Self::Queries,
            Self::Config => Self::Schema,
            Self::Stats => Self::Config,
        }
    }
}

/// State for the monitoring TUI application.
pub struct MonitorApp {
    advisor: Advisor,
    focused: Panel,
    should_quit: bool,
    scroll_offset: u16,
}

impl MonitorApp {
    /// Create a new monitor app with the given advisor.
    #[must_use]
    pub fn new(advisor: Advisor) -> Self {
        Self {
            advisor,
            focused: Panel::Queries,
            should_quit: false,
            scroll_offset: 0,
        }
    }

    /// Run the TUI in the terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if terminal initialization or
    /// rendering fails.
    pub fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.main_loop(&mut terminal);

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
        )?;
        terminal.show_cursor()?;

        result
    }

    /// Run one render cycle without terminal setup (for testing).
    pub fn render_once(
        &self,
        terminal: &mut Terminal<
            CrosstermBackend<Vec<u8>>,
        >,
    ) -> io::Result<()> {
        terminal.draw(|frame| {
            self.draw(frame);
        })?;
        Ok(())
    }

    /// Get a mutable reference to the advisor.
    pub fn advisor_mut(&mut self) -> &mut Advisor {
        &mut self.advisor
    }

    fn main_loop(
        &mut self,
        terminal: &mut Terminal<
            CrosstermBackend<io::Stdout>,
        >,
    ) -> io::Result<()> {
        loop {
            terminal.draw(|frame| {
                self.draw(frame);
            })?;

            if event::poll(
                std::time::Duration::from_millis(250),
            )? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key.code, key.modifiers);
                }
            }

            if self.should_quit {
                return Ok(());
            }
        }
    }

    fn handle_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('c')
                if modifiers
                    .contains(KeyModifiers::CONTROL) =>
            {
                self.should_quit = true;
            }
            KeyCode::Tab => {
                self.focused = self.focused.next();
                self.scroll_offset = 0;
            }
            KeyCode::BackTab => {
                self.focused = self.focused.prev();
                self.scroll_offset = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_offset =
                    self.scroll_offset.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset =
                    self.scroll_offset.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

        self.draw_title_bar(frame, chunks[0]);
        self.draw_panels(frame, chunks[1]);
        self.draw_status_bar(frame, chunks[2]);
    }

    fn draw_title_bar(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
    ) {
        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                " RA PostgreSQL Monitor ",
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        frame.render_widget(title, area);
    }

    fn draw_panels(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
    ) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(area);

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(rows[0]);

        let bottom_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(rows[1]);

        self.draw_queries_panel(
            frame,
            top_cols[0],
            self.focused == Panel::Queries,
        );
        self.draw_schema_panel(
            frame,
            top_cols[1],
            self.focused == Panel::Schema,
        );
        self.draw_config_panel(
            frame,
            bottom_cols[0],
            self.focused == Panel::Config,
        );
        self.draw_stats_panel(
            frame,
            bottom_cols[1],
            self.focused == Panel::Stats,
        );
    }

    fn draw_queries_panel(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        focused: bool,
    ) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Active Queries ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let queries = self.advisor.query_monitor().recent_queries(20);
        if queries.is_empty() {
            let para = Paragraph::new("No queries recorded")
                .block(block)
                .wrap(Wrap { trim: true });
            frame.render_widget(para, area);
            return;
        }

        let items: Vec<ListItem> = queries
            .iter()
            .map(|q| {
                let severity_color = match q.severity {
                    crate::query_monitor::QuerySeverity::Normal => {
                        Color::Green
                    }
                    crate::query_monitor::QuerySeverity::Slow => {
                        Color::Yellow
                    }
                    crate::query_monitor::QuerySeverity::VerySlow => {
                        Color::Red
                    }
                    crate::query_monitor::QuerySeverity::Critical => {
                        Color::LightRed
                    }
                };
                let truncated = if q.query.len() > 40 {
                    format!("{}...", &q.query[..37])
                } else {
                    q.query.clone()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{}] ", q.severity),
                        Style::default().fg(severity_color),
                    ),
                    Span::raw(format!(
                        "{:.1}ms ",
                        q.duration_ms,
                    )),
                    Span::styled(
                        truncated,
                        Style::default().fg(Color::White),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    fn draw_schema_panel(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        focused: bool,
    ) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Schema Issues ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let recs = self.advisor.all_recommendations();
        let schema_recs: Vec<_> = recs
            .iter()
            .filter(|r| r.category == Category::Schema)
            .collect();

        if schema_recs.is_empty() {
            let para = Paragraph::new("No schema issues found")
                .block(block)
                .wrap(Wrap { trim: true });
            frame.render_widget(para, area);
            return;
        }

        let items: Vec<ListItem> = schema_recs
            .iter()
            .map(|r| {
                let icon = severity_icon(r.severity);
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{icon} "),
                        severity_style(r.severity),
                    ),
                    Span::raw(format!(
                        "{}: {}",
                        r.target, r.message,
                    )),
                ]))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    fn draw_config_panel(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        focused: bool,
    ) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Configuration Issues ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let issues = self.advisor.config_checker().issues();
        if issues.is_empty() {
            let para = Paragraph::new(
                "No configuration issues found",
            )
            .block(block)
            .wrap(Wrap { trim: true });
            frame.render_widget(para, area);
            return;
        }

        let items: Vec<ListItem> = issues
            .iter()
            .map(|issue| {
                let icon = severity_icon(issue.severity);
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{icon} "),
                        severity_style(issue.severity),
                    ),
                    Span::styled(
                        format!("{}: ", issue.parameter),
                        Style::default()
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(issue.message.clone()),
                ]))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    fn draw_stats_panel(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        focused: bool,
    ) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Statistics Staleness ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let findings =
            self.advisor.staleness_checker().findings();
        if findings.is_empty() {
            let para = Paragraph::new(
                "No staleness alerts",
            )
            .block(block)
            .wrap(Wrap { trim: true });
            frame.render_widget(para, area);
            return;
        }

        let items: Vec<ListItem> = findings
            .iter()
            .map(|info| {
                let color = match info.level {
                    crate::stats_staleness::StalenessLevel::Fresh => {
                        Color::Green
                    }
                    crate::stats_staleness::StalenessLevel::Aging => {
                        Color::Yellow
                    }
                    crate::stats_staleness::StalenessLevel::Stale => {
                        Color::Red
                    }
                    crate::stats_staleness::StalenessLevel::VeryStale => {
                        Color::LightRed
                    }
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{}] ", info.level),
                        Style::default().fg(color),
                    ),
                    Span::raw(format!(
                        "{}: {:.1}% modified",
                        info.table,
                        info.modification_ratio * 100.0,
                    )),
                ]))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    fn draw_status_bar(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
    ) {
        let recs = self.advisor.all_recommendations();
        let critical =
            recs.iter()
                .filter(|r| r.severity == Severity::Critical)
                .count();
        let errors =
            recs.iter()
                .filter(|r| r.severity == Severity::Error)
                .count();
        let warnings =
            recs.iter()
                .filter(|r| r.severity == Severity::Warning)
                .count();

        let status = Paragraph::new(Line::from(vec![
            Span::styled(
                " q:quit ",
                Style::default()
                    .fg(Color::DarkGray),
            ),
            Span::styled(
                "Tab:next panel ",
                Style::default()
                    .fg(Color::DarkGray),
            ),
            Span::styled(
                "j/k:scroll ",
                Style::default()
                    .fg(Color::DarkGray),
            ),
            Span::raw("| "),
            Span::styled(
                format!("{critical} critical "),
                Style::default().fg(if critical > 0 {
                    Color::LightRed
                } else {
                    Color::Green
                }),
            ),
            Span::styled(
                format!("{errors} errors "),
                Style::default().fg(if errors > 0 {
                    Color::Red
                } else {
                    Color::Green
                }),
            ),
            Span::styled(
                format!("{warnings} warnings"),
                Style::default().fg(if warnings > 0 {
                    Color::Yellow
                } else {
                    Color::Green
                }),
            ),
        ]));
        frame.render_widget(status, area);
    }
}

fn severity_icon(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "i",
        Severity::Warning => "!",
        Severity::Error => "X",
        Severity::Critical => "!!",
    }
}

fn severity_style(severity: Severity) -> Style {
    match severity {
        Severity::Info => {
            Style::default().fg(Color::Cyan)
        }
        Severity::Warning => {
            Style::default().fg(Color::Yellow)
        }
        Severity::Error => {
            Style::default().fg(Color::Red)
        }
        Severity::Critical => Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bloat_detector::BloatDetector;
    use crate::config_checker::ConfigChecker;
    use crate::query_monitor::QueryMonitor;
    use crate::schema_analyzer::SchemaAnalyzer;
    use crate::stats_staleness::StalenessChecker;

    fn make_advisor() -> Advisor {
        Advisor::new(
            QueryMonitor::new(100.0),
            SchemaAnalyzer::new(),
            ConfigChecker::new(),
            BloatDetector::new(),
            StalenessChecker::new(),
        )
    }

    #[test]
    fn panel_navigation() {
        assert_eq!(Panel::Queries.next(), Panel::Schema);
        assert_eq!(Panel::Schema.next(), Panel::Config);
        assert_eq!(Panel::Config.next(), Panel::Stats);
        assert_eq!(Panel::Stats.next(), Panel::Queries);

        assert_eq!(Panel::Queries.prev(), Panel::Stats);
        assert_eq!(Panel::Schema.prev(), Panel::Queries);
    }

    #[test]
    fn handle_quit_key() {
        let mut app = MonitorApp::new(make_advisor());
        assert!(!app.should_quit);
        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.should_quit);
    }

    #[test]
    fn handle_esc_key() {
        let mut app = MonitorApp::new(make_advisor());
        app.handle_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.should_quit);
    }

    #[test]
    fn handle_ctrl_c() {
        let mut app = MonitorApp::new(make_advisor());
        app.handle_key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        );
        assert!(app.should_quit);
    }

    #[test]
    fn handle_tab_navigation() {
        let mut app = MonitorApp::new(make_advisor());
        assert_eq!(app.focused, Panel::Queries);
        app.handle_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.focused, Panel::Schema);
        app.handle_key(KeyCode::BackTab, KeyModifiers::NONE);
        assert_eq!(app.focused, Panel::Queries);
    }

    #[test]
    fn handle_scroll() {
        let mut app = MonitorApp::new(make_advisor());
        assert_eq!(app.scroll_offset, 0);
        app.handle_key(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.scroll_offset, 1);
        app.handle_key(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(app.scroll_offset, 0);
        // Should not underflow
        app.handle_key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn render_empty_advisor() {
        let app = MonitorApp::new(make_advisor());
        let backend =
            CrosstermBackend::new(Vec::<u8>::new());
        let mut terminal =
            Terminal::new(backend).expect("terminal init failed");
        let result = app.render_once(&mut terminal);
        assert!(result.is_ok());
    }

    #[test]
    fn severity_icons() {
        assert_eq!(severity_icon(Severity::Info), "i");
        assert_eq!(severity_icon(Severity::Warning), "!");
        assert_eq!(severity_icon(Severity::Error), "X");
        assert_eq!(severity_icon(Severity::Critical), "!!");
    }
}
