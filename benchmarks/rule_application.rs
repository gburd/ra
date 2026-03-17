//! Benchmarks for rule application performance.
//!
//! Measures the cost of applying transformation rules to expression trees
//! of varying sizes. Run with:
//!
//!   cargo bench --bench rule_application
//!
//! Results are written to `target/criterion/` as HTML reports.

use criterion::{criterion_group, criterion_main, Criterion};

fn bench_single_rule_application(c: &mut Criterion) {
    c.bench_function("single_rule_small_tree", |b| {
        b.iter(|| {
            // Placeholder: apply a single rule to a small expression tree
        });
    });
}

fn bench_rule_chain(c: &mut Criterion) {
    c.bench_function("rule_chain_10_rules", |b| {
        b.iter(|| {
            // Placeholder: apply a chain of 10 rules sequentially
        });
    });
}

fn bench_equality_saturation(c: &mut Criterion) {
    c.bench_function("equality_saturation_small", |b| {
        b.iter(|| {
            // Placeholder: run equality saturation on a small e-graph
        });
    });
}

criterion_group!(
    benches,
    bench_single_rule_application,
    bench_rule_chain,
    bench_equality_saturation,
);
criterion_main!(benches);
