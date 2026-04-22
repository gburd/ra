//! Integration benchmarks for hybrid search.
//!
//! Compares:
//! - Hybrid search vs pure FTS
//! - Hybrid search vs pure vector
//! - Strategy selection overhead
//! - Score fusion methods

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ra_engine::{
    choose_hybrid_strategy, fuse_scores, hybrid_scan_cost_factor, HybridStrategy, ScoreFusion,
};

/// Benchmark hybrid search vs pure FTS.
fn bench_hybrid_vs_fts(c: &mut Criterion) {
    let mut group = c.benchmark_group("hybrid_vs_fts");

    // Pure FTS simulation: only compute BM25 scores
    group.bench_function("pure_fts", |b| {
        b.iter(|| {
            let bm25_score = black_box(10.0);
            // Normalize
            let normalized = bm25_score / (bm25_score + 1.0);
            black_box(normalized)
        });
    });

    // Hybrid search: compute both BM25 and vector, then fuse
    group.bench_function("hybrid_weighted", |b| {
        b.iter(|| {
            let bm25_score = black_box(10.0);
            let vector_score = black_box(0.5);
            let fused = fuse_scores(
                bm25_score,
                vector_score,
                ScoreFusion::WeightedAverage,
                black_box(0.7),
                black_box(60),
            );
            black_box(fused)
        });
    });

    // Hybrid with RRF
    group.bench_function("hybrid_rrf", |b| {
        b.iter(|| {
            let bm25_score = black_box(10.0);
            let vector_score = black_box(0.5);
            let fused = fuse_scores(
                bm25_score,
                vector_score,
                ScoreFusion::ReciprocalRankFusion,
                black_box(0.5),
                black_box(60),
            );
            black_box(fused)
        });
    });

    group.finish();
}

/// Benchmark hybrid search vs pure vector.
fn bench_hybrid_vs_vector(c: &mut Criterion) {
    let mut group = c.benchmark_group("hybrid_vs_vector");

    // Pure vector simulation: only compute vector distance
    group.bench_function("pure_vector", |b| {
        b.iter(|| {
            let vector_dist = black_box(0.5);
            // Normalize to similarity
            let similarity = 1.0 / (1.0 + vector_dist);
            black_box(similarity)
        });
    });

    // Hybrid search with both modalities
    group.bench_function("hybrid_weighted", |b| {
        b.iter(|| {
            let bm25_score = black_box(10.0);
            let vector_score = black_box(0.5);
            let fused = fuse_scores(
                bm25_score,
                vector_score,
                ScoreFusion::WeightedAverage,
                black_box(0.3),
                black_box(60),
            );
            black_box(fused)
        });
    });

    group.finish();
}

/// Benchmark strategy selection overhead.
fn bench_strategy_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_selection");

    // FTS-first scenario
    group.bench_function("fts_first", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.005), // Highly selective FTS
                black_box(0.1),
                black_box(None),
                black_box(1_000_000.0),
            )
        });
    });

    // Vector-first scenario
    group.bench_function("vector_first", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.1),
                black_box(0.003), // Highly selective vector
                black_box(None),
                black_box(1_000_000.0),
            )
        });
    });

    // Parallel scenario
    group.bench_function("parallel", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.05),
                black_box(0.08),
                black_box(Some(10)), // Small limit
                black_box(1_000_000.0),
            )
        });
    });

    // Cost-based decision
    group.bench_function("cost_based", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.05),
                black_box(0.06),
                black_box(Some(500)),
                black_box(1_000_000.0),
            )
        });
    });

    group.finish();
}

/// Benchmark score fusion methods.
fn bench_score_fusion_methods(c: &mut Criterion) {
    let mut group = c.benchmark_group("score_fusion");

    let bm25 = 10.0;
    let vector = 0.5;

    group.bench_function("weighted_average", |b| {
        b.iter(|| {
            fuse_scores(
                black_box(bm25),
                black_box(vector),
                ScoreFusion::WeightedAverage,
                black_box(0.7),
                black_box(60),
            )
        });
    });

    group.bench_function("rrf", |b| {
        b.iter(|| {
            fuse_scores(
                black_box(bm25),
                black_box(vector),
                ScoreFusion::ReciprocalRankFusion,
                black_box(0.5),
                black_box(60),
            )
        });
    });

    group.bench_function("learned", |b| {
        b.iter(|| {
            fuse_scores(
                black_box(bm25),
                black_box(vector),
                ScoreFusion::Learned,
                black_box(0.5),
                black_box(60),
            )
        });
    });

    group.finish();
}

