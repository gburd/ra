//! Streaming statistics pipeline.
//!
//! Connects ring buffers, percentile tracking, EWMA smoothing, and
//! monitoring adapters into a unified pipeline that ingests raw
//! metric samples and produces smoothed, export-ready statistics.
//!
//! ## Pipeline Architecture
//!
//! ```text
//! Monitoring Sources -> Adapter -> Ring Buffer -> Percentile Tracker
//!                                                      |
//!                                                  Smoother
//!                                                      |
//!                                                  Cost Model
//! ```
//!
//! The [`StreamingPipeline`] orchestrates this flow. Each metric
//! channel has its own ring buffer, percentile tracker, and EWMA
//! smoother. Threshold-based change detection triggers cost model
//! updates only when metrics shift significantly, avoiding
//! unnecessary recomputation.

use std::time::{Duration, Instant};

use crate::adapters::MonitoringAdapter;
use crate::percentiles::PercentileTracker;
use crate::ring_buffer::RingBuffer;
use crate::smoother::Ewma;

/// Default ring buffer capacity per metric channel.
const DEFAULT_BUFFER_CAPACITY: usize = 4096;

/// Default EWMA alpha for smoothing.
const DEFAULT_ALPHA: f64 = 0.1;

/// Minimum interval between cost model updates.
const MIN_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

/// Change thresholds for triggering cost model updates.
#[derive(Debug, Clone, Copy)]
pub struct ChangeThresholds {
    /// Fractional change in CPU metric to trigger update (default 0.10).
    pub cpu: f64,
    /// Fractional change in memory metric (default 0.15).
    pub memory: f64,
    /// Fractional change in I/O metric (default 0.20).
    pub io: f64,
}

impl Default for ChangeThresholds {
    fn default() -> Self {
        Self {
            cpu: 0.10,
            memory: 0.15,
            io: 0.20,
        }
    }
}

/// Resource metric kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricKind {
    /// CPU utilization (0-100 percentage).
    Cpu,
    /// Memory usage (bytes or percentage).
    Memory,
    /// I/O operations (ops/sec or latency).
    Io,
    /// Query latency (milliseconds).
    Latency,
    /// Custom named metric (index into names table).
    Custom(usize),
}

/// Per-metric channel in the pipeline.
struct MetricChannel {
    kind: MetricKind,
    name: String,
    buffer: RingBuffer,
    percentiles: PercentileTracker,
    smoother: Ewma,
    last_smoothed: f64,
}

impl MetricChannel {
    fn new(
        kind: MetricKind,
        name: impl Into<String>,
        capacity: usize,
        alpha: f64,
    ) -> Self {
        let name = name.into();
        Self {
            kind,
            percentiles: PercentileTracker::new(&name),
            name,
            buffer: RingBuffer::new(capacity),
            smoother: Ewma::new(alpha),
            last_smoothed: 0.0,
        }
    }

    fn ingest(&mut self, value: f64) -> f64 {
        self.buffer.push(value);
        self.percentiles.record(value);
        let smoothed = self.smoother.update(value);
        self.last_smoothed = smoothed;
        smoothed
    }
}

impl std::fmt::Debug for MetricChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricChannel")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("samples", &self.buffer.len())
            .field("last_smoothed", &self.last_smoothed)
            .finish_non_exhaustive()
    }
}

/// Snapshot of a cost model update produced by the pipeline.
#[derive(Debug, Clone)]
pub struct CostModelUpdate {
    /// Smoothed CPU metric.
    pub cpu: f64,
    /// Smoothed memory metric.
    pub memory: f64,
    /// Smoothed I/O metric.
    pub io: f64,
    /// Smoothed query latency metric.
    pub latency: f64,
    /// Timestamp of this update.
    pub timestamp: Instant,
}

/// The streaming statistics pipeline.
///
/// Manages multiple metric channels, applies smoothing, and
/// produces cost model updates when thresholds are exceeded.
pub struct StreamingPipeline {
    channels: Vec<MetricChannel>,
    thresholds: ChangeThresholds,
    last_update: Instant,
    last_cpu: f64,
    last_memory: f64,
    last_io: f64,
    update_count: u64,
    sample_count: u64,
    adapter: Option<Box<dyn MonitoringAdapter>>,
}

impl StreamingPipeline {
    /// Create a pipeline with default settings and the four standard
    /// metric channels (CPU, memory, I/O, latency).
    pub fn new() -> Self {
        let alpha = DEFAULT_ALPHA;
        let cap = DEFAULT_BUFFER_CAPACITY;
        Self {
            channels: vec![
                MetricChannel::new(MetricKind::Cpu, "cpu", cap, alpha),
                MetricChannel::new(
                    MetricKind::Memory,
                    "memory",
                    cap,
                    alpha,
                ),
                MetricChannel::new(MetricKind::Io, "io", cap, alpha),
                MetricChannel::new(
                    MetricKind::Latency,
                    "latency",
                    cap,
                    alpha,
                ),
            ],
            thresholds: ChangeThresholds::default(),
            last_update: Instant::now(),
            last_cpu: 0.0,
            last_memory: 0.0,
            last_io: 0.0,
            update_count: 0,
            sample_count: 0,
            adapter: None,
        }
    }

