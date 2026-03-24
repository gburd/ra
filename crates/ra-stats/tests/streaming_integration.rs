//! Integration tests for the streaming statistics pipeline.
//!
//! Validates ring buffer performance, percentile accuracy, EWMA
//! smoothing behavior, monitoring adapter integration, cost model
//! update logic, and end-to-end pipeline simulation.

use std::time::{Duration, Instant};

use ra_stats::adapters::otel::{OtelAdapter, OtelMetricKind};
use ra_stats::adapters::prometheus::PrometheusAdapter;
use ra_stats::adapters::statsd::StatsdAdapter;
use ra_stats::adapters::MonitoringAdapter;
use ra_stats::percentiles::{PercentileTracker, TDigest};
use ra_stats::ring_buffer::RingBuffer;
use ra_stats::smoother::{Ewma, SmootherSet};
use ra_stats::streaming::{
    ChangeThresholds, MetricKind, StreamingPipeline,
};

// ============================================================
// 1. Ring Buffer Performance Tests
// ============================================================

#[test]
fn ring_buffer_million_samples_under_100ms() {
    let cap = 1_048_576; // 2^20, ~1M
    let mut buffer = RingBuffer::new(cap);
    let start = Instant::now();

    for i in 0..1_000_000 {
        buffer.push(i as f64);
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(100),
        "1M pushes took {elapsed:?}, expected <100ms"
    );
    assert_eq!(buffer.len(), 1_000_000);
}

#[test]
fn ring_buffer_wraparound_correctness_at_scale() {
    let cap = 1024;
    let mut buffer = RingBuffer::new(cap);

    // Push 10x the capacity to force many wraparounds
    for i in 0..10_240 {
        buffer.push(i as f64);
    }

    assert_eq!(buffer.len(), cap);
    let snap = buffer.snapshot();
    assert_eq!(snap.len(), cap);

    // The oldest value should be 10240 - 1024 = 9216
    assert!(
        (snap[0] - 9216.0).abs() < f64::EPSILON,
        "expected oldest=9216, got {}",
        snap[0]
    );
    // The newest should be 10239
    assert!(
        (snap[cap - 1] - 10239.0).abs() < f64::EPSILON,
        "expected newest=10239, got {}",
        snap[cap - 1]
    );
}

#[test]
fn ring_buffer_mean_accuracy_after_wraparound() {
    let cap = 100;
    let mut buffer = RingBuffer::new(cap);

    // Fill with values 901..=1000 via wraparound
    for i in 1..=1000 {
        buffer.push(i as f64);
    }

    let mean = buffer.mean().expect("non-empty buffer should have mean");
    // Mean of 901..=1000 = 950.5
    assert!(
        (mean - 950.5).abs() < f64::EPSILON,
        "expected mean=950.5, got {mean}"
    );
}

#[test]
fn ring_buffer_snapshot_is_ordered() {
    let cap = 256;
    let mut buffer = RingBuffer::new(cap);

    for i in 0..500 {
        buffer.push(i as f64);
    }

    let snap = buffer.snapshot();
    for window in snap.windows(2) {
        assert!(
            window[0] < window[1],
            "snapshot should be oldest-first: {} >= {}",
            window[0],
            window[1]
        );
    }
}

// ============================================================
// 2. Percentile Tracking Accuracy
// ============================================================

/// Verify percentile accuracy within +-5% for a uniform distribution.
fn verify_uniform_percentiles(n: usize, label: &str) {
    let mut td = TDigest::new(200.0);
    for i in 1..=n {
        td.add(i as f64);
    }

    let n_f = n as f64;
    let p50 = td.p50().expect("p50");
    let p75 = td.p75().expect("p75");
    let p90 = td.p90().expect("p90");
    let p99 = td.p99().expect("p99");

    let expected_p50 = n_f * 0.50;
    let expected_p75 = n_f * 0.75;
    let expected_p90 = n_f * 0.90;
    let expected_p99 = n_f * 0.99;

    let tol = 0.05; // 5% tolerance

    assert!(
        (p50 - expected_p50).abs() / expected_p50 < tol,
        "[{label}] p50={p50:.1}, expected ~{expected_p50:.1}"
    );
    assert!(
        (p75 - expected_p75).abs() / expected_p75 < tol,
        "[{label}] p75={p75:.1}, expected ~{expected_p75:.1}"
    );
    assert!(
        (p90 - expected_p90).abs() / expected_p90 < tol,
        "[{label}] p90={p90:.1}, expected ~{expected_p90:.1}"
    );
    assert!(
        (p99 - expected_p99).abs() / expected_p99 < tol,
        "[{label}] p99={p99:.1}, expected ~{expected_p99:.1}"
    );
}

