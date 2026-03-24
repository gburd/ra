//! Benchmarks for streaming statistics pipeline (Track 2).
//!
//! Verifies performance requirements:
//! - Ring buffer: 1M pushes in <100ms, no hot-path allocations
//! - Percentile queries: p50/p75/p90/p99 each <1us with 1K samples
//! - EWMA smoothing: update overhead measurement
//! - Adaptive updates: threshold evaluation cost

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize,
    Criterion,
};
use ra_stats::percentiles::TDigest;
use ra_stats::ring_buffer::RingBuffer;
use ra_stats::smoother::Ewma;
use ra_stats::streaming::{
    ChangeThresholds, MetricKind, StreamingPipeline,
};

// ============================================================
// 1. Ring Buffer Performance
// ============================================================

fn bench_ring_buffer_push_1m(c: &mut Criterion) {
    c.bench_function("ring_buffer_push_1m", |b| {
        b.iter_batched(
            || RingBuffer::new(1_048_576),
            |mut buffer| {
                for i in 0..1_000_000_u64 {
                    buffer.push(black_box(i as f64));
                }
                buffer
            },
            BatchSize::PerIteration,
        );
    });
}

fn bench_ring_buffer_push_single(c: &mut Criterion) {
    let mut buffer = RingBuffer::new(4096);
    // Pre-fill to ensure wraparound path
    for i in 0..4096_u64 {
        buffer.push(i as f64);
    }

    c.bench_function("ring_buffer_push_single", |b| {
        b.iter(|| buffer.push(black_box(42.0)));
    });
}

fn bench_ring_buffer_snapshot(c: &mut Criterion) {
    let mut buffer = RingBuffer::new(4096);
    for i in 0..4096_u64 {
        buffer.push(i as f64);
    }

    c.bench_function("ring_buffer_snapshot_4096", |b| {
        b.iter(|| black_box(buffer.snapshot()));
    });
}

fn bench_ring_buffer_mean(c: &mut Criterion) {
    let mut buffer = RingBuffer::new(4096);
    for i in 0..4096_u64 {
        buffer.push(i as f64);
    }

    c.bench_function("ring_buffer_mean_4096", |b| {
        b.iter(|| black_box(buffer.mean()));
    });
}

// ============================================================
// 2. Percentile Query Performance
// ============================================================

fn bench_percentile_queries_1k(c: &mut Criterion) {
    let mut group = c.benchmark_group("percentile_queries_1k");

    // Build a pre-loaded digest with 1K samples
    let mut td = TDigest::new(200.0);
    for i in 1..=1000 {
        td.add(i as f64);
    }
    // Force compression before benchmarking queries
    td.p50();

    group.bench_function("p50", |b| {
        let mut digest = td.clone();
        b.iter(|| black_box(digest.p50()));
    });

    group.bench_function("p75", |b| {
        let mut digest = td.clone();
        b.iter(|| black_box(digest.p75()));
    });

    group.bench_function("p90", |b| {
        let mut digest = td.clone();
        b.iter(|| black_box(digest.p90()));
    });

    group.bench_function("p99", |b| {
        let mut digest = td.clone();
        b.iter(|| black_box(digest.p99()));
    });

    group.finish();
}

fn bench_tdigest_add(c: &mut Criterion) {
    c.bench_function("tdigest_add_single", |b| {
        let mut td = TDigest::new(200.0);
        let mut i = 0_u64;
        b.iter(|| {
            td.add(black_box(i as f64));
            i = i.wrapping_add(1);
        });
    });
}

fn bench_tdigest_add_10k(c: &mut Criterion) {
    c.bench_function("tdigest_add_10k", |b| {
        b.iter_batched(
            || TDigest::new(200.0),
            |mut td| {
                for i in 0..10_000_u64 {
                    td.add(black_box(i as f64));
                }
                td
            },
            BatchSize::SmallInput,
        );
    });
}

// ============================================================
// 3. EWMA Smoothing Performance
// ============================================================

fn bench_ewma_update(c: &mut Criterion) {
    let mut ewma = Ewma::new(0.1);
    // Establish steady state
    for _ in 0..100 {
        ewma.update(50.0);
    }

    c.bench_function("ewma_update_single", |b| {
        b.iter(|| black_box(ewma.update(black_box(55.0))));
    });
}