    /// Set change thresholds.
    #[must_use]
    pub fn with_thresholds(mut self, thresholds: ChangeThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Attach a monitoring adapter for metric export.
    #[must_use]
    pub fn with_adapter(
        mut self,
        adapter: Box<dyn MonitoringAdapter>,
    ) -> Self {
        self.adapter = Some(adapter);
        self
    }

    /// Add a custom named metric channel.
    pub fn add_channel(&mut self, name: impl Into<String>) -> usize {
        let idx = self.channels.len();
        let kind = MetricKind::Custom(idx);
        self.channels.push(MetricChannel::new(
            kind,
            name,
            DEFAULT_BUFFER_CAPACITY,
            DEFAULT_ALPHA,
        ));
        idx
    }

    /// Ingest a sample into the named channel.
    ///
    /// Returns the smoothed value, or `None` if the channel does not
    /// exist.
    pub fn ingest(
        &mut self,
        kind: MetricKind,
        value: f64,
    ) -> Option<f64> {
        self.sample_count += 1;
        let channel = self
            .channels
            .iter_mut()
            .find(|c| c.kind == kind)?;
        let smoothed = channel.ingest(value);

        if let Some(adapter) = &mut self.adapter {
            adapter.record_histogram(
                &channel.name,
                value,
                &[],
            );
            adapter.record_gauge(
                &format!("{}.smoothed", channel.name),
                smoothed,
                &[],
            );
        }

        Some(smoothed)
    }

    /// Check whether the current smoothed values exceed thresholds
    /// relative to the last update, and if so produce a
    /// [`CostModelUpdate`].
    ///
    /// Enforces the minimum update interval of 100ms.
    pub fn maybe_update(&mut self) -> Option<CostModelUpdate> {
        let now = Instant::now();
        if now.duration_since(self.last_update) < MIN_UPDATE_INTERVAL {
            return None;
        }

        let cpu = self.smoothed(MetricKind::Cpu);
        let memory = self.smoothed(MetricKind::Memory);
        let io = self.smoothed(MetricKind::Io);
        let latency = self.smoothed(MetricKind::Latency);

        let cpu_changed =
            exceeds_threshold(self.last_cpu, cpu, self.thresholds.cpu);
        let mem_changed = exceeds_threshold(
            self.last_memory,
            memory,
            self.thresholds.memory,
        );
        let io_changed =
            exceeds_threshold(self.last_io, io, self.thresholds.io);

        if !cpu_changed && !mem_changed && !io_changed {
            return None;
        }

        self.last_cpu = cpu;
        self.last_memory = memory;
        self.last_io = io;
        self.last_update = now;
        self.update_count += 1;

        Some(CostModelUpdate {
            cpu,
            memory,
            io,
            latency,
            timestamp: now,
        })
    }

    /// Force an update regardless of thresholds or interval.
    pub fn force_update(&mut self) -> CostModelUpdate {
        let cpu = self.smoothed(MetricKind::Cpu);
        let memory = self.smoothed(MetricKind::Memory);
        let io = self.smoothed(MetricKind::Io);
        let latency = self.smoothed(MetricKind::Latency);
        let now = Instant::now();

        self.last_cpu = cpu;
        self.last_memory = memory;
        self.last_io = io;
        self.last_update = now;
        self.update_count += 1;

        CostModelUpdate {
            cpu,
            memory,
            io,
            latency,
            timestamp: now,
        }
    }

    /// Get the current smoothed value for a metric kind.
    pub fn smoothed(&self, kind: MetricKind) -> f64 {
        self.channels
            .iter()
            .find(|c| c.kind == kind)
            .map_or(0.0, |c| c.last_smoothed)
    }

    /// Get the percentile summary for a metric kind.
    pub fn percentiles(
        &mut self,
        kind: MetricKind,
    ) -> Option<crate::percentiles::PercentileSummary> {
        self.channels
            .iter_mut()
            .find(|c| c.kind == kind)
            .and_then(|c| c.percentiles.summary())
    }

    /// Total samples ingested across all channels.
    pub fn sample_count(&self) -> u64 {
        self.sample_count
    }

    /// Number of cost model updates produced.
    pub fn update_count(&self) -> u64 {
        self.update_count
    }

    /// Number of metric channels.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Flush any attached monitoring adapter.
    pub fn flush(&mut self) {
        if let Some(adapter) = &mut self.adapter {
            adapter.flush();
        }
    }
}

impl Default for StreamingPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for StreamingPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingPipeline")
            .field("channels", &self.channels.len())
            .field("sample_count", &self.sample_count)
            .field("update_count", &self.update_count)
            .finish_non_exhaustive()
    }
}