#[test]
fn percentile_uniform_10k() {
    verify_uniform_percentiles(10_000, "10K uniform");
}

#[test]
fn percentile_uniform_100k() {
    verify_uniform_percentiles(100_000, "100K uniform");
}

#[test]
fn percentile_uniform_1m() {
    verify_uniform_percentiles(1_000_000, "1M uniform");
}

#[test]
fn percentile_exponential_distribution() {
    // Simulate an exponential distribution via inverse CDF:
    // F^-1(u) = -ln(1-u)/lambda for u in [0,1)
    let mut td = TDigest::new(200.0);
    let n = 100_000;
    let lambda = 1.0;

    for i in 0..n {
        let u = (i as f64 + 0.5) / n as f64;
        let value = -(1.0 - u).ln() / lambda;
        td.add(value);
    }

    // True p50 of Exp(1) = ln(2) ~ 0.693
    let p50 = td.p50().expect("p50");
    let expected_p50 = 2.0_f64.ln();
    assert!(
        (p50 - expected_p50).abs() / expected_p50 < 0.05,
        "Exp p50={p50:.4}, expected ~{expected_p50:.4}"
    );

    // True p99 of Exp(1) = -ln(0.01) ~ 4.605
    let p99 = td.p99().expect("p99");
    let expected_p99 = -(0.01_f64.ln());
    assert!(
        (p99 - expected_p99).abs() / expected_p99 < 0.05,
        "Exp p99={p99:.4}, expected ~{expected_p99:.4}"
    );
}

#[test]
fn percentile_tracker_with_known_data() {
    let mut tracker = PercentileTracker::new("query_latency_ms");
    for i in 1..=10_000 {
        tracker.record(i as f64);
    }

    let summary = tracker.summary().expect("non-empty tracker");
    assert_eq!(summary.count, 10_000.0);
    assert!((summary.min - 1.0).abs() < f64::EPSILON);
    assert!((summary.max - 10_000.0).abs() < f64::EPSILON);

    // p50 should be ~5000
    let tol = 0.05;
    assert!(
        (summary.p50 - 5000.0).abs() / 5000.0 < tol,
        "tracker p50={}, expected ~5000",
        summary.p50
    );
    assert!(
        (summary.p99 - 9900.0).abs() / 9900.0 < tol,
        "tracker p99={}, expected ~9900",
        summary.p99
    );
}

#[test]
fn percentile_compression_tradeoff() {
    let mut low = TDigest::new(50.0);
    let mut high = TDigest::new(500.0);
    let n = 100_000;

    for i in 1..=n {
        low.add(i as f64);
        high.add(i as f64);
    }

    let expected_p99 = n as f64 * 0.99;

    let low_p99 = low.p99().expect("p99");
    let high_p99 = high.p99().expect("p99");

    // Higher compression should yield better accuracy
    let low_err = (low_p99 - expected_p99).abs() / expected_p99;
    let high_err = (high_p99 - expected_p99).abs() / expected_p99;

    assert!(
        high_err <= low_err + 0.01,
        "high compression error ({high_err:.4}) should be <= \
         low compression error ({low_err:.4})"
    );
}

// ============================================================
// 3. EWMA Smoother Behavior
// ============================================================

#[test]
fn ewma_step_response() {
    // Test that EWMA responds to a sudden jump from 0 to 100
    let mut ewma = Ewma::new(0.3);

    // Establish baseline at 0
    for _ in 0..50 {
        ewma.update(0.0);
    }
    let baseline = ewma.value().expect("baseline");
    assert!(
        baseline.abs() < 0.01,
        "baseline should be ~0, got {baseline}"
    );

    // Step to 100
    for _ in 0..50 {
        ewma.update(100.0);
    }
    let converged = ewma.value().expect("converged");

    // With alpha=0.3 and 50 steps, should be very close to 100
    assert!(
        (converged - 100.0).abs() < 1.0,
        "after step, expected ~100, got {converged}"
    );
}

