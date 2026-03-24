//! Prometheus exposition format adapter.
//!
//! Buffers metrics and renders them in the Prometheus text-based
//! exposition format (for scraping by a Prometheus server at a
//! `/metrics` HTTP endpoint).

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

#[cfg(test)]
#[allow(clippy::float_cmp)]
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
}
