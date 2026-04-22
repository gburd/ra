//! Benchmarks for full-text search cost models and optimizations.
//!
//! Compares GIN vs RUM vs FULLTEXT index performance for:
//! - Boolean queries (AND, OR, phrase)
//! - Ranked retrieval with various algorithms
//! - Top-K queries with different limits
//! - Skip-list intersection vs linear merge

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ra_engine::fts_cost::{
    boolean_query_cost, fulltext_scan_cost, gin_scan_cost, index_vs_seqscan_speedup,
    inverted_index_lookup_cost, rum_scan_cost, select_fts_index_type, skip_list_intersection_cost,
    top_k_ranking_cost, BooleanOperator, FtsIndexType, RankingAlgorithm,
};
use ra_engine::fts_rules::optimize_top_k_fts;

/// Benchmark inverted index lookup for single terms.
fn inverted_index_lookup_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("inverted_index_lookup");

    let total_docs = 1_000_000;
    let test_cases = vec![
        ("rare", 100),
        ("uncommon", 1_000),
        ("common", 10_000),
        ("very_common", 100_000),
    ];

    for (label, freq) in test_cases {
        group.bench_with_input(BenchmarkId::from_parameter(label), &freq, |b, &f| {
            b.iter(|| {
                let cost = inverted_index_lookup_cost(
                    black_box("search"),
                    black_box(total_docs),
                    black_box(f),
                );
                black_box(cost)
            });
        });
    }

    group.finish();
}