#[test]
fn ewma_noise_filtering() {
    // Feed a signal of 50 with noise +-20, verify smoothed stays
    // within a tighter band
    let mut ewma = Ewma::new(0.1);
    let signal = 50.0;

    // Build up state
    for _ in 0..100 {
        ewma.update(signal);
    }

    // Now feed noisy samples and track the smoothed output
    let noise_amplitudes = [
        20.0, -15.0, 18.0, -12.0, 19.0, -17.0, 14.0, -20.0, 16.0,
        -11.0, 20.0, -18.0, 15.0, -13.0, 17.0, -19.0, 12.0, -16.0,
        20.0, -14.0,
    ];

    let mut max_deviation = 0.0_f64;
    for noise in noise_amplitudes {
        let smoothed = ewma.update(signal + noise);
        let deviation = (smoothed - signal).abs();
        if deviation > max_deviation {
            max_deviation = deviation;
        }
    }

    // With alpha=0.1, the noise should be heavily dampened
    assert!(
        max_deviation < 10.0,
        "max deviation from signal should be <10, got {max_deviation}"
    );
}

#[test]
fn ewma_half_life_decay_verification() {
    let half_life = 20_u64;
    let mut ewma = Ewma::from_half_life(half_life);

    // Feed a single 1.0 then feed 0.0 for half_life steps
    ewma.update(1.0);
    for _ in 0..half_life {
        ewma.update(0.0);
    }

    let val = ewma.value().expect("val");
    // After half_life steps of 0.0, original should decay to ~0.5
    assert!(
        (val - 0.5).abs() < 0.1,
        "after {half_life} steps, expected ~0.5, got {val}"
    );
}

#[test]
fn ewma_alpha_comparison() {
    // Higher alpha = faster tracking, less smoothing
    let alphas = [0.1, 0.3, 0.5];
    let mut smoothers: Vec<Ewma> =
        alphas.iter().map(|&a| Ewma::new(a)).collect();

    // Establish baseline at 50
    for s in &mut smoothers {
        for _ in 0..50 {
            s.update(50.0);
        }
    }

    // Jump to 100
    for s in &mut smoothers {
        s.update(100.0);
    }

    let values: Vec<f64> = smoothers
        .iter()
        .map(|s| s.value().expect("val"))
        .collect();

    // Higher alpha should track closer to 100 after one step
    assert!(
        values[2] > values[1],
        "alpha=0.5 ({}) should track faster than alpha=0.3 ({})",
        values[2],
        values[1]
    );
    assert!(
        values[1] > values[0],
        "alpha=0.3 ({}) should track faster than alpha=0.1 ({})",
        values[1],
        values[0]
    );
}

#[test]
fn smoother_set_parallel_metrics() {
    let mut set = SmootherSet::new(0.3);
    set.add("cpu");
    set.add("memory");
    set.add("io");

    // Feed different patterns
    for i in 0..100 {
        set.update("cpu", 50.0 + (i as f64 * 0.5));
        set.update("memory", 2048.0);
        set.update("io", 100.0 * (1.0 + (i as f64 * 0.01)));
    }

    // CPU should be trending upward
    let cpu = set.get("cpu").expect("cpu");
    assert!(cpu > 50.0, "CPU should be >50, got {cpu}");

    // Memory should be near 2048
    let mem = set.get("memory").expect("memory");
    assert!(
        (mem - 2048.0).abs() < 10.0,
        "memory should be ~2048, got {mem}"
    );

    // IO should be above baseline
    let io = set.get("io").expect("io");
    assert!(io > 100.0, "IO should be >100, got {io}");
}

// ============================================================
// 4. Monitoring Adapter Integration
// ============================================================

#[test]
fn otel_adapter_captures_all_metric_types() {
    let mut adapter = OtelAdapter::new();

    adapter.record_gauge("cpu_pct", 75.0, &[("host", "db1")]);
    adapter.record_histogram(
        "query_latency_ms",
        12.5,
        &[("query_type", "select")],
    );
    adapter.record_counter("total_queries", 42, &[]);

    assert_eq!(adapter.pending_count(), 3);

    let metrics = adapter.metrics();
    assert_eq!(metrics[0].kind, OtelMetricKind::Gauge);
    assert_eq!(metrics[1].kind, OtelMetricKind::Histogram);
    assert_eq!(metrics[2].kind, OtelMetricKind::Sum);

    // Verify attributes survive round-trip
    assert_eq!(metrics[0].attributes.len(), 1);
    assert_eq!(
        metrics[0].attributes[0],
        ("host".to_string(), "db1".to_string())
    );
}

