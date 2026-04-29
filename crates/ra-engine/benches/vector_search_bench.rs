#![expect(clippy::print_stdout, reason = "benchmark diagnostic output")]
//! Benchmarks for vector similarity search cost models.
//!
//! Validates Phase 4 targets:
//! - HNSW: 10-100x faster than sequential scan
//! - `IVFFlat`: 5-50x faster than sequential scan
//! - Dimension scaling is linear for distance calculations
//! - Logarithmic scaling for HNSW with dataset size

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ra_engine::vector_cost::{
    hnsw_search_cost, ivfflat_search_cost, select_vector_index_type, vector_distance_cost,
    vector_sequential_scan_cost, QueryFrequency, VectorIndexType, VectorMetric,
};
use ra_engine::vector_rules::{
    estimate_vector_query_cost, optimize_vector_filter_order, VectorIndexParams,
};

fn bench_distance_costs(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector_distance");

    for dimensions in [64, 128, 256, 512, 1024, 1536] {
        group.bench_with_input(
            BenchmarkId::new("l2", dimensions),
            &dimensions,
            |b, &dims| {
                b.iter(|| black_box(vector_distance_cost(dims, VectorMetric::L2)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("cosine", dimensions),
            &dimensions,
            |b, &dims| {
                b.iter(|| black_box(vector_distance_cost(dims, VectorMetric::Cosine)));
            },
        );
    }

    group.finish();
}

fn bench_hnsw_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("hnsw_scaling");

    let dimensions = 128;
    let m = 16;
    let ef_search = 40;
    let k = 10;

    for total_vectors in [1_000, 10_000, 100_000, 1_000_000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(total_vectors),
            &total_vectors,
            |b, &n| {
                b.iter(|| {
                    black_box(hnsw_search_cost(
                        dimensions,
                        m,
                        ef_search,
                        n,
                        k,
                        VectorMetric::L2,
                    ))
                });
            },
        );
    }

    group.finish();
}

fn bench_ivfflat_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("ivfflat_scaling");

    let dimensions = 128;
    let k = 10;

    for total_vectors in [10_000, 50_000, 100_000, 500_000] {
        let lists = (total_vectors as f64).sqrt() as usize;
        let probes = (lists / 10).max(1);

        group.bench_with_input(
            BenchmarkId::from_parameter(total_vectors),
            &total_vectors,
            |b, &n| {
                b.iter(|| {
                    black_box(ivfflat_search_cost(
                        dimensions,
                        lists,
                        probes,
                        n,
                        k,
                        VectorMetric::L2,
                    ))
                });
            },
        );
    }

    group.finish();
}

fn bench_sequential_vs_hnsw(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_vs_hnsw");

    let dimensions = 128;
    let total_vectors = 100_000;
    let m = 16;
    let ef_search = 40;
    let k = 10;

    group.bench_function("sequential", |b| {
        b.iter(|| {
            black_box(vector_sequential_scan_cost(
                dimensions,
                total_vectors,
                VectorMetric::L2,
            ))
        });
    });

    group.bench_function("hnsw", |b| {
        b.iter(|| {
            black_box(hnsw_search_cost(
                dimensions,
                m,
                ef_search,
                total_vectors,
                k,
                VectorMetric::L2,
            ))
        });
    });

    group.finish();

    // Verify speedup target
    let seq_cost = vector_sequential_scan_cost(dimensions, total_vectors, VectorMetric::L2);
    let hnsw_cost = hnsw_search_cost(dimensions, m, ef_search, total_vectors, k, VectorMetric::L2);
    let speedup = seq_cost.total() / hnsw_cost.total();

    println!("HNSW speedup: {speedup:.1}x (target: 10-100x)");
    assert!(
        speedup >= 10.0,
        "HNSW speedup {speedup:.1}x below target 10x",
    );
}

fn bench_sequential_vs_ivfflat(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_vs_ivfflat");

    let dimensions = 128;
    let total_vectors = 50_000;
    let lists = 200;
    let probes = 10;
    let k = 10;

    group.bench_function("sequential", |b| {
        b.iter(|| {
            black_box(vector_sequential_scan_cost(
                dimensions,
                total_vectors,
                VectorMetric::L2,
            ))
        });
    });

    group.bench_function("ivfflat", |b| {
        b.iter(|| {
            black_box(ivfflat_search_cost(
                dimensions,
                lists,
                probes,
                total_vectors,
                k,
                VectorMetric::L2,
            ))
        });
    });

    group.finish();

    // Verify speedup target
    let seq_cost = vector_sequential_scan_cost(dimensions, total_vectors, VectorMetric::L2);
    let ivfflat_cost = ivfflat_search_cost(
        dimensions,
        lists,
        probes,
        total_vectors,
        k,
        VectorMetric::L2,
    );
    let speedup = seq_cost.total() / ivfflat_cost.total();

    println!("IVFFlat speedup: {speedup:.1}x (target: 5-50x)");
    assert!(
        speedup >= 5.0,
        "IVFFlat speedup {speedup:.1}x below target 5x",
    );
}

