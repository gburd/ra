//! OpenTelemetry-compatible monitoring adapter.
//!
//! Records metrics using the OpenTelemetry naming conventions and
//! data model. Includes an [`OtelIngester`] that routes OTEL semantic
//! convention metric names (`system.cpu.utilization`,
//! `system.memory.usage`, `system.disk.io`) into per-resource
//! [`RingBuffer`](crate::ring_buffer::RingBuffer) channels.
//!
//! In a full deployment the adapter would delegate to the
//! `opentelemetry` SDK; here it buffers samples for inspection.

use super::MonitoringAdapter;
use crate::ring_buffer::RingBuffer;

/// Default ring buffer capacity for ingester channels.
const INGESTER_BUFFER_CAPACITY: usize = 1024;

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

/// Routes OpenTelemetry semantic-convention metric names into
/// per-resource ring buffers for downstream consumption.
///
/// Recognizes:
/// - `system.cpu.utilization` -> CPU channel
/// - `system.memory.usage`   -> memory channel
/// - `system.disk.io`        -> I/O channel
pub struct OtelIngester {
    cpu_buffer: RingBuffer,
    mem_buffer: RingBuffer,
    io_buffer: RingBuffer,
    ingest_count: u64,
    unrecognized_count: u64,
}

impl OtelIngester {
    /// Create an ingester with default buffer capacity (1024).
    pub fn new() -> Self {
        Self::with_capacity(INGESTER_BUFFER_CAPACITY)
    }

    /// Create an ingester with a custom buffer capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            cpu_buffer: RingBuffer::new(capacity),
            mem_buffer: RingBuffer::new(capacity),
            io_buffer: RingBuffer::new(capacity),
            ingest_count: 0,
            unrecognized_count: 0,
        }
    }

    /// Ingest a metric by its OTEL semantic-convention name.
    ///
    /// Returns `true` if the metric was routed to a known channel.
    pub fn ingest_metric(&mut self, name: &str, value: f64) -> bool {
        self.ingest_count += 1;
        match name {
            "system.cpu.utilization" => {
                self.cpu_buffer.push(value);
                true
            }
            "system.memory.usage" => {
                self.mem_buffer.push(value);
                true
            }
            "system.disk.io" => {
                self.io_buffer.push(value);
                true
            }
            _ => {
                self.unrecognized_count += 1;
                false
            }
        }
    }

    /// Snapshot of the CPU ring buffer.
    pub fn cpu_snapshot(&self) -> Vec<f64> {
        self.cpu_buffer.snapshot()
    }

    /// Snapshot of the memory ring buffer.
    pub fn memory_snapshot(&self) -> Vec<f64> {
        self.mem_buffer.snapshot()
    }

    /// Snapshot of the I/O ring buffer.
    pub fn io_snapshot(&self) -> Vec<f64> {
        self.io_buffer.snapshot()
    }

    /// Mean of the CPU buffer, or `None` if empty.
    pub fn cpu_mean(&self) -> Option<f64> {
        self.cpu_buffer.mean()
    }

    /// Mean of the memory buffer, or `None` if empty.
    pub fn memory_mean(&self) -> Option<f64> {
        self.mem_buffer.mean()
    }

    /// Mean of the I/O buffer, or `None` if empty.
    pub fn io_mean(&self) -> Option<f64> {
        self.io_buffer.mean()
    }

    /// Total metrics ingested (including unrecognized).
    pub fn ingest_count(&self) -> u64 {
        self.ingest_count
    }

    /// Count of unrecognized metric names.
    pub fn unrecognized_count(&self) -> u64 {
        self.unrecognized_count
    }
}

impl Default for OtelIngester {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OtelIngester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtelIngester")
            .field("cpu_samples", &self.cpu_buffer.len())
            .field("mem_samples", &self.mem_buffer.len())
            .field("io_samples", &self.io_buffer.len())
            .field("ingest_count", &self.ingest_count)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::expect_used)]
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

    // ---- OtelIngester ----

    #[test]
    fn ingester_routes_cpu() {
        let mut ingester = OtelIngester::new();
        assert!(ingester.ingest_metric("system.cpu.utilization", 0.75));
        assert_eq!(ingester.cpu_snapshot(), vec![0.75]);
        assert!(ingester.memory_snapshot().is_empty());
        assert!(ingester.io_snapshot().is_empty());
    }

    #[test]
    fn ingester_routes_memory() {
        let mut ingester = OtelIngester::new();
        assert!(ingester.ingest_metric("system.memory.usage", 4096.0));
        assert_eq!(ingester.memory_snapshot(), vec![4096.0]);
    }

    #[test]
    fn ingester_routes_io() {
        let mut ingester = OtelIngester::new();
        assert!(ingester.ingest_metric("system.disk.io", 512.0));
        assert_eq!(ingester.io_snapshot(), vec![512.0]);
    }

    #[test]
    fn ingester_ignores_unknown() {
        let mut ingester = OtelIngester::new();
        assert!(!ingester.ingest_metric("custom.metric", 1.0));
        assert_eq!(ingester.unrecognized_count(), 1);
        assert_eq!(ingester.ingest_count(), 1);
    }

    #[test]
    fn ingester_mean_values() {
        let mut ingester = OtelIngester::new();
        ingester.ingest_metric("system.cpu.utilization", 0.50);
        ingester.ingest_metric("system.cpu.utilization", 0.70);
        let mean = ingester.cpu_mean().expect("should have mean");
        assert!((mean - 0.60).abs() < f64::EPSILON);
    }

    #[test]
    fn ingester_mean_empty() {
        let ingester = OtelIngester::new();
        assert!(ingester.cpu_mean().is_none());
        assert!(ingester.memory_mean().is_none());
        assert!(ingester.io_mean().is_none());
    }

    #[test]
    fn ingester_ring_buffer_wraps() {
        let mut ingester = OtelIngester::with_capacity(3);
        ingester.ingest_metric("system.cpu.utilization", 1.0);
        ingester.ingest_metric("system.cpu.utilization", 2.0);
        ingester.ingest_metric("system.cpu.utilization", 3.0);
        ingester.ingest_metric("system.cpu.utilization", 4.0);
        assert_eq!(ingester.cpu_snapshot(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn ingester_counts_all_ingests() {
        let mut ingester = OtelIngester::new();
        ingester.ingest_metric("system.cpu.utilization", 0.5);
        ingester.ingest_metric("system.memory.usage", 1024.0);
        ingester.ingest_metric("unknown", 0.0);
        assert_eq!(ingester.ingest_count(), 3);
        assert_eq!(ingester.unrecognized_count(), 1);
    }

    #[test]
    fn ingester_debug_format() {
        let ingester = OtelIngester::new();
        let dbg = format!("{ingester:?}");
        assert!(dbg.contains("OtelIngester"));
    }

    #[test]
    fn ingester_default() {
        let ingester = OtelIngester::default();
        assert_eq!(ingester.ingest_count(), 0);
    }
}