#[test]
fn otel_adapter_drain_and_refill() {
    let mut adapter = OtelAdapter::new();

    for i in 0..10 {
        adapter.record_gauge("metric", i as f64, &[]);
    }
    assert_eq!(adapter.pending_count(), 10);

    let drained = adapter.drain();
    assert_eq!(drained.len(), 10);
    assert_eq!(adapter.pending_count(), 0);

    // Can refill after drain
    adapter.record_gauge("new_metric", 1.0, &[]);
    assert_eq!(adapter.pending_count(), 1);
}

#[test]
fn prometheus_adapter_gauge_upsert_and_render() {
    let mut adapter = PrometheusAdapter::new();

    // Record multiple gauge updates to same series
    adapter.record_gauge("cpu_pct", 50.0, &[("host", "db1")]);
    adapter.record_gauge("cpu_pct", 90.0, &[("host", "db1")]);

    // Should upsert (not duplicate)
    assert_eq!(adapter.gauge_count(), 1);

    let rendered = adapter.render();
    assert!(rendered.contains("# TYPE cpu_pct gauge"));
    assert!(rendered.contains("90")); // Latest value
    assert!(rendered.contains("host=\"db1\""));
}

#[test]
fn prometheus_adapter_counter_accumulation() {
    let mut adapter = PrometheusAdapter::new();

    adapter.record_counter("total_queries", 100, &[("db", "main")]);
    adapter.record_counter("total_queries", 50, &[("db", "main")]);

    assert_eq!(adapter.counter_count(), 1);

    let rendered = adapter.render();
    assert!(rendered.contains("150")); // Accumulated value
}

#[test]
fn prometheus_adapter_multiple_series() {
    let mut adapter = PrometheusAdapter::new();

    adapter.record_gauge("cpu", 50.0, &[("host", "db1")]);
    adapter.record_gauge("cpu", 75.0, &[("host", "db2")]);
    adapter.record_gauge("mem", 2048.0, &[]);

    // 3 distinct gauge series
    assert_eq!(adapter.gauge_count(), 3);
}

#[test]
fn statsd_adapter_line_format() {
    let mut adapter = StatsdAdapter::new("ra.test");

    adapter.record_gauge("cpu", 75.5, &[]);
    adapter.record_counter("queries", 1, &[]);
    adapter.record_histogram("latency", 12.3, &[]);

    let lines = adapter.lines();
    assert_eq!(lines[0], "ra.test.cpu:75.5|g");
    assert_eq!(lines[1], "ra.test.queries:1|c");
    assert_eq!(lines[2], "ra.test.latency:12.3|ms");
}

#[test]
fn statsd_adapter_tags_format() {
    let mut adapter = StatsdAdapter::new("ra");

    adapter.record_gauge(
        "cpu",
        80.0,
        &[("host", "db1"), ("env", "prod")],
    );

    let line = &adapter.lines()[0];
    assert!(line.contains("|#host:db1,env:prod"));
}

#[test]
fn statsd_adapter_drain_cycle() {
    let mut adapter = StatsdAdapter::new("ra");

    for i in 0..100 {
        adapter.record_gauge("metric", i as f64, &[]);
    }
    assert_eq!(adapter.pending_count(), 100);

    let batch = adapter.drain();
    assert_eq!(batch.len(), 100);
    assert_eq!(adapter.pending_count(), 0);
}

#[test]
fn adapter_summary_records_four_percentile_gauges() {
    let mut adapter = OtelAdapter::new();
    let summary = ra_stats::PercentileSummary {
        p50: 10.0,
        p75: 20.0,
        p90: 30.0,
        p99: 40.0,
        count: 1000.0,
        min: 1.0,
        max: 50.0,
    };

    adapter.record_summary("latency", &summary, &[]);

    // record_summary should emit 4 gauge metrics (p50, p75, p90, p99)
    assert_eq!(adapter.pending_count(), 4);

    let metrics = adapter.metrics();
    let names: Vec<&str> =
        metrics.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"latency.p50"));
    assert!(names.contains(&"latency.p75"));
    assert!(names.contains(&"latency.p90"));
    assert!(names.contains(&"latency.p99"));
}

