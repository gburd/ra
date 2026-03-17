//! Benchmarks for .rra file parsing performance.
//!
//! Measures parsing throughput for rule files of varying sizes.
//! Run with:
//!
//!   cargo bench --bench parsing
//!
//! Results are written to `target/criterion/` as HTML reports.

use criterion::{criterion_group, criterion_main, Criterion};

fn bench_parse_simple_rule(c: &mut Criterion) {
    c.bench_function("parse_simple_rule", |b| {
        b.iter(|| {
            // Placeholder: parse a minimal .rra file
        });
    });
}

fn bench_parse_complex_rule(c: &mut Criterion) {
    c.bench_function("parse_complex_rule", |b| {
        b.iter(|| {
            // Placeholder: parse a fully-populated .rra file
        });
    });
}

fn bench_parse_directory(c: &mut Criterion) {
    c.bench_function("parse_100_rules", |b| {
        b.iter(|| {
            // Placeholder: parse a directory of 100 rule files
        });
    });
}

criterion_group!(
    benches,
    bench_parse_simple_rule,
    bench_parse_complex_rule,
    bench_parse_directory,
);
criterion_main!(benches);