fn bench_ewma_update_10k(c: &mut Criterion) {
    c.bench_function("ewma_update_10k", |b| {
        b.iter_batched(
            || Ewma::new(0.1),
            |mut ewma| {
                for i in 0..10_000_u64 {
                    ewma.update(black_box(i as f64));
                }
                ewma
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_ewma_from_half_life(c: &mut Criterion) {
    c.bench_function("ewma_from_half_life", |b| {
        b.iter(|| black_box(Ewma::from_half_life(black_box(20))));
    });
}

// ============================================================
// 4. Adaptive Update (Threshold Evaluation)
// ============================================================

fn bench_pipeline_ingest(c: &mut Criterion) {
    let mut pipeline = StreamingPipeline::new();

    c.bench_function("pipeline_ingest_single", |b| {
        b.iter(|| {
            pipeline
                .ingest(MetricKind::Cpu, black_box(50.0));
        });
    });
}

fn bench_pipeline_force_update(c: &mut Criterion) {
    let mut pipeline = StreamingPipeline::new();
    // Pre-fill with data
    for _ in 0..1000 {
        pipeline.ingest(MetricKind::Cpu, 50.0);
        pipeline.ingest(MetricKind::Memory, 1024.0);
        pipeline.ingest(MetricKind::Io, 100.0);
        pipeline.ingest(MetricKind::Latency, 5.0);
    }

    c.bench_function("pipeline_force_update", |b| {
        b.iter(|| black_box(pipeline.force_update()));
    });
}

fn bench_pipeline_maybe_update(c: &mut Criterion) {
    c.bench_function("pipeline_maybe_update", |b| {
        b.iter_batched(
            || {
                let mut pipeline =
                    StreamingPipeline::new().with_thresholds(
                        ChangeThresholds {
                            cpu: 0.10,
                            memory: 0.15,
                            io: 0.20,
                        },
                    );
                for _ in 0..100 {
                    pipeline.ingest(MetricKind::Cpu, 50.0);
                    pipeline.ingest(MetricKind::Memory, 1024.0);
                    pipeline.ingest(MetricKind::Io, 100.0);
                }
                pipeline.force_update();
                // Push values that differ to trigger threshold
                for _ in 0..50 {
                    pipeline.ingest(MetricKind::Cpu, 90.0);
                }
                pipeline
            },
            |mut pipeline| black_box(pipeline.maybe_update()),
            BatchSize::SmallInput,
        );
    });
}

fn bench_pipeline_smoothed(c: &mut Criterion) {
    let mut pipeline = StreamingPipeline::new();
    for _ in 0..1000 {
        pipeline.ingest(MetricKind::Cpu, 50.0);
    }

    c.bench_function("pipeline_smoothed_lookup", |b| {
        b.iter(|| {
            black_box(pipeline.smoothed(MetricKind::Cpu));
        });
    });
}

fn bench_pipeline_percentiles(c: &mut Criterion) {
    let mut pipeline = StreamingPipeline::new();
    for i in 1..=1000 {
        pipeline.ingest(MetricKind::Latency, i as f64);
    }

    c.bench_function("pipeline_percentiles_1k", |b| {
        b.iter(|| black_box(pipeline.percentiles(MetricKind::Latency)));
    });
}

fn bench_pipeline_ingest_all_channels(c: &mut Criterion) {
    let mut pipeline = StreamingPipeline::new();

    c.bench_function("pipeline_ingest_4_channels", |b| {
        b.iter(|| {
            pipeline
                .ingest(MetricKind::Cpu, black_box(50.0));
            pipeline
                .ingest(MetricKind::Memory, black_box(1024.0));
            pipeline
                .ingest(MetricKind::Io, black_box(100.0));
            pipeline
                .ingest(MetricKind::Latency, black_box(5.0));
        });
    });
}

criterion_group!(
    ring_buffer,
    bench_ring_buffer_push_1m,
    bench_ring_buffer_push_single,
    bench_ring_buffer_snapshot,
    bench_ring_buffer_mean,
);

criterion_group!(
    percentiles,
    bench_percentile_queries_1k,
    bench_tdigest_add,
    bench_tdigest_add_10k,
);

criterion_group!(
    ewma,
    bench_ewma_update,
    bench_ewma_update_10k,
    bench_ewma_from_half_life,
);

criterion_group!(
    adaptive_updates,
    bench_pipeline_ingest,
    bench_pipeline_force_update,
    bench_pipeline_maybe_update,
    bench_pipeline_smoothed,
    bench_pipeline_percentiles,
    bench_pipeline_ingest_all_channels,
);

criterion_main!(ring_buffer, percentiles, ewma, adaptive_updates);