// ============================================================
// 5. Cost Model Update Logic (via StreamingPipeline)
// ============================================================

#[test]
fn pipeline_updates_on_cpu_threshold() {
    let mut pipeline = StreamingPipeline::new().with_thresholds(
        ChangeThresholds {
            cpu: 0.10,
            memory: 0.15,
            io: 0.20,
        },
    );

    // Establish baseline
    for _ in 0..10 {
        pipeline.ingest(MetricKind::Cpu, 50.0);
        pipeline.ingest(MetricKind::Memory, 1024.0);
        pipeline.ingest(MetricKind::Io, 100.0);
    }

    // Force a first update to set last_* values
    let initial = pipeline.force_update();
    assert!(initial.cpu > 0.0);

    // Now simulate high CPU pressure
    for _ in 0..50 {
        pipeline.ingest(MetricKind::Cpu, 90.0);
    }

    // Wait past the min update interval
    std::thread::sleep(Duration::from_millis(110));

    let update = pipeline.maybe_update();
    assert!(
        update.is_some(),
        "cost model should update when CPU jumps from 50 to 90"
    );

    let u = update.expect("update");
    assert!(
        u.cpu > 50.0,
        "updated CPU should be >50, got {}",
        u.cpu
    );
}

#[test]
fn pipeline_no_update_below_threshold() {
    let mut pipeline = StreamingPipeline::new().with_thresholds(
        ChangeThresholds {
            cpu: 0.50,   // Very high threshold
            memory: 0.50,
            io: 0.50,
        },
    );

    // Establish baseline
    for _ in 0..10 {
        pipeline.ingest(MetricKind::Cpu, 50.0);
        pipeline.ingest(MetricKind::Memory, 1024.0);
        pipeline.ingest(MetricKind::Io, 100.0);
    }

    pipeline.force_update();

    // Small change (10%): won't exceed 50% threshold
    for _ in 0..50 {
        pipeline.ingest(MetricKind::Cpu, 55.0);
    }

    std::thread::sleep(Duration::from_millis(110));

    let update = pipeline.maybe_update();
    assert!(
        update.is_none(),
        "should not update when change is below threshold"
    );
}

#[test]
fn pipeline_respects_min_interval() {
    let mut pipeline = StreamingPipeline::new();

    pipeline.ingest(MetricKind::Cpu, 50.0);
    pipeline.force_update();

    // Large jump, but no time elapsed
    for _ in 0..50 {
        pipeline.ingest(MetricKind::Cpu, 100.0);
    }

    let update = pipeline.maybe_update();
    assert!(
        update.is_none(),
        "should not update within min interval"
    );
}

#[test]
fn pipeline_force_update_always_works() {
    let mut pipeline = StreamingPipeline::new();

    pipeline.ingest(MetricKind::Cpu, 42.0);
    pipeline.ingest(MetricKind::Memory, 2048.0);
    pipeline.ingest(MetricKind::Io, 500.0);
    pipeline.ingest(MetricKind::Latency, 5.5);

    let u = pipeline.force_update();
    assert!((u.cpu - 42.0).abs() < f64::EPSILON);
    assert!((u.memory - 2048.0).abs() < f64::EPSILON);
    assert!((u.io - 500.0).abs() < f64::EPSILON);
    assert!((u.latency - 5.5).abs() < f64::EPSILON);
    assert_eq!(pipeline.update_count(), 1);
}

#[test]
fn pipeline_update_count_tracks_correctly() {
    let mut pipeline = StreamingPipeline::new();

    assert_eq!(pipeline.update_count(), 0);

    pipeline.ingest(MetricKind::Cpu, 50.0);
    pipeline.force_update();
    assert_eq!(pipeline.update_count(), 1);

    pipeline.force_update();
    assert_eq!(pipeline.update_count(), 2);
}

// ============================================================
// 6. End-to-End Simulation
// ============================================================

