//! `StatsD`/`DogStatsD` monitoring adapter.
//!
//! Formats metrics in the `StatsD` line protocol and includes a
//! [`StatsdParser`] that parses the wire format back into structured
//! metrics. The parser handles counters (`c`), gauges (`g`), and
//! timers (`ms`), plus `DogStatsD` tag extensions.
//!
//! In production the adapter would send UDP packets to a `StatsD`
//! daemon; here it buffers formatted lines for inspection.

use super::MonitoringAdapter;
use std::collections::HashMap;

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

/// A parsed `StatsD` metric from the wire format.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedStatsdMetric {
    /// Metric name (e.g. `ra.cpu`).
    pub name: String,
    /// Numeric value.
    pub value: f64,
    /// Metric type.
    pub metric_type: StatsdType,
    /// `DogStatsD` tags (empty if none).
    pub tags: Vec<(String, String)>,
    /// Sample rate (1.0 if unspecified).
    pub sample_rate: f64,
}

/// Parses `StatsD` wire-format lines and aggregates counters/timers.
///
/// Wire format: `<name>:<value>|<type>[|@<sample_rate>][|#<tags>]`
///
/// The aggregator accumulates counter deltas and collects timer
/// samples for computing summary statistics.
#[derive(Debug, Default)]
pub struct StatsdParser {
    /// Accumulated counter totals keyed by metric name.
    counters: HashMap<String, f64>,
    /// Collected timer samples keyed by metric name.
    timers: HashMap<String, Vec<f64>>,
    /// Most recent gauge values keyed by metric name.
    gauges: HashMap<String, f64>,
    /// Total lines parsed.
    parse_count: u64,
    /// Lines that failed to parse.
    error_count: u64,
}