/// Benchmark different alpha weights.
fn bench_alpha_weights(c: &mut Criterion) {
    let mut group = c.benchmark_group("alpha_weights");

    let bm25 = 10.0;
    let vector = 0.5;

    for alpha in [0.1, 0.3, 0.5, 0.7, 0.9] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("alpha_{alpha}")),
            &alpha,
            |b, &alpha| {
                b.iter(|| {
                    fuse_scores(
                        black_box(bm25),
                        black_box(vector),
                        ScoreFusion::WeightedAverage,
                        black_box(alpha),
                        black_box(60),
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmark cost estimation.
fn bench_cost_estimation(c: &mut Criterion) {
    let mut group = c.benchmark_group("cost_estimation");

    group.bench_function("fts_first", |b| {
        b.iter(|| {
            hybrid_scan_cost_factor(HybridStrategy::FTSFirst, black_box(0.01), black_box(0.05))
        });
    });

    group.bench_function("vector_first", |b| {
        b.iter(|| {
            hybrid_scan_cost_factor(
                HybridStrategy::VectorFirst,
                black_box(0.05),
                black_box(0.01),
            )
        });
    });

    group.bench_function("parallel", |b| {
        b.iter(|| {
            hybrid_scan_cost_factor(HybridStrategy::Parallel, black_box(0.05), black_box(0.05))
        });
    });

    group.finish();
}

/// Benchmark with varying selectivities.
fn bench_selectivity_impact(c: &mut Criterion) {
    let mut group = c.benchmark_group("selectivity_impact");

    for selectivity in [0.001, 0.01, 0.05, 0.1, 0.5] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("sel_{selectivity}")),
            &selectivity,
            |b, &sel| {
                b.iter(|| {
                    choose_hybrid_strategy(
                        black_box(sel),
                        black_box(sel),
                        black_box(Some(10)),
                        black_box(1_000_000.0),
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmark with varying table sizes.
fn bench_table_size_impact(c: &mut Criterion) {
    let mut group = c.benchmark_group("table_size_impact");

    for size in [1_000.0, 10_000.0, 100_000.0, 1_000_000.0, 10_000_000.0] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("size_{}", size as i64)),
            &size,
            |b, &size| {
                b.iter(|| {
                    choose_hybrid_strategy(
                        black_box(0.05),
                        black_box(0.05),
                        black_box(Some(10)),
                        black_box(size),
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmark with varying result limits.
fn bench_limit_impact(c: &mut Criterion) {
    let mut group = c.benchmark_group("limit_impact");

    for limit in [1, 5, 10, 50, 100, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("limit_{limit}")),
            &limit,
            |b, &lim| {
                b.iter(|| {
                    choose_hybrid_strategy(
                        black_box(0.05),
                        black_box(0.08),
                        black_box(Some(lim)),
                        black_box(1_000_000.0),
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmark parallel execution overhead.
fn bench_parallel_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_overhead");

    // Sequential: FTS then vector
    group.bench_function("sequential_fts_first", |b| {
        b.iter(|| {
            let fts_score = black_box(10.0);
            let vector_score = black_box(0.5);
            // FTS first, then vector on filtered results
            let normalized_fts = fts_score / (fts_score + 1.0);
            let normalized_vec = 1.0 / (1.0 + vector_score);
            let combined = 0.7 * normalized_fts + 0.3 * normalized_vec;
            black_box(combined)
        });
    });

    // Parallel: both independently, then merge
    group.bench_function("parallel_with_merge", |b| {
        b.iter(|| {
            // Simulate parallel execution
            let fts_score = black_box(10.0);
            let vector_score = black_box(0.5);

            // Both computed independently
            let normalized_fts = fts_score / (fts_score + 1.0);
            let normalized_vec = 1.0 / (1.0 + vector_score);

            // Merge step
            let combined = 0.7 * normalized_fts + 0.3 * normalized_vec;
            black_box(combined)
        });
    });

    group.finish();
}

/// Benchmark strategy comparison for realistic scenarios.
fn bench_realistic_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_scenarios");

    // Scenario 1: News article search (1M articles)
    group.bench_function("news_search_fts_selective", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.002), // 0.2% match "machine learning"
                black_box(0.01),  // 1% similar embeddings
                black_box(Some(20)),
                black_box(1_000_000.0),
            )
        });
    });

    // Scenario 2: Product catalog (500K products)
    group.bench_function("product_search_vector_selective", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.05),  // 5% match category
                black_box(0.005), // 0.5% similar features
                black_box(Some(50)),
                black_box(500_000.0),
            )
        });
    });

    // Scenario 3: Document search with small result set
    group.bench_function("document_search_small_limit", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.02), // 2% FTS match
                black_box(0.03), // 3% vector match
                black_box(Some(10)),
                black_box(1_000_000.0),
            )
        });
    });

    group.finish();
}

criterion_group!(
    hybrid_integration_benches,
    bench_hybrid_vs_fts,
    bench_hybrid_vs_vector,
    bench_strategy_selection,
    bench_score_fusion_methods,
    bench_alpha_weights,
    bench_cost_estimation,
    bench_selectivity_impact,
    bench_table_size_impact,
    bench_limit_impact,
    bench_parallel_overhead,
    bench_realistic_scenarios,
);

criterion_main!(hybrid_integration_benches);