#[test]
fn end_to_end_load_simulation() {
    let adapter = OtelAdapter::new();
    let mut pipeline = StreamingPipeline::new()
        .with_thresholds(ChangeThresholds {
            cpu: 0.10,
            memory: 0.15,
            io: 0.20,
        })
        .with_adapter(Box::new(adapter));

    // Phase 1: Low load
    for _ in 0..100 {
        pipeline.ingest(MetricKind::Cpu, 20.0);
        pipeline.ingest(MetricKind::Memory, 512.0);
        pipeline.ingest(MetricKind::Io, 50.0);
        pipeline.ingest(MetricKind::Latency, 2.0);
    }

    let initial = pipeline.force_update();
    assert!(initial.cpu < 30.0);
    assert!(initial.latency < 5.0);

    // Phase 2: Load spike
    for _ in 0..200 {
        pipeline.ingest(MetricKind::Cpu, 85.0);
        pipeline.ingest(MetricKind::Memory, 3072.0);
        pipeline.ingest(MetricKind::Io, 500.0);
        pipeline.ingest(MetricKind::Latency, 25.0);
    }

    std::thread::sleep(Duration::from_millis(110));

    let spike_update = pipeline.maybe_update();
    assert!(
        spike_update.is_some(),
        "pipeline should detect load spike"
    );
    let spike = spike_update.expect("spike");
    assert!(spike.cpu > initial.cpu);
    assert!(spike.io > initial.io);

    // Phase 3: Recovery
    for _ in 0..200 {
        pipeline.ingest(MetricKind::Cpu, 25.0);
        pipeline.ingest(MetricKind::Memory, 600.0);
        pipeline.ingest(MetricKind::Io, 60.0);
        pipeline.ingest(MetricKind::Latency, 3.0);
    }

    std::thread::sleep(Duration::from_millis(110));

    let recovery = pipeline.maybe_update();
    assert!(
        recovery.is_some(),
        "pipeline should detect recovery"
    );
    let recovered = recovery.expect("recovered");
    assert!(
        recovered.cpu < spike.cpu,
        "CPU should drop: {} vs {}",
        recovered.cpu,
        spike.cpu
    );
}

#[test]
fn end_to_end_sample_count_tracking() {
    let mut pipeline = StreamingPipeline::new();

    let n = 1000;
    for _ in 0..n {
        pipeline.ingest(MetricKind::Cpu, 50.0);
        pipeline.ingest(MetricKind::Memory, 1024.0);
    }

    // Each iteration ingests 2 samples
    assert_eq!(pipeline.sample_count(), n * 2);
}

#[test]
fn end_to_end_percentile_access() {
    let mut pipeline = StreamingPipeline::new();

    // Feed known latency values 1..=1000
    for i in 1..=1000 {
        pipeline.ingest(MetricKind::Latency, i as f64);
    }

    let summary = pipeline.percentiles(MetricKind::Latency);
    assert!(summary.is_some());
    let s = summary.expect("percentile summary");

    assert!((s.min - 1.0).abs() < f64::EPSILON);
    assert!((s.max - 1000.0).abs() < f64::EPSILON);
    assert!(s.p50 > 400.0 && s.p50 < 600.0);
    assert!(s.p99 > 950.0);
}

#[test]
fn end_to_end_custom_channel() {
    let mut pipeline = StreamingPipeline::new();
    let idx = pipeline.add_channel("disk_iops");

    assert_eq!(pipeline.channel_count(), 5); // 4 standard + 1 custom

    let kind = MetricKind::Custom(idx);
    for _ in 0..50 {
        pipeline.ingest(kind, 500.0);
    }

    let smoothed = pipeline.smoothed(kind);
    assert!(
        (smoothed - 500.0).abs() < 1.0,
        "custom channel smoothed={smoothed}, expected ~500"
    );
}

#[test]
fn update_latency_under_1ms() {
    let mut pipeline = StreamingPipeline::new();

    // Pre-fill with data
    for _ in 0..1000 {
        pipeline.ingest(MetricKind::Cpu, 50.0);
        pipeline.ingest(MetricKind::Memory, 1024.0);
        pipeline.ingest(MetricKind::Io, 100.0);
        pipeline.ingest(MetricKind::Latency, 5.0);
    }

    // Measure force_update latency
    let start = Instant::now();
    for _ in 0..1000 {
        pipeline.force_update();
    }
    let elapsed = start.elapsed();
    let per_update = elapsed / 1000;

    assert!(
        per_update < Duration::from_millis(1),
        "per-update latency = {per_update:?}, expected <1ms"
    );
}