impl StatsdParser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a single `StatsD` wire-format line.
    ///
    /// Returns the parsed metric on success.
    pub fn parse_line(
        &mut self,
        line: &str,
    ) -> Option<ParsedStatsdMetric> {
        self.parse_count += 1;

        let Some(colon_pos) = line.find(':') else {
            self.error_count += 1;
            return None;
        };
        let name = line[..colon_pos].to_string();
        let rest = &line[colon_pos + 1..];

        let segments: Vec<&str> = rest.split('|').collect();
        if segments.len() < 2 {
            self.error_count += 1;
            return None;
        }

        let Ok(value) = segments[0].parse::<f64>() else {
            self.error_count += 1;
            return None;
        };

        let metric_type = match segments[1] {
            "g" => StatsdType::Gauge,
            "c" => StatsdType::Counter,
            "ms" => StatsdType::Timer,
            _ => {
                self.error_count += 1;
                return None;
            }
        };

        let mut sample_rate = 1.0;
        let mut tags = Vec::new();

        for segment in &segments[2..] {
            if let Some(rate_str) = segment.strip_prefix('@') {
                if let Ok(r) = rate_str.parse::<f64>() {
                    sample_rate = r;
                }
            } else if let Some(tag_str) = segment.strip_prefix('#') {
                for pair in tag_str.split(',') {
                    if let Some(colon) = pair.find(':') {
                        tags.push((
                            pair[..colon].to_string(),
                            pair[colon + 1..].to_string(),
                        ));
                    }
                }
            }
        }

        let metric = ParsedStatsdMetric {
            name: name.clone(),
            value,
            metric_type,
            tags,
            sample_rate,
        };

        match metric_type {
            StatsdType::Counter => {
                let adjusted = value / sample_rate;
                *self.counters.entry(name).or_insert(0.0) += adjusted;
            }
            StatsdType::Timer => {
                self.timers
                    .entry(name)
                    .or_default()
                    .push(value);
            }
            StatsdType::Gauge => {
                self.gauges.insert(name, value);
            }
        }

        Some(metric)
    }

    /// Parse multiple lines (e.g. from a UDP datagram).
    pub fn parse_batch(&mut self, data: &str) -> usize {
        let mut count = 0;
        for line in data.lines() {
            let line = line.trim();
            if !line.is_empty() && self.parse_line(line).is_some() {
                count += 1;
            }
        }
        count
    }

    /// Get accumulated counter value for a metric name.
    pub fn counter_value(&self, name: &str) -> Option<f64> {
        self.counters.get(name).copied()
    }

    /// Get the most recent gauge value for a metric name.
    pub fn gauge_value(&self, name: &str) -> Option<f64> {
        self.gauges.get(name).copied()
    }

    /// Get collected timer samples for a metric name.
    pub fn timer_samples(&self, name: &str) -> Option<&[f64]> {
        self.timers.get(name).map(Vec::as_slice)
    }

    /// Compute the mean of timer samples for a metric.
    pub fn timer_mean(&self, name: &str) -> Option<f64> {
        let samples = self.timers.get(name)?;
        if samples.is_empty() {
            return None;
        }
        let sum: f64 = samples.iter().sum();
        Some(sum / samples.len() as f64)
    }

    /// Total lines attempted.
    pub fn parse_count(&self) -> u64 {
        self.parse_count
    }

    /// Lines that failed to parse.
    pub fn error_count(&self) -> u64 {
        self.error_count
    }

    /// Number of distinct counter names.
    pub fn counter_count(&self) -> usize {
        self.counters.len()
    }

    /// Number of distinct gauge names.
    pub fn gauge_count(&self) -> usize {
        self.gauges.len()
    }

    /// Number of distinct timer names.
    pub fn timer_count(&self) -> usize {
        self.timers.len()
    }

    /// Reset all aggregated state.
    pub fn reset(&mut self) {
        self.counters.clear();
        self.timers.clear();
        self.gauges.clear();
        self.parse_count = 0;
        self.error_count = 0;
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
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

    // ---- StatsdParser ----

    #[test]
    fn parse_counter() {
        let mut parser = StatsdParser::new();
        let m = parser.parse_line("requests:1|c");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.name, "requests");
        assert!((m.value - 1.0).abs() < f64::EPSILON);
        assert_eq!(m.metric_type, StatsdType::Counter);
        assert!((parser.counter_value("requests").unwrap() - 1.0)
            .abs() < f64::EPSILON);
    }

    #[test]
    fn parse_gauge() {
        let mut parser = StatsdParser::new();
        parser.parse_line("cpu:75.5|g");
        assert!((parser.gauge_value("cpu").unwrap() - 75.5).abs()
            < f64::EPSILON);
    }

    #[test]
    fn parse_timer() {
        let mut parser = StatsdParser::new();
        parser.parse_line("latency:12.3|ms");
        parser.parse_line("latency:15.7|ms");
        let samples = parser.timer_samples("latency").unwrap();
        assert_eq!(samples.len(), 2);
        let mean = parser.timer_mean("latency").unwrap();
        assert!((mean - 14.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_with_tags() {
        let mut parser = StatsdParser::new();
        let m = parser
            .parse_line("cpu:50|g|#host:db1,env:prod")
            .unwrap();
        assert_eq!(m.tags.len(), 2);
        assert_eq!(m.tags[0], ("host".into(), "db1".into()));
        assert_eq!(m.tags[1], ("env".into(), "prod".into()));
    }

    #[test]
    fn parse_with_sample_rate() {
        let mut parser = StatsdParser::new();
        let m = parser.parse_line("hits:1|c|@0.5").unwrap();
        assert!((m.sample_rate - 0.5).abs() < f64::EPSILON);
        // Counter should be adjusted: 1 / 0.5 = 2
        assert!((parser.counter_value("hits").unwrap() - 2.0).abs()
            < f64::EPSILON);
    }

    #[test]
    fn counter_accumulates() {
        let mut parser = StatsdParser::new();
        parser.parse_line("reqs:10|c");
        parser.parse_line("reqs:5|c");
        assert!((parser.counter_value("reqs").unwrap() - 15.0).abs()
            < f64::EPSILON);
    }

    #[test]
    fn gauge_overwrites() {
        let mut parser = StatsdParser::new();
        parser.parse_line("cpu:50|g");
        parser.parse_line("cpu:75|g");
        assert!((parser.gauge_value("cpu").unwrap() - 75.0).abs()
            < f64::EPSILON);
        assert_eq!(parser.gauge_count(), 1);
    }

    #[test]
    fn parse_batch() {
        let mut parser = StatsdParser::new();
        let count = parser.parse_batch(
            "cpu:50|g\nreqs:1|c\nlatency:10|ms\n",
        );
        assert_eq!(count, 3);
        assert_eq!(parser.gauge_count(), 1);
        assert_eq!(parser.counter_count(), 1);
        assert_eq!(parser.timer_count(), 1);
    }

    #[test]
    fn parse_invalid_line() {
        let mut parser = StatsdParser::new();
        assert!(parser.parse_line("garbage").is_none());
        assert!(parser.parse_line("no_type:1|x").is_none());
        assert_eq!(parser.error_count(), 2);
    }

    #[test]
    fn reset_clears_state() {
        let mut parser = StatsdParser::new();
        parser.parse_line("cpu:50|g");
        parser.parse_line("reqs:1|c");
        parser.reset();
        assert_eq!(parser.gauge_count(), 0);
        assert_eq!(parser.counter_count(), 0);
        assert_eq!(parser.parse_count(), 0);
    }

    #[test]
    fn roundtrip_adapter_to_parser() {
        let mut adapter = StatsdAdapter::new("ra");
        adapter.record_gauge("cpu", 80.0, &[]);
        adapter.record_counter("reqs", 5, &[]);
        adapter.record_histogram("latency", 10.0, &[]);

        let mut parser = StatsdParser::new();
        for line in adapter.lines() {
            parser.parse_line(line);
        }
        assert!((parser.gauge_value("ra.cpu").unwrap() - 80.0).abs()
            < f64::EPSILON);
        assert!((parser.counter_value("ra.reqs").unwrap() - 5.0).abs()
            < f64::EPSILON);
        assert_eq!(parser.timer_count(), 1);
    }
}
