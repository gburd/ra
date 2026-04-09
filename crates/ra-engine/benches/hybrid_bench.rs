//! Benchmark for hybrid search optimization.
//!
//! Measures performance of:
//! - Strategy selection (cost-based)
//! - Score fusion methods (weighted average, RRF, learned)
//! - Hybrid scan cost estimation
//!
//! Target: < 2x overhead vs single-modality search

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ra_engine::{
    HybridStrategy, ScoreFusion, choose_hybrid_strategy, fuse_scores,
    hybrid_scan_cost_factor,
};

fn bench_strategy_selection(c: &mut Criterion) {
    c.bench_function("choose_hybrid_strategy_fts_selective", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.001), // Highly selective FTS
                black_box(0.1),   // Less selective vector
                black_box(None),
                black_box(1_000_000.0),
            )
        });
    });

    c.bench_function("choose_hybrid_strategy_vector_selective", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.1),   // Less selective FTS
                black_box(0.001), // Highly selective vector
                black_box(None),
                black_box(1_000_000.0),
            )
        });
    });

    c.bench_function("choose_hybrid_strategy_small_limit", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.05),
                black_box(0.08),
                black_box(Some(10)),
                black_box(1_000_000.0),
            )
        });
    });

    c.bench_function("choose_hybrid_strategy_cost_based", |b| {
        b.iter(|| {
            choose_hybrid_strategy(
                black_box(0.05),
                black_box(0.06),
                black_box(Some(500)),
                black_box(1_000_000.0),
            )
        });
    });
}

fn bench_score_fusion(c: &mut Criterion) {
    let bm25_score = 10.0;
    let vector_score = 0.5;
    let alpha = 0.7;
    let k = 60;

    c.bench_function("fuse_scores_weighted_average", |b| {
        b.iter(|| {
            fuse_scores(
                black_box(bm25_score),
                black_box(vector_score),
                ScoreFusion::WeightedAverage,
                black_box(alpha),
                black_box(k),
            )
        });
    });

    c.bench_function("fuse_scores_rrf", |b| {
        b.iter(|| {
            fuse_scores(
                black_box(bm25_score),
                black_box(vector_score),
                ScoreFusion::ReciprocalRankFusion,
                black_box(alpha),
                black_box(k),
            )
        });
    });

    c.bench_function("fuse_scores_learned", |b| {
        b.iter(|| {
            fuse_scores(
                black_box(bm25_score),
                black_box(vector_score),
                ScoreFusion::Learned,
                black_box(alpha),
                black_box(k),
            )
        });
    });
}

fn bench_cost_estimation(c: &mut Criterion) {
    c.bench_function("hybrid_scan_cost_factor_fts_first", |b| {
        b.iter(|| {
            hybrid_scan_cost_factor(
                HybridStrategy::FTSFirst,
                black_box(0.01),
                black_box(0.05),
            )
        });
    });

    c.bench_function("hybrid_scan_cost_factor_vector_first", |b| {
        b.iter(|| {
            hybrid_scan_cost_factor(
                HybridStrategy::VectorFirst,
                black_box(0.05),
                black_box(0.01),
            )
        });
    });

    c.bench_function("hybrid_scan_cost_factor_parallel", |b| {
        b.iter(|| {
            hybrid_scan_cost_factor(
                HybridStrategy::Parallel,
                black_box(0.05),
                black_box(0.05),
            )
        });
    });
}

criterion_group!(
    hybrid_benches,
    bench_strategy_selection,
    bench_score_fusion,
    bench_cost_estimation
);
criterion_main!(hybrid_benches);
