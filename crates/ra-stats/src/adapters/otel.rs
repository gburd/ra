//! OpenTelemetry-compatible monitoring adapter.
//!
//! Records metrics using the OpenTelemetry naming conventions and
//! data model. In a full deployment this would delegate to the
//! `opentelemetry` SDK; here it buffers samples for inspection.

use super::MonitoringAdapter;

/// Buffered metric record in OTEL format.
#[derive(Debug, Clone)]
pub struct OtelMetric {
    /// Instrument name.
    pub name: String,
    /// Metric kind.
    pub kind: OtelMetricKind,
    /// Numeric value.
    pub value: f64,
    /// Attributes (key-value pairs).
    pub attributes: Vec<(String, String)>,
}

/// Metric instrument types matching OTLP data model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtelMetricKind {
    /// Gauge: instantaneous value.
    Gauge,
    /// Histogram: distribution observation.
    Histogram,
    /// Sum: monotonic counter.
    Sum,
}

/// OpenTelemetry adapter that buffers metrics in-memory.
#[derive(Debug, Default)]
pub struct OtelAdapter {
    buffer: Vec<OtelMetric>,
}

impl OtelAdapter {
    /// Create a new adapter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Access buffered metrics (for testing / export).
    pub fn metrics(&self) -> &[OtelMetric] {
        &self.buffer
    }

    /// Drain all buffered metrics.
    pub fn drain(&mut self) -> Vec<OtelMetric> {
        std::mem::take(&mut self.buffer)
    }

    fn push(
        &mut self,
        name: &str,
        kind: OtelMetricKind,
        value: f64,
        tags: &[(&str, &str)],
    ) {
        self.buffer.push(OtelMetric {
            name: name.to_string(),
            kind,
            value,
            attributes: tags
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        });
    }
}

impl MonitoringAdapter for OtelAdapter {
    fn record_gauge(
        &mut self,
        name: &str,
        value: f64,
        tags: &[(&str, &str)],
    ) {
        self.push(name, OtelMetricKind::Gauge, value, tags);
    }

    fn record_histogram(
        &mut self,
        name: &str,
        value: f64,
        tags: &[(&str, &str)],
    ) {
        self.push(name, OtelMetricKind::Histogram, value, tags);
    }

    fn record_counter(
        &mut self,
        name: &str,
        delta: u64,
        tags: &[(&str, &str)],
    ) {
        self.push(name, OtelMetricKind::Sum, delta as f64, tags);
    }

    fn flush(&mut self) {
        // In production: export to OTLP endpoint.
        // Here: no-op, metrics remain in buffer for retrieval.
    }

    fn pending_count(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_gauge() {
        let mut adapter = OtelAdapter::new();
        adapter.record_gauge("cpu_pct", 75.0, &[("host", "db1")]);
        assert_eq!(adapter.pending_count(), 1);
        let m = &adapter.metrics()[0];
        assert_eq!(m.name, "cpu_pct");
        assert_eq!(m.kind, OtelMetricKind::Gauge);
        assert!((m.value - 75.0).abs() < f64::EPSILON);
        assert_eq!(m.attributes[0], ("host".into(), "db1".into()));
    }

    #[test]
    fn records_histogram() {
        let mut adapter = OtelAdapter::new();
        adapter.record_histogram("latency_ms", 12.5, &[]);
        let m = &adapter.metrics()[0];
        assert_eq!(m.kind, OtelMetricKind::Histogram);
    }

    #[test]
    fn records_counter() {
        let mut adapter = OtelAdapter::new();
        adapter.record_counter("requests", 42, &[]);
        let m = &adapter.metrics()[0];
        assert_eq!(m.kind, OtelMetricKind::Sum);
        assert!((m.value - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn drain_clears_buffer() {
        let mut adapter = OtelAdapter::new();
        adapter.record_gauge("a", 1.0, &[]);
        adapter.record_gauge("b", 2.0, &[]);
        let drained = adapter.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(adapter.pending_count(), 0);
    }
}
