//! `StatsD`/`DogStatsD` monitoring adapter.
//!
//! Formats metrics in the `StatsD` line protocol. In production this
//! would send UDP packets to a `StatsD` daemon; here it buffers the
//! formatted lines for inspection.

use super::MonitoringAdapter;

/// `StatsD` metric type suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsdType {
    /// Gauge (g).
    Gauge,
    /// Counter (c).
    Counter,
    /// Timer / histogram (ms).
    Timer,
}

impl StatsdType {
    fn suffix(self) -> &'static str {
        match self {
            Self::Gauge => "g",
            Self::Counter => "c",
            Self::Timer => "ms",
        }
    }
}

/// `StatsD` adapter that buffers formatted metric lines.
#[derive(Debug, Default)]
pub struct StatsdAdapter {
    /// Prefix prepended to all metric names.
    prefix: String,
    /// Buffered lines in `StatsD` wire format.
    lines: Vec<String>,
}

impl StatsdAdapter {
    /// Create an adapter with the given metric name prefix.
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            lines: Vec::new(),
        }
    }

    /// Access the buffered `StatsD` lines.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Drain buffered lines.
    pub fn drain(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }

    fn format(
        &self,
        name: &str,
        value: f64,
        metric_type: StatsdType,
        tags: &[(&str, &str)],
    ) -> String {
        let full_name = if self.prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}.{name}", self.prefix)
        };

        let mut line = format!(
            "{full_name}:{value}|{}",
            metric_type.suffix()
        );

        if !tags.is_empty() {
            use std::fmt::Write;
            let tag_str: Vec<String> = tags
                .iter()
                .map(|(k, v)| format!("{k}:{v}"))
                .collect();
            let _ = write!(line, "|#{}", tag_str.join(","));
        }

        line
    }
}

impl MonitoringAdapter for StatsdAdapter {
    fn record_gauge(
        &mut self,
        name: &str,
        value: f64,
        tags: &[(&str, &str)],
    ) {
        let line = self.format(name, value, StatsdType::Gauge, tags);
        self.lines.push(line);
    }

    fn record_histogram(
        &mut self,
        name: &str,
        value: f64,
        tags: &[(&str, &str)],
    ) {
        let line = self.format(name, value, StatsdType::Timer, tags);
        self.lines.push(line);
    }

    fn record_counter(
        &mut self,
        name: &str,
        delta: u64,
        tags: &[(&str, &str)],
    ) {
        let line =
            self.format(name, delta as f64, StatsdType::Counter, tags);
        self.lines.push(line);
    }

    fn flush(&mut self) {
        // In production: send UDP packets to StatsD daemon.
        // Here: no-op, lines remain for inspection.
    }

    fn pending_count(&self) -> usize {
        self.lines.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_format() {
        let mut s = StatsdAdapter::new("ra");
        s.record_gauge("cpu", 75.5, &[]);
        let line = &s.lines()[0];
        assert_eq!(line, "ra.cpu:75.5|g");
    }

    #[test]
    fn counter_format() {
        let mut s = StatsdAdapter::new("ra");
        s.record_counter("queries", 1, &[]);
        let line = &s.lines()[0];
        assert_eq!(line, "ra.queries:1|c");
    }

    #[test]
    fn histogram_format() {
        let mut s = StatsdAdapter::new("ra");
        s.record_histogram("latency", 12.3, &[]);
        let line = &s.lines()[0];
        assert_eq!(line, "ra.latency:12.3|ms");
    }

    #[test]
    fn tags_appended() {
        let mut s = StatsdAdapter::new("ra");
        s.record_gauge(
            "cpu",
            50.0,
            &[("host", "db1"), ("env", "prod")],
        );
        let line = &s.lines()[0];
        assert!(line.contains("|#host:db1,env:prod"));
    }

    #[test]
    fn no_prefix() {
        let mut s = StatsdAdapter::new("");
        s.record_gauge("cpu", 10.0, &[]);
        assert_eq!(s.lines()[0], "cpu:10|g");
    }

    #[test]
    fn drain_clears() {
        let mut s = StatsdAdapter::new("ra");
        s.record_gauge("a", 1.0, &[]);
        s.record_gauge("b", 2.0, &[]);
        let drained = s.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(s.pending_count(), 0);
    }
}
