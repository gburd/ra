//! Prometheus exposition format adapter.
//!
//! Buffers metrics and renders them in the Prometheus text-based
//! exposition format. Includes a [`PromScraper`] that parses the
//! Prometheus text format (as returned by a `/metrics` endpoint)
//! and extracts gauge and counter values.

use super::MonitoringAdapter;
use std::collections::HashMap;

/// A Prometheus metric entry.
#[derive(Debug, Clone)]
pub struct PromMetric {
    /// Metric name (must match `[a-zA-Z_:][a-zA-Z0-9_:]*`).
    pub name: String,
    /// Labels.
    pub labels: Vec<(String, String)>,
    /// Current value.
    pub value: f64,
    /// Help text.
    pub help: Option<String>,
    /// Type annotation.
    pub metric_type: PromType,
}

/// Prometheus metric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromType {
    /// Gauge: value that can go up and down.
    Gauge,
    /// Counter: monotonically increasing value.
    Counter,
    /// Histogram: bucketed distribution.
    Histogram,
}

/// Prometheus adapter that buffers metrics for text exposition.
#[derive(Debug, Default)]
pub struct PrometheusAdapter {
    /// Gauges keyed by (name, sorted-labels).
    gauges: HashMap<String, PromMetric>,
    /// Counters keyed by (name, sorted-labels).
    counters: HashMap<String, PromMetric>,
    /// Histogram observations keyed by name.
    histograms: Vec<PromMetric>,
}

impl PrometheusAdapter {
    /// Create a new adapter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Render all metrics in Prometheus exposition format.
    pub fn render(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();

        for metric in self.gauges.values() {
            let _ = writeln!(out, "# TYPE {} gauge", metric.name);
            out.push_str(&Self::format_line(metric));
        }

        for metric in self.counters.values() {
            let _ = writeln!(out, "# TYPE {} counter", metric.name);
            out.push_str(&Self::format_line(metric));
        }

        out
    }

    fn format_line(m: &PromMetric) -> String {
        if m.labels.is_empty() {
            format!("{} {}\n", m.name, m.value)
        } else {
            let labels: Vec<String> = m
                .labels
                .iter()
                .map(|(k, v)| format!("{k}=\"{v}\""))
                .collect();
            format!(
                "{}{{{}}} {}\n",
                m.name,
                labels.join(","),
                m.value
            )
        }
    }

    fn key(name: &str, tags: &[(&str, &str)]) -> String {
        let mut parts: Vec<String> = tags
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        parts.sort();
        format!("{name}:{}", parts.join(","))
    }

    fn labels_from(tags: &[(&str, &str)]) -> Vec<(String, String)> {
        tags.iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// Access gauge metrics (for testing).
    pub fn gauge_count(&self) -> usize {
        self.gauges.len()
    }

    /// Access counter metrics (for testing).
    pub fn counter_count(&self) -> usize {
        self.counters.len()
    }
}

impl MonitoringAdapter for PrometheusAdapter {
    fn record_gauge(
        &mut self,
        name: &str,
        value: f64,
        tags: &[(&str, &str)],
    ) {
        let key = Self::key(name, tags);
        let entry = self.gauges.entry(key).or_insert_with(|| PromMetric {
            name: name.to_string(),
            labels: Self::labels_from(tags),
            value: 0.0,
            help: None,
            metric_type: PromType::Gauge,
        });
        entry.value = value;
    }

    fn record_histogram(
        &mut self,
        name: &str,
        value: f64,
        tags: &[(&str, &str)],
    ) {
        self.histograms.push(PromMetric {
            name: name.to_string(),
            labels: Self::labels_from(tags),
            value,
            help: None,
            metric_type: PromType::Histogram,
        });
    }

    fn record_counter(
        &mut self,
        name: &str,
        delta: u64,
        tags: &[(&str, &str)],
    ) {
        let key = Self::key(name, tags);
        let entry =
            self.counters.entry(key).or_insert_with(|| PromMetric {
                name: name.to_string(),
                labels: Self::labels_from(tags),
                value: 0.0,
                help: None,
                metric_type: PromType::Counter,
            });
        entry.value += delta as f64;
    }

    fn flush(&mut self) {
        // In production: the Prometheus scraper pulls from render().
        // Nothing to push.
    }

