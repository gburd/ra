//! Benchmarks comparing pure-Rust vs SIMD-accelerated tokenization.
//!
//! Measures tokenization throughput for various SQL sizes and
//! complexity levels to validate the lime SIMD tokenizer integration.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use ra_parser::lime_parser::lexer;
use ra_parser::lime_parser::lime_tokenizer;

/// Small query (~50 bytes).
const SMALL_SQL: &str = "SELECT id, name FROM users WHERE age > 21";

/// Medium query (~200 bytes).
const MEDIUM_SQL: &str = "\
SELECT u.id, u.name, o.total, o.created_at \
FROM users u \
INNER JOIN orders o ON u.id = o.user_id \
WHERE u.active = TRUE AND o.total > 100 \
ORDER BY o.created_at DESC \
LIMIT 50 OFFSET 10";

/// Large query (~1KB).
const LARGE_SQL: &str = "\
WITH active_users AS (\
    SELECT id, name, email, department \
    FROM users \
    WHERE active = TRUE AND created_at > '2024-01-01'\
), \
dept_orders AS (\
    SELECT u.department, COUNT(*) AS order_count, \
           SUM(o.total) AS dept_total \
    FROM active_users u \
    INNER JOIN orders o ON u.id = o.user_id \
    WHERE o.status IN ('completed', 'shipped') \
    GROUP BY u.department \
    HAVING SUM(o.total) > 1000\
) \
SELECT au.id, au.name, au.department, \
       do.order_count, do.dept_total, \
       CASE WHEN do.dept_total > 10000 THEN 'high' \
            WHEN do.dept_total > 5000 THEN 'medium' \
            ELSE 'low' END AS tier, \
       CAST(do.dept_total AS FLOAT) / do.order_count AS avg_order \
FROM active_users au \
INNER JOIN dept_orders do ON au.department = do.department \
WHERE au.name LIKE '%smith%' OR au.name ILIKE '%jones%' \
ORDER BY do.dept_total DESC, au.name ASC \
LIMIT 100";

/// Generate a very large SQL string by repeating UNION ALL.
fn generate_xlarge_sql(n: usize) -> String {
    let mut parts = Vec::with_capacity(n);
    for i in 0..n {
        parts.push(format!(
            "SELECT {i} AS id, 'name_{i}' AS name, \
             {val}.{frac} AS value \
             FROM table_{i} WHERE col > {i}",
            i = i,
            val = i * 100,
            frac = i % 100,
        ));
    }
    parts.join(" UNION ALL ")
}

fn bench_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenize_small");
    group.throughput(Throughput::Bytes(SMALL_SQL.len() as u64));

    group.bench_function("rust", |b| {
        b.iter(|| lexer::tokenize(black_box(SMALL_SQL)));
    });
    group.bench_function("simd", |b| {
        b.iter(|| lime_tokenizer::tokenize_simd(black_box(SMALL_SQL)));
    });

    group.finish();
}

fn bench_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenize_medium");
    group.throughput(Throughput::Bytes(MEDIUM_SQL.len() as u64));

    group.bench_function("rust", |b| {
        b.iter(|| lexer::tokenize(black_box(MEDIUM_SQL)));
    });
    group.bench_function("simd", |b| {
        b.iter(|| lime_tokenizer::tokenize_simd(black_box(MEDIUM_SQL)));
    });

    group.finish();
}

fn bench_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenize_large");
    group.throughput(Throughput::Bytes(LARGE_SQL.len() as u64));

    group.bench_function("rust", |b| {
        b.iter(|| lexer::tokenize(black_box(LARGE_SQL)));
    });
    group.bench_function("simd", |b| {
        b.iter(|| lime_tokenizer::tokenize_simd(black_box(LARGE_SQL)));
    });

    group.finish();
}

fn bench_xlarge(c: &mut Criterion) {
    let sql = generate_xlarge_sql(100);
    let mut group = c.benchmark_group("tokenize_xlarge");
    group.throughput(Throughput::Bytes(sql.len() as u64));

    group.bench_function("rust", |b| {
        b.iter(|| lexer::tokenize(black_box(&sql)));
    });
    group.bench_function("simd", |b| {
        b.iter(|| lime_tokenizer::tokenize_simd(black_box(&sql)));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_small,
    bench_medium,
    bench_large,
    bench_xlarge,
);
criterion_main!(benches);