/// Check whether the relative change between `old` and `new` exceeds
/// the given threshold fraction.
fn exceeds_threshold(old: f64, new: f64, threshold: f64) -> bool {
    if old.abs() < f64::EPSILON {
        return new.abs() > f64::EPSILON;
    }
    ((new - old) / old).abs() > threshold
}

#[cfg(test)]

mod tests {
    use super::*;
    use crate::adapters::otel::OtelAdapter;

    #[test]
    fn pipeline_creation() {
        let p = StreamingPipeline::new();
        assert_eq!(p.channel_count(), 4);
        assert_eq!(p.sample_count(), 0);
        assert_eq!(p.update_count(), 0);
    }

    #[test]
    fn ingest_returns_smoothed() {
        let mut p = StreamingPipeline::new();
        let v = p.ingest(MetricKind::Cpu, 80.0);
        assert!(v.is_some());
        let smoothed = v.expect("should exist");
        assert!((smoothed - 80.0).abs() < f64::EPSILON);
        assert_eq!(p.sample_count(), 1);
    }

    #[test]
    fn ingest_unknown_returns_none() {
        let mut p = StreamingPipeline::new();
        assert!(p.ingest(MetricKind::Custom(999), 1.0).is_none());
    }

    #[test]
    fn smoothed_reflects_ewma() {
        let mut p = StreamingPipeline::new();
        for _ in 0..20 {
            p.ingest(MetricKind::Cpu, 50.0);
        }
        let smoothed = p.smoothed(MetricKind::Cpu);
        assert!(
            (smoothed - 50.0).abs() < 1.0,
            "expected ~50, got {smoothed}"
        );
    }

    #[test]
    fn force_update_always_produces() {
        let mut p = StreamingPipeline::new();
        p.ingest(MetricKind::Cpu, 50.0);
        p.ingest(MetricKind::Memory, 1024.0);
        p.ingest(MetricKind::Io, 100.0);
        p.ingest(MetricKind::Latency, 5.0);

        let update = p.force_update();
        assert!(update.cpu > 0.0);
        assert_eq!(p.update_count(), 1);
    }

    #[test]
    fn maybe_update_respects_min_interval() {
        let mut p = StreamingPipeline::new();
        p.ingest(MetricKind::Cpu, 50.0);
        p.force_update();

        // Immediate second call should be throttled
        p.ingest(MetricKind::Cpu, 100.0);
        assert!(p.maybe_update().is_none());
    }

    #[test]
    fn threshold_detection() {
        assert!(exceeds_threshold(100.0, 115.0, 0.10));
        assert!(!exceeds_threshold(100.0, 105.0, 0.10));
        assert!(exceeds_threshold(0.0, 1.0, 0.10));
        assert!(!exceeds_threshold(0.0, 0.0, 0.10));
    }

    #[test]
    fn add_custom_channel() {
        let mut p = StreamingPipeline::new();
        let idx = p.add_channel("disk_usage");
        assert_eq!(p.channel_count(), 5);
        let kind = MetricKind::Custom(idx);
        p.ingest(kind, 42.0);
        assert!((p.smoothed(kind) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn with_adapter_records_metrics() {
        let adapter = OtelAdapter::new();
        let mut p = StreamingPipeline::new()
            .with_adapter(Box::new(adapter));
        p.ingest(MetricKind::Cpu, 50.0);
        p.ingest(MetricKind::Cpu, 60.0);
        // Adapter is consumed into the pipeline, so we verify via
        // sample count that ingestion worked
        assert_eq!(p.sample_count(), 2);
    }

    #[test]
    fn percentiles_available_after_ingest() {
        let mut p = StreamingPipeline::new();
        for i in 1..=100 {
            p.ingest(MetricKind::Latency, i as f64);
        }
        let summary = p.percentiles(MetricKind::Latency);
        assert!(summary.is_some());
        let s = summary.expect("summary");
        assert!(s.p50 > 0.0);
        assert!(s.p99 > s.p50);
    }

    #[test]
    fn default_thresholds() {
        let t = ChangeThresholds::default();
        assert!((t.cpu - 0.10).abs() < f64::EPSILON);
        assert!((t.memory - 0.15).abs() < f64::EPSILON);
        assert!((t.io - 0.20).abs() < f64::EPSILON);
    }

    #[test]
    fn custom_thresholds() {
        let p = StreamingPipeline::new().with_thresholds(
            ChangeThresholds {
                cpu: 0.05,
                memory: 0.05,
                io: 0.05,
            },
        );
        assert!((p.thresholds.cpu - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn debug_format() {
        let p = StreamingPipeline::new();
        let dbg = format!("{p:?}");
        assert!(dbg.contains("StreamingPipeline"));
    }
}