fn bench_index_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_selection");

    let test_cases = vec![
        ("small_dataset", 1_000, 128, QueryFrequency::High, 0.99),
        ("medium_dataset", 50_000, 128, QueryFrequency::Medium, 0.95),
        ("large_dataset", 500_000, 128, QueryFrequency::High, 0.98),
        ("high_dims", 100_000, 512, QueryFrequency::Medium, 0.94),
        ("low_recall", 100_000, 128, QueryFrequency::Low, 0.85),
    ];

    for (name, total_vectors, dimensions, query_freq, recall) in test_cases {
        group.bench_function(name, |b| {
            b.iter(|| {
                black_box(select_vector_index_type(
                    total_vectors,
                    dimensions,
                    query_freq,
                    recall,
                ))
            });
        });
    }

    group.finish();
}

fn bench_filter_optimization(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter_optimization");

    group.bench_function("optimize_filter_order", |b| {
        b.iter(|| {
            black_box(optimize_vector_filter_order(
                0.8,  // non-vector selectivity
                0.01, // non-vector cost
                0.1,  // vector selectivity
                VectorIndexType::HNSW,
                100_000, // total rows
            ))
        });
    });

    group.bench_function("estimate_query_cost_prefilter", |b| {
        let params = VectorIndexParams::default();
        b.iter(|| {
            black_box(estimate_vector_query_cost(
                128,
                100_000,
                VectorMetric::L2,
                0.01, // vector selectivity
                0.90, // non-vector selectivity (high)
                0.001,
                VectorIndexType::HNSW,
                params,
            ))
        });
    });

    group.bench_function("estimate_query_cost_postfilter", |b| {
        let params = VectorIndexParams::default();
        b.iter(|| {
            black_box(estimate_vector_query_cost(
                128,
                100_000,
                VectorMetric::L2,
                0.01, // vector selectivity
                0.05, // non-vector selectivity (low)
                0.01,
                VectorIndexType::HNSW,
                params,
            ))
        });
    });

    group.finish();
}

fn bench_dimension_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("dimension_scaling");

    let total_vectors = 100_000;
    let m = 16;
    let ef_search = 40;
    let k = 10;

    for dimensions in [64, 128, 256, 512, 1024] {
        group.bench_with_input(
            BenchmarkId::new("hnsw", dimensions),
            &dimensions,
            |b, &dims| {
                b.iter(|| {
                    black_box(hnsw_search_cost(
                        dims,
                        m,
                        ef_search,
                        total_vectors,
                        k,
                        VectorMetric::L2,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sequential", dimensions),
            &dimensions,
            |b, &dims| {
                b.iter(|| {
                    black_box(vector_sequential_scan_cost(
                        dims,
                        total_vectors,
                        VectorMetric::L2,
                    ))
                });
            },
        );
    }

    group.finish();
}

fn bench_metric_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("metric_comparison");

    let dimensions = 128;
    let total_vectors = 100_000;
    let ef_search = 40;
    let k = 10;

    for metric in [
        VectorMetric::L2,
        VectorMetric::InnerProduct,
        VectorMetric::Cosine,
    ] {
        let metric_name = match metric {
            VectorMetric::L2 => "l2",
            VectorMetric::InnerProduct => "inner_product",
            VectorMetric::Cosine => "cosine",
        };

        group.bench_with_input(BenchmarkId::new("hnsw", metric_name), &metric, |b, &m| {
            b.iter(|| {
                black_box(hnsw_search_cost(
                    dimensions,
                    16,
                    ef_search,
                    total_vectors,
                    k,
                    m,
                ))
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_distance_costs,
    bench_hnsw_scaling,
    bench_ivfflat_scaling,
    bench_sequential_vs_hnsw,
    bench_sequential_vs_ivfflat,
    bench_index_selection,
    bench_filter_optimization,
    bench_dimension_scaling,
    bench_metric_comparison,
);

criterion_main!(benches);