#[test]
fn ingest_throughput_benchmark() {
    let mut pipeline = StreamingPipeline::new();

    let start = Instant::now();
    for i in 0..100_000 {
        pipeline.ingest(MetricKind::Cpu, (i % 100) as f64);
    }
    let elapsed = start.elapsed();

    // 100K ingests should complete well under 1 second
    assert!(
        elapsed < Duration::from_secs(1),
        "100K ingests took {elapsed:?}, expected <1s"
    );
}

// ============================================================
// 7. Pipeline with Adapter Integration
// ============================================================

#[test]
fn pipeline_with_otel_adapter() {
    let adapter = OtelAdapter::new();
    let mut pipeline =
        StreamingPipeline::new().with_adapter(Box::new(adapter));

    for _ in 0..10 {
        pipeline.ingest(MetricKind::Cpu, 50.0);
        pipeline.ingest(MetricKind::Latency, 10.0);
    }

    // Adapter receives metrics during ingest (histogram + gauge
    // per ingest call = 2 metrics per ingest)
    assert_eq!(pipeline.sample_count(), 20);
}

#[test]
fn pipeline_with_statsd_adapter() {
    let adapter = StatsdAdapter::new("ra.pipeline");
    let mut pipeline =
        StreamingPipeline::new().with_adapter(Box::new(adapter));

    pipeline.ingest(MetricKind::Cpu, 75.0);
    pipeline.ingest(MetricKind::Memory, 2048.0);

    assert_eq!(pipeline.sample_count(), 2);
}

#[test]
fn pipeline_with_prometheus_adapter() {
    let adapter = PrometheusAdapter::new();
    let mut pipeline =
        StreamingPipeline::new().with_adapter(Box::new(adapter));

    for _ in 0..10 {
        pipeline.ingest(MetricKind::Cpu, 60.0);
    }

    assert_eq!(pipeline.sample_count(), 10);
}

// ============================================================
// 8. Edge Cases and Boundary Conditions
// ============================================================

#[test]
fn ring_buffer_capacity_one_stress() {
    let mut buf = RingBuffer::new(1);
    for i in 0..10_000 {
        buf.push(i as f64);
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.snapshot(), vec![i as f64]);
    }
}

#[test]
fn tdigest_single_value_all_percentiles_equal() {
    let mut td = TDigest::default();
    td.add(42.0);

    let p50 = td.p50().expect("p50");
    let p99 = td.p99().expect("p99");

    // All percentiles should be approximately 42
    assert!((p50 - 42.0).abs() < 1.0);
    assert!((p99 - 42.0).abs() < 1.0);
}

#[test]
fn tdigest_two_values_boundary() {
    let mut td = TDigest::default();
    td.add(0.0);
    td.add(100.0);

    let p50 = td.p50().expect("p50");
    assert!(
        p50 >= 0.0 && p50 <= 100.0,
        "p50={p50} should be in [0, 100]"
    );
}

#[test]
fn ewma_steady_state_convergence() {
    let mut ewma = Ewma::new(0.1);
    let target = 42.0;

    for _ in 0..1000 {
        ewma.update(target);
    }

    let val = ewma.value().expect("val");
    assert!(
        (val - target).abs() < f64::EPSILON * 100.0,
        "EWMA should converge to target, got {val}"
    );
}

#[test]
fn pipeline_default_channels() {
    let pipeline = StreamingPipeline::default();
    assert_eq!(pipeline.channel_count(), 4);
    assert_eq!(pipeline.sample_count(), 0);
    assert_eq!(pipeline.update_count(), 0);
}

#[test]
fn pipeline_smoothed_zero_before_ingest() {
    let pipeline = StreamingPipeline::new();
    assert!(
        pipeline.smoothed(MetricKind::Cpu).abs() < f64::EPSILON
    );
    assert!(
        pipeline.smoothed(MetricKind::Memory).abs() < f64::EPSILON
    );
}

#[test]
fn pipeline_percentiles_none_before_ingest() {
    let mut pipeline = StreamingPipeline::new();
    assert!(pipeline.percentiles(MetricKind::Cpu).is_none());
}
