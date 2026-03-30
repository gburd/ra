//! Benchmarks comparing native and polyglot backends.

#![allow(clippy::unwrap_used)]

use criterion::{criterion_group, criterion_main, Criterion};
use ra_dialect::{Dialect, DialectTranslator, TranslationBackend};

fn benchmark_simple_select(c: &mut Criterion) {
    let sql = "SELECT id, name FROM users WHERE age > 18";

    c.bench_function("native_postgres_to_mysql_simple", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            Dialect::MySql,
            TranslationBackend::Native,
        );
        b.iter(|| {
            translator.translate(sql).unwrap();
        });
    });

    #[cfg(feature = "polyglot-backend")]
    c.bench_function("polyglot_postgres_to_mysql_simple", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            Dialect::MySql,
            TranslationBackend::Polyglot,
        );
        b.iter(|| {
            translator.translate(sql).unwrap();
        });
    });
}

fn benchmark_complex_query(c: &mut Criterion) {
    let sql = r"
        WITH recent_orders AS (
            SELECT
                o.id,
                o.user_id,
                o.total,
                ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY created_at DESC) as rn
            FROM orders o
            WHERE o.created_at > CURRENT_DATE - INTERVAL '30 days'
        )
        SELECT
            u.id,
            u.name,
            COALESCE(r.total, 0) as recent_total
        FROM users u
        LEFT JOIN recent_orders r ON u.id = r.user_id AND r.rn = 1
        WHERE u.status = 'active'
        ORDER BY recent_total DESC
        LIMIT 100
    ";

    c.bench_function("native_postgres_to_mysql_complex", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            Dialect::MySql,
            TranslationBackend::Native,
        );
        b.iter(|| {
            let _ = translator.translate(sql);
        });
    });

    #[cfg(feature = "polyglot-backend")]
    c.bench_function("polyglot_postgres_to_mysql_complex", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            Dialect::MySql,
            TranslationBackend::Polyglot,
        );
        b.iter(|| {
            translator.translate(sql).unwrap();
        });
    });
}

fn benchmark_function_translation(c: &mut Criterion) {
    let sql = "SELECT IFNULL(a, b), CONCAT(c, d), LENGTH(e) FROM table1";

    c.bench_function("native_mysql_to_postgres_functions", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::MySql,
            Dialect::PostgreSql,
            TranslationBackend::Native,
        );
        b.iter(|| {
            let _ = translator.translate(sql);
        });
    });

    #[cfg(feature = "polyglot-backend")]
    c.bench_function("polyglot_mysql_to_postgres_functions", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::MySql,
            Dialect::PostgreSql,
            TranslationBackend::Polyglot,
        );
        b.iter(|| {
            translator.translate(sql).unwrap();
        });
    });
}

#[cfg(feature = "polyglot-backend")]
fn benchmark_extended_dialects(c: &mut Criterion) {
    let sql = "SELECT * FROM users WHERE age BETWEEN 18 AND 65 ORDER BY name LIMIT 10";

    c.bench_function("polyglot_postgres_to_bigquery", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            Dialect::BigQuery,
            TranslationBackend::Polyglot,
        );
        b.iter(|| {
            translator.translate(sql).unwrap();
        });
    });

    c.bench_function("polyglot_postgres_to_snowflake", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            Dialect::Snowflake,
            TranslationBackend::Polyglot,
        );
        b.iter(|| {
            translator.translate(sql).unwrap();
        });
    });

    c.bench_function("polyglot_postgres_to_clickhouse", |b| {
        let translator = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            Dialect::ClickHouse,
            TranslationBackend::Polyglot,
        );
        b.iter(|| {
            translator.translate(sql).unwrap();
        });
    });
}

criterion_group!(
    benches,
    benchmark_simple_select,
    benchmark_complex_query,
    benchmark_function_translation,
);

#[cfg(feature = "polyglot-backend")]
criterion_group!(extended_benches, benchmark_extended_dialects);

#[cfg(not(feature = "polyglot-backend"))]
criterion_main!(benches);

#[cfg(feature = "polyglot-backend")]
criterion_main!(benches, extended_benches);