/// Benchmark skip-list intersection vs linear merge.
fn skip_list_intersection_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("skip_list_intersection");

    let test_cases = vec![
        ("small", 1_000, 1_000),
        ("medium", 10_000, 10_000),
        ("large", 100_000, 100_000),
        ("skewed_small_large", 1_000, 100_000),
        ("skewed_medium_large", 10_000, 100_000),
    ];

    for (label, size_a, size_b) in test_cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(size_a, size_b),
            |bencher, &(a, b)| {
                bencher.iter(|| {
                    let cost = skip_list_intersection_cost(black_box(a), black_box(b));
                    black_box(cost)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark boolean query costs for different operators.
fn boolean_query_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("boolean_query");

    let total_docs = 1_000_000;
    let terms_2 = vec!["rust", "language"];
    let freqs_2 = vec![10_000, 20_000];

    let terms_3 = vec!["full", "text", "search"];
    let freqs_3 = vec![5_000, 3_000, 2_000];

    let terms_5 = vec!["query", "optimization", "database", "index", "cost"];
    let freqs_5 = vec![15_000, 8_000, 25_000, 12_000, 6_000];

    for (label, terms, freqs) in [
        ("2_terms_and", terms_2.as_slice(), freqs_2.as_slice()),
        ("3_terms_and", terms_3.as_slice(), freqs_3.as_slice()),
        ("5_terms_and", terms_5.as_slice(), freqs_5.as_slice()),
    ] {
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(terms, freqs),
            |b, &(t, f)| {
                b.iter(|| {
                    let cost = boolean_query_cost(
                        black_box(t),
                        black_box(BooleanOperator::And),
                        black_box(total_docs),
                        black_box(f),
                    );
                    black_box(cost)
                });
            },
        );
    }

    for (label, terms, freqs) in [
        ("2_terms_phrase", terms_2.as_slice(), freqs_2.as_slice()),
        ("3_terms_phrase", terms_3.as_slice(), freqs_3.as_slice()),
    ] {
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(terms, freqs),
            |b, &(t, f)| {
                b.iter(|| {
                    let cost = boolean_query_cost(
                        black_box(t),
                        black_box(BooleanOperator::Phrase),
                        black_box(total_docs),
                        black_box(f),
                    );
                    black_box(cost)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark top-K ranking with different algorithms and limits.
fn top_k_ranking_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("top_k_ranking");

    let matching_docs = 100_000;
    let algorithms = vec![
        ("tf_idf", RankingAlgorithm::TfIdf),
        ("bm25", RankingAlgorithm::Bm25),
        ("cover_density", RankingAlgorithm::CoverDensity),
    ];

    let limits = vec![
        ("no_limit", None),
        ("limit_10", Some(10)),
        ("limit_100", Some(100)),
        ("limit_1000", Some(1000)),
    ];

    for (algo_label, algo) in &algorithms {
        for (limit_label, limit) in &limits {
            let label = format!("{}_{}", algo_label, limit_label);
            group.bench_with_input(
                BenchmarkId::from_parameter(&label),
                &(algo, limit),
                |b, &(&a, &l)| {
                    b.iter(|| {
                        let cost = top_k_ranking_cost(
                            black_box(matching_docs),
                            black_box(a),
                            black_box(l),
                        );
                        black_box(cost)
                    });
                },
            );
        }
    }

    group.finish();
}

/// Benchmark index type selection.
fn index_selection_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_selection");

    let test_cases = vec![
        ("small_table_boolean", BooleanOperator::And, false, 500),
        (
            "large_table_boolean",
            BooleanOperator::And,
            false,
            1_000_000,
        ),
        ("phrase_no_rank", BooleanOperator::Phrase, false, 100_000),
        ("phrase_ranked", BooleanOperator::Phrase, true, 100_000),
        ("ranked_large", BooleanOperator::And, true, 1_000_000),
    ];

    for (label, op, rank, size) in test_cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(op, rank, size),
            |b, &(o, r, s)| {
                b.iter(|| {
                    let idx = select_fts_index_type(black_box(o), black_box(r), black_box(s));
                    black_box(idx)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark speedup calculations.
fn speedup_calculation_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("speedup_calculation");

    let total_docs = 1_000_000;
    let test_cases = vec![
        ("gin_high_sel", FtsIndexType::Gin, 100),
        ("gin_medium_sel", FtsIndexType::Gin, 10_000),
        ("gin_low_sel", FtsIndexType::Gin, 200_000),
        ("rum_high_sel", FtsIndexType::Rum, 100),
        ("rum_medium_sel", FtsIndexType::Rum, 10_000),
        ("fulltext_high_sel", FtsIndexType::Fulltext, 100),
    ];

    for (label, idx_type, matching) in test_cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(idx_type, matching),
            |b, &(idx, m)| {
                b.iter(|| {
                    let speedup = index_vs_seqscan_speedup(
                        black_box(total_docs),
                        black_box(m),
                        black_box(idx),
                    );
                    black_box(speedup)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark GIN vs RUM vs FULLTEXT scan costs.
fn index_type_comparison_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_type_comparison");

    let total_docs = 1_000_000;
    let terms = vec!["search", "query"];
    let freqs = vec![10_000, 15_000];

    group.bench_function("gin_boolean_no_rank", |b| {
        b.iter(|| {
            let cost = gin_scan_cost(
                black_box(&terms),
                black_box(BooleanOperator::And),
                black_box(total_docs),
                black_box(&freqs),
                black_box(false),
                black_box(None),
            );
            black_box(cost)
        });
    });

    group.bench_function("gin_ranked_no_limit", |b| {
        b.iter(|| {
            let cost = gin_scan_cost(
                black_box(&terms),
                black_box(BooleanOperator::And),
                black_box(total_docs),
                black_box(&freqs),
                black_box(true),
                black_box(None),
            );
            black_box(cost)
        });
    });

    group.bench_function("gin_ranked_limit_10", |b| {
        b.iter(|| {
            let cost = gin_scan_cost(
                black_box(&terms),
                black_box(BooleanOperator::And),
                black_box(total_docs),
                black_box(&freqs),
                black_box(true),
                black_box(Some(10)),
            );
            black_box(cost)
        });
    });

    group.bench_function("rum_boolean_no_rank", |b| {
        b.iter(|| {
            let cost = rum_scan_cost(
                black_box(&terms),
                black_box(BooleanOperator::And),
                black_box(total_docs),
                black_box(&freqs),
                black_box(false),
                black_box(None),
            );
            black_box(cost)
        });
    });

    group.bench_function("rum_ranked_no_limit", |b| {
        b.iter(|| {
            let cost = rum_scan_cost(
                black_box(&terms),
                black_box(BooleanOperator::And),
                black_box(total_docs),
                black_box(&freqs),
                black_box(true),
                black_box(None),
            );
            black_box(cost)
        });
    });

    group.bench_function("rum_ranked_limit_10", |b| {
        b.iter(|| {
            let cost = rum_scan_cost(
                black_box(&terms),
                black_box(BooleanOperator::And),
                black_box(total_docs),
                black_box(&freqs),
                black_box(true),
                black_box(Some(10)),
            );
            black_box(cost)
        });
    });

    group.bench_function("fulltext_limit_10", |b| {
        b.iter(|| {
            let cost = fulltext_scan_cost(
                black_box(&terms),
                black_box(BooleanOperator::And),
                black_box(total_docs),
                black_box(&freqs),
                black_box(Some(10)),
            );
            black_box(cost)
        });
    });

    group.finish();
}

/// Benchmark optimization decision making.
fn optimization_decision_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("optimization_decision");

    let total_docs = 1_000_000;
    let terms = vec!["search"];
    let freqs = vec![10_000];

    group.bench_function("rum_with_limit", |b| {
        b.iter(|| {
            let decision = optimize_top_k_fts(
                black_box(true),
                black_box(false),
                black_box(Some(10)),
                black_box(&terms),
                black_box(total_docs),
                black_box(&freqs),
            );
            black_box(decision)
        });
    });

    group.bench_function("gin_fallback", |b| {
        b.iter(|| {
            let decision = optimize_top_k_fts(
                black_box(false),
                black_box(true),
                black_box(Some(10)),
                black_box(&terms),
                black_box(total_docs),
                black_box(&freqs),
            );
            black_box(decision)
        });
    });

    group.bench_function("no_index", |b| {
        b.iter(|| {
            let decision = optimize_top_k_fts(
                black_box(false),
                black_box(false),
                black_box(Some(10)),
                black_box(&terms),
                black_box(total_docs),
                black_box(&freqs),
            );
            black_box(decision)
        });
    });

    group.finish();
}

/// Benchmark top-K speedup demonstration.
fn top_k_speedup_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("top_k_speedup");

    let matching_docs = vec![1_000, 10_000, 100_000];
    let limits = vec![10, 100];

    for docs in &matching_docs {
        for limit in &limits {
            let label = format!("docs_{}_limit_{}", docs, limit);
            group.bench_with_input(
                BenchmarkId::from_parameter(&label),
                &(docs, limit),
                |b, &(&d, &l)| {
                    b.iter(|| {
                        let cost_no_limit = top_k_ranking_cost(
                            black_box(d),
                            black_box(RankingAlgorithm::Bm25),
                            black_box(None),
                        );
                        let cost_with_limit = top_k_ranking_cost(
                            black_box(d),
                            black_box(RankingAlgorithm::Bm25),
                            black_box(Some(l)),
                        );
                        black_box((cost_no_limit, cost_with_limit))
                    });
                },
            );
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    inverted_index_lookup_benchmark,
    skip_list_intersection_benchmark,
    boolean_query_benchmark,
    top_k_ranking_benchmark,
    index_selection_benchmark,
    speedup_calculation_benchmark,
    index_type_comparison_benchmark,
    optimization_decision_benchmark,
    top_k_speedup_comparison,
);
criterion_main!(benches);
