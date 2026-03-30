//! Benchmark for lazy rule compilation.
//!
//! Measures the performance impact of lazy rule loading for queries of varying complexity.

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{LazyQueryPattern, LazyRuleCompiler};
use ra_engine::rewrite::all_rules;

/// Create a simple single-table query
fn simple_query() -> RelExpr {
    RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef {
                table: None,
                column: "x".to_string(),
            })),
            right: Box::new(Expr::Const(Const::Int(10))),
        },
        input: Box::new(RelExpr::Scan {
            table: "t".to_string(),
            alias: None,
        }),
    }
}

/// Create a two-way join query
fn join_query() -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef {
                table: Some("t1".to_string()),
                column: "id".to_string(),
            })),
            right: Box::new(Expr::Column(ColumnRef {
                table: Some("t2".to_string()),
                column: "id".to_string(),
            })),
        },
        left: Box::new(RelExpr::Scan {
            table: "t1".to_string(),
            alias: None,
        }),
        right: Box::new(RelExpr::Scan {
            table: "t2".to_string(),
            alias: None,
        }),
    }
}

/// Create a complex query with nested joins
fn complex_query() -> RelExpr {
    let t1 = RelExpr::Scan {
        table: "t1".to_string(),
        alias: None,
    };
    let t2 = RelExpr::Scan {
        table: "t2".to_string(),
        alias: None,
    };
    let t3 = RelExpr::Scan {
        table: "t3".to_string(),
        alias: None,
    };
    let t4 = RelExpr::Scan {
        table: "t4".to_string(),
        alias: None,
    };

    let join12 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let join34 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(t3),
        right: Box::new(t4),
    };

    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(join12),
        right: Box::new(join34),
    }
}

/// Benchmark rule compilation: lazy vs. all rules
fn rule_compilation_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_compilation");

    // Simple query
    let simple = simple_query();
    let simple_pattern = LazyQueryPattern::analyze(&simple);
    let compiler = LazyRuleCompiler::new();

    group.bench_function("simple_all_rules", |b| {
        b.iter(|| {
            let rules = all_rules();
            black_box(rules.len())
        });
    });

    group.bench_function("simple_lazy_rules", |b| {
        b.iter(|| {
            let rules = compiler.compile(&simple_pattern);
            black_box(rules.len())
        });
    });

    // Join query
    let join = join_query();
    let join_pattern = LazyQueryPattern::analyze(&join);

    group.bench_function("join_all_rules", |b| {
        b.iter(|| {
            let rules = all_rules();
            black_box(rules.len())
        });
    });

    group.bench_function("join_lazy_rules", |b| {
        b.iter(|| {
            let rules = compiler.compile(&join_pattern);
            black_box(rules.len())
        });
    });

    // Complex query
    let complex = complex_query();
    let complex_pattern = LazyQueryPattern::analyze(&complex);

    group.bench_function("complex_all_rules", |b| {
        b.iter(|| {
            let rules = all_rules();
            black_box(rules.len())
        });
    });

    group.bench_function("complex_lazy_rules", |b| {
        b.iter(|| {
            let rules = compiler.compile(&complex_pattern);
            black_box(rules.len())
        });
    });

    group.finish();
}

/// Benchmark query pattern analysis
fn pattern_analysis_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_analysis");

    for (name, query) in [
        ("simple", simple_query()),
        ("join", join_query()),
        ("complex", complex_query()),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &query, |b, q| {
            b.iter(|| {
                let pattern = LazyQueryPattern::analyze(q);
                black_box(pattern)
            });
        });
    }

    group.finish();
}

/// Benchmark rule count reduction
fn rule_count_benchmark(c: &mut Criterion) {
    let compiler = LazyRuleCompiler::new();
    let all = all_rules();

    let queries = [
        ("simple", simple_query()),
        ("join", join_query()),
        ("complex", complex_query()),
    ];

    for (name, query) in &queries {
        let pattern = LazyQueryPattern::analyze(query);
        let lazy_rules = compiler.compile(&pattern);
        let reduction = ((all.len() - lazy_rules.len()) as f64 / all.len() as f64) * 100.0;

        println!(
            "{}: {} rules (all) → {} rules (lazy) = {:.1}% reduction",
            name,
            all.len(),
            lazy_rules.len(),
            reduction
        );
    }
}

criterion_group!(
    benches,
    rule_compilation_benchmark,
    pattern_analysis_benchmark,
);
criterion_main!(benches);