    fn pending_count(&self) -> usize {
        self.gauges.len() + self.counters.len() + self.histograms.len()
    }
}

/// Result of parsing a single metric line from Prometheus text format.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrapedMetric {
    /// Metric name.
    pub name: String,
    /// Label pairs extracted from `{key="val",...}`.
    pub labels: Vec<(String, String)>,
    /// Numeric value.
    pub value: f64,
    /// Type as declared by `# TYPE` directives (if seen).
    pub metric_type: Option<PromType>,
}

/// Parses Prometheus text-format exposition output.
///
/// Handles `# TYPE` directives to annotate metric types and
/// extracts metric name, labels, and value from each sample line.
#[derive(Debug, Default)]
pub struct PromScraper {
    type_hints: HashMap<String, PromType>,
    metrics: Vec<ScrapedMetric>,
}

impl PromScraper {
    /// Create a new scraper.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a full Prometheus text-format response body.
    ///
    /// Populates the internal metric list, which can be retrieved
    /// with [`metrics`](Self::metrics).
    pub fn parse(&mut self, body: &str) {
        self.type_hints.clear();
        self.metrics.clear();

        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("# HELP") {
                continue;
            }
            if line.starts_with("# TYPE ") {
                self.parse_type_directive(line);
            } else if !line.starts_with('#') {
                if let Some(metric) = self.parse_sample_line(line) {
                    self.metrics.push(metric);
                }
            }
        }
    }

    fn parse_type_directive(&mut self, line: &str) {
        let rest = &line["# TYPE ".len()..];
        let mut parts = rest.splitn(2, ' ');
        let Some(name) = parts.next() else { return };
        let Some(type_str) = parts.next() else { return };
        let prom_type = match type_str.trim() {
            "gauge" => PromType::Gauge,
            "counter" => PromType::Counter,
            "histogram" => PromType::Histogram,
            _ => return,
        };
        self.type_hints.insert(name.to_string(), prom_type);
    }

    fn parse_sample_line(&self, line: &str) -> Option<ScrapedMetric> {
        let (name_and_labels, value_str) =
            if let Some(brace_start) = line.find('{') {
                let brace_end = line.find('}')?;
                let after_brace = line[brace_end + 1..].trim();
                let value_str =
                    after_brace.split_whitespace().next()?;
                let name = &line[..brace_start];
                let label_str = &line[brace_start + 1..brace_end];
                let labels = Self::parse_labels(label_str);
                (
                    (name.to_string(), labels),
                    value_str.to_string(),
                )
            } else {
                let mut parts = line.split_whitespace();
                let name = parts.next()?.to_string();
                let value_str = parts.next()?.to_string();
                ((name, Vec::new()), value_str)
            };

        let value: f64 = value_str.parse().ok()?;
        let metric_type =
            self.type_hints.get(&name_and_labels.0).copied();

        Some(ScrapedMetric {
            name: name_and_labels.0,
            labels: name_and_labels.1,
            value,
            metric_type,
        })
    }

    fn parse_labels(label_str: &str) -> Vec<(String, String)> {
        let mut labels = Vec::new();
        for pair in label_str.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            if let Some(eq_pos) = pair.find('=') {
                let key = pair[..eq_pos].trim().to_string();
                let val_raw = pair[eq_pos + 1..].trim();
                let val = val_raw
                    .trim_matches('"')
                    .to_string();
                labels.push((key, val));
            }
        }
        labels
    }

    /// Scraped metrics from the last [`parse`](Self::parse) call.
    pub fn metrics(&self) -> &[ScrapedMetric] {
        &self.metrics
    }

    /// Filter scraped metrics to only gauges.
    pub fn gauges(&self) -> Vec<&ScrapedMetric> {
        self.metrics
            .iter()
            .filter(|m| m.metric_type == Some(PromType::Gauge))
            .collect()
    }

    /// Filter scraped metrics to only counters.
    pub fn counters(&self) -> Vec<&ScrapedMetric> {
        self.metrics
            .iter()
            .filter(|m| m.metric_type == Some(PromType::Counter))
            .collect()
    }

    /// Number of metrics from the last parse.
    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_upsert() {
        let mut p = PrometheusAdapter::new();
        p.record_gauge("cpu", 50.0, &[("host", "db1")]);
        p.record_gauge("cpu", 75.0, &[("host", "db1")]);
        assert_eq!(p.gauge_count(), 1);
        let rendered = p.render();
        assert!(rendered.contains("75"));
    }

    #[test]
    fn counter_accumulates() {
        let mut p = PrometheusAdapter::new();
        p.record_counter("reqs", 10, &[]);
        p.record_counter("reqs", 5, &[]);
        let rendered = p.render();
        assert!(rendered.contains("15"));
    }

    #[test]
    fn render_format() {
        let mut p = PrometheusAdapter::new();
        p.record_gauge("mem_bytes", 1024.0, &[]);
        let rendered = p.render();
        assert!(rendered.contains("# TYPE mem_bytes gauge"));
        assert!(rendered.contains("mem_bytes 1024"));
    }

    #[test]
    fn render_with_labels() {
        let mut p = PrometheusAdapter::new();
        p.record_gauge("cpu", 90.0, &[("host", "db1")]);
        let rendered = p.render();
        assert!(rendered.contains("host=\"db1\""));
    }

    #[test]
    fn different_labels_separate_series() {
        let mut p = PrometheusAdapter::new();
        p.record_gauge("cpu", 50.0, &[("host", "db1")]);
        p.record_gauge("cpu", 75.0, &[("host", "db2")]);
        assert_eq!(p.gauge_count(), 2);
    }

    // ---- PromScraper ----

    #[test]
    fn scraper_parses_simple_gauge() {
        let mut scraper = PromScraper::new();
        scraper.parse("# TYPE cpu gauge\ncpu 75.5\n");
        assert_eq!(scraper.metric_count(), 1);
        let m = &scraper.metrics()[0];
        assert_eq!(m.name, "cpu");
        assert!((m.value - 75.5).abs() < f64::EPSILON);
        assert_eq!(m.metric_type, Some(PromType::Gauge));
    }

    #[test]
    fn scraper_parses_counter() {
        let mut scraper = PromScraper::new();
        scraper.parse(
            "# TYPE http_requests counter\nhttp_requests 1234\n",
        );
        let counters = scraper.counters();
        assert_eq!(counters.len(), 1);
        assert!((counters[0].value - 1234.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scraper_parses_labels() {
        let mut scraper = PromScraper::new();
        scraper.parse(
            "# TYPE cpu gauge\n\
             cpu{host=\"db1\",env=\"prod\"} 90.0\n",
        );
        let m = &scraper.metrics()[0];
        assert_eq!(m.labels.len(), 2);
        assert_eq!(m.labels[0], ("host".into(), "db1".into()));
        assert_eq!(m.labels[1], ("env".into(), "prod".into()));
    }

    #[test]
    fn scraper_multiple_metrics() {
        let mut scraper = PromScraper::new();
        scraper.parse(
            "# TYPE cpu gauge\n\
             cpu 50\n\
             # TYPE mem gauge\n\
             mem 1024\n\
             # TYPE reqs counter\n\
             reqs 999\n",
        );
        assert_eq!(scraper.metric_count(), 3);
        assert_eq!(scraper.gauges().len(), 2);
        assert_eq!(scraper.counters().len(), 1);
    }

    #[test]
    fn scraper_skips_comments_and_help() {
        let mut scraper = PromScraper::new();
        scraper.parse(
            "# HELP cpu CPU usage\n\
             # TYPE cpu gauge\n\
             cpu 42\n",
        );
        assert_eq!(scraper.metric_count(), 1);
    }

    #[test]
    fn scraper_handles_empty_input() {
        let mut scraper = PromScraper::new();
        scraper.parse("");
        assert_eq!(scraper.metric_count(), 0);
    }

    #[test]
    fn scraper_no_type_hint() {
        let mut scraper = PromScraper::new();
        scraper.parse("unknown_metric 3.14\n");
        assert_eq!(scraper.metric_count(), 1);
        assert_eq!(scraper.metrics()[0].metric_type, None);
    }

    #[test]
    fn scraper_roundtrip_with_adapter() {
        let mut adapter = PrometheusAdapter::new();
        adapter.record_gauge("cpu", 80.0, &[("host", "db1")]);
        adapter.record_counter("reqs", 100, &[]);
        let rendered = adapter.render();

        let mut scraper = PromScraper::new();
        scraper.parse(&rendered);
        assert!(scraper.metric_count() >= 2);
    }
}
