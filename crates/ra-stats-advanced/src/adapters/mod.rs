//! Monitoring adapters for exporting streaming statistics.
//!
//! Defines a [`MonitoringAdapter`] trait that concrete backends
//! implement to push metrics to external observability systems.
//! Included adapters:
//!
//! - [`otel::OtelAdapter`] -- OpenTelemetry-compatible
//! - [`prometheus::PrometheusAdapter`] -- Prometheus exposition format
//! - [`statsd::StatsdAdapter`] -- `StatsD`/`DogStatsD` UDP protocol
//!
//! All adapters are intentionally lightweight stubs that record
//! metrics in-memory. In production builds they would be backed by
//! real SDK clients; here they demonstrate the integration seam.

pub mod otel;
pub mod prometheus;
pub mod statsd;

use crate::percentiles::PercentileSummary;

/// A metric sample emitted by the streaming pipeline.
#[derive(Debug, Clone)]
pub struct MetricSample {
    /// Metric name (e.g. `query_latency_ms`).
    pub name: String,
    /// Metric value.
    pub value: f64,
    /// Key-value tags attached to this sample.
    pub tags: Vec<(String, String)>,
}

/// Common interface for monitoring backends.
pub trait MonitoringAdapter: std::fmt::Debug + Send {
    /// Push a single gauge value.
    fn record_gauge(&mut self, name: &str, value: f64, tags: &[(&str, &str)]);

    /// Push a histogram observation.
    fn record_histogram(&mut self, name: &str, value: f64, tags: &[(&str, &str)]);

    /// Push a counter increment.
    fn record_counter(&mut self, name: &str, delta: u64, tags: &[(&str, &str)]);

    /// Push a full percentile summary (p50/p75/p90/p99).
    fn record_summary(&mut self, name: &str, summary: &PercentileSummary, tags: &[(&str, &str)]) {
        let tag_owned: Vec<(String, String)> = tags
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        let tag_refs: Vec<(&str, &str)> = tag_owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        self.record_gauge(&format!("{name}.p50"), summary.p50, &tag_refs);
        self.record_gauge(&format!("{name}.p75"), summary.p75, &tag_refs);
        self.record_gauge(&format!("{name}.p90"), summary.p90, &tag_refs);
        self.record_gauge(&format!("{name}.p99"), summary.p99, &tag_refs);
    }

    /// Flush any buffered data to the backend.
    fn flush(&mut self);

    /// Number of metrics recorded since last flush (for testing).
    fn pending_count(&self) -> usize;
}

/// No-op adapter that discards all metrics.
#[derive(Debug, Default)]
pub struct NullAdapter;

impl MonitoringAdapter for NullAdapter {
    fn record_gauge(&mut self, _name: &str, _value: f64, _tags: &[(&str, &str)]) {}

    fn record_histogram(&mut self, _name: &str, _value: f64, _tags: &[(&str, &str)]) {}

    fn record_counter(&mut self, _name: &str, _delta: u64, _tags: &[(&str, &str)]) {}

    fn flush(&mut self) {}

    fn pending_count(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_adapter_discards() {
        let mut adapter = NullAdapter;
        adapter.record_gauge("cpu", 50.0, &[]);
        adapter.record_counter("queries", 1, &[]);
        adapter.record_histogram("latency", 10.0, &[]);
        assert_eq!(adapter.pending_count(), 0);
    }
}
