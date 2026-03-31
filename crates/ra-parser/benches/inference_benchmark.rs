//! Benchmarks for dialect inference performance.
//!
//! These benchmarks ensure the inference engine maintains sub-millisecond
//! performance for typical SQL queries.

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use ra_parser::parser::inference::DialectInference;

/// Simple queries - should be very fast (< 10μs)
fn bench_simple_queries(c: &mut Criterion) {
    let queries = vec![
        ("PostgreSQL simple", "SELECT $1::int FROM users"),
        ("MySQL simple", "SELECT `id` FROM users"),
        ("Oracle simple", "SELECT * FROM DUAL"),
        ("SQL Server simple", "SELECT TOP 10 * FROM users"),
        ("Standard SQL", "SELECT id, name FROM users WHERE active = true"),
    ];

    let mut group = c.benchmark_group("simple_queries");
    for (name, sql) in queries {
        group.bench_with_input(BenchmarkId::from_parameter(name), sql, |b, sql| {
            b.iter(|| {
                let mut inference = DialectInference::new();
                inference.detect_from_tokens(black_box(sql));
                inference.detect_from_syntax(black_box(sql));
                inference.detect_from_functions(black_box(sql));
                inference.compute_scores()
            });
        });
    }
    group.finish();
}

/// Medium complexity queries (< 50μs)
fn bench_medium_queries(c: &mut Criterion) {
    let queries = vec![
        (
            "PostgreSQL CTE with JSONB",
            "WITH active_users AS (
                SELECT id, data->'name' as name
                FROM users
                WHERE data @> '{\"status\": \"active\"}'::jsonb
            )
            SELECT * FROM active_users ORDER BY name LIMIT 10",
        ),
        (
            "MySQL JOIN with GROUP_CONCAT",
            "SELECT u.id, u.`name`, GROUP_CONCAT(o.item SEPARATOR ', ') as items
            FROM `users` u
            INNER JOIN `orders` o ON u.id = o.user_id
            GROUP BY u.id, u.`name`
            LIMIT 20, 10",
        ),
        (
            "Oracle hierarchical query",
            "SELECT employee_id, manager_id, LEVEL, NVL(name, 'Unknown')
            FROM employees
            START WITH manager_id IS NULL
            CONNECT BY PRIOR employee_id = manager_id
            ORDER SIBLINGS BY name",
        ),
    ];

    let mut group = c.benchmark_group("medium_queries");
    for (name, sql) in queries {
        group.bench_with_input(BenchmarkId::from_parameter(name), sql, |b, sql| {
            b.iter(|| {
                let mut inference = DialectInference::new();
                inference.detect_from_tokens(black_box(sql));
                inference.detect_from_syntax(black_box(sql));
                inference.detect_from_functions(black_box(sql));
                inference.compute_scores()
            });
        });
    }
    group.finish();
}

/// Complex queries with multiple features (< 100μs)
fn bench_complex_queries(c: &mut Criterion) {
    let queries = vec![
        (
            "PostgreSQL complex analytics",
            "WITH RECURSIVE category_tree AS (
                SELECT id, parent_id, name, 1 as level, ARRAY[id] as path
                FROM categories
                WHERE parent_id IS NULL
                UNION ALL
                SELECT c.id, c.parent_id, c.name, ct.level + 1,
                       ct.path || c.id
                FROM categories c
                JOIN category_tree ct ON c.parent_id = ct.id
            ),
            sales_by_category AS (
                SELECT
                    ct.name,
                    ct.level,
                    SUM(s.amount)::numeric(15,2) as total_sales,
                    COUNT(DISTINCT s.customer_id) as unique_customers,
                    jsonb_agg(jsonb_build_object(
                        'product', p.name,
                        'sales', s.amount
                    )) as products
                FROM category_tree ct
                JOIN products p ON p.category_id = ct.id
                JOIN sales s ON s.product_id = p.id
                WHERE s.sale_date >= CURRENT_DATE - INTERVAL '30 days'
                GROUP BY ct.id, ct.name, ct.level
            )
            SELECT name, level, total_sales, unique_customers,
                   ROW_NUMBER() OVER (PARTITION BY level ORDER BY total_sales DESC) as rank_in_level
            FROM sales_by_category
            WHERE total_sales > 1000
            ORDER BY level, total_sales DESC",
        ),
    ];

    let mut group = c.benchmark_group("complex_queries");
    for (name, sql) in queries {
        group.bench_with_input(BenchmarkId::from_parameter(name), sql, |b, sql| {
            b.iter(|| {
                let mut inference = DialectInference::new();
                inference.detect_from_tokens(black_box(sql));
                inference.detect_from_syntax(black_box(sql));
                inference.detect_from_functions(black_box(sql));
                inference.compute_scores()
            });
        });
    }
    group.finish();
}

/// Benchmark inference accuracy on real-world queries
fn bench_accuracy_corpus(c: &mut Criterion) {
    // Mix of queries from different dialects
    let corpus = vec![
        "SELECT * FROM users WHERE id = $1",
        "SELECT `id`, `name` FROM users WHERE active = 1",
        "SELECT employee_id FROM employees CONNECT BY PRIOR manager_id = employee_id",
        "SELECT TOP 100 * FROM orders WITH (NOLOCK)",
        "SELECT ARRAY[1,2,3]::int[] as numbers",
        "INSERT INTO users (name) VALUES ('Alice') RETURNING id",
        "SELECT GROUP_CONCAT(name SEPARATOR ', ') FROM users",
        "SELECT * FROM DUAL",
        "WITH data AS (SELECT * FROM users) SELECT * FROM data",
        "SELECT name <-> 'John' FROM users ORDER BY name <-> 'John' LIMIT 10",
    ];

    c.bench_function("corpus_inference", |b| {
        b.iter(|| {
            for sql in &corpus {
                let mut inference = DialectInference::new();
                inference.detect_from_tokens(black_box(sql));
                inference.detect_from_syntax(black_box(sql));
                inference.detect_from_functions(black_box(sql));
                let _ = inference.compute_scores();
            }
        });
    });
}

criterion_group!(
    benches,
    bench_simple_queries,
    bench_medium_queries,
    bench_complex_queries,
    bench_accuracy_corpus
);
criterion_main!(benches);
