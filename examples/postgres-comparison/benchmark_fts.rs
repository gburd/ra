//! Benchmark full-text search: native PostgreSQL vs Ra optimization.
//!
//! This example compares full-text search performance using PostgreSQL's
//! built-in text search capabilities across different query types.
//!
//! Run with: cargo run --example benchmark_fts --features postgres

use ra_adapters::{compare_queries, PostgresAdapter};
use std::env;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let db_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/benchmark_db".to_string());

    println!("Connecting to PostgreSQL at: {db_url}");

    let mut adapter = PostgresAdapter::new();
    adapter.connect(&db_url)?;

    println!("Checking full-text search extensions...");
    let extensions = adapter.check_extensions()?;
    println!("  pg_trgm: {}", extensions.get("pg_trgm").unwrap_or(&false));
    println!("  rum: {}", extensions.get("rum").unwrap_or(&false));

    println!("\nRunning full-text search benchmarks...\n");

    let queries = vec![
        // Basic plainto_tsquery search
        "SELECT id, title, content, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'machine learning')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            plainto_tsquery('english', 'machine learning') \
        ORDER BY rank DESC \
        LIMIT 10"
            .to_string(),
        // Phrase search with phraseto_tsquery
        "SELECT id, title, content, \
            ts_rank(to_tsvector('english', content), \
                phraseto_tsquery('english', 'natural language processing')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            phraseto_tsquery('english', 'natural language processing') \
        ORDER BY rank DESC \
        LIMIT 15"
            .to_string(),
        // Boolean query with AND/OR operators
        "SELECT id, title, content, \
            ts_rank(to_tsvector('english', content), \
                to_tsquery('english', 'machine & learning | artificial & intelligence')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            to_tsquery('english', 'machine & learning | artificial & intelligence') \
        ORDER BY rank DESC \
        LIMIT 20"
            .to_string(),
        // Multi-field search with weighted ranking
        "SELECT id, title, content, \
            ts_rank(setweight(to_tsvector('english', title), 'A') || \
                setweight(to_tsvector('english', content), 'B'), \
                plainto_tsquery('english', 'deep learning')) AS rank \
        FROM documents \
        WHERE (setweight(to_tsvector('english', title), 'A') || \
            setweight(to_tsvector('english', content), 'B')) @@ \
            plainto_tsquery('english', 'deep learning') \
        ORDER BY rank DESC \
        LIMIT 10"
            .to_string(),
        // FTS with metadata filtering
        "SELECT id, title, content, category, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'neural networks')) AS rank \
        FROM documents \
        WHERE category = 'research' \
            AND created_at > NOW() - INTERVAL '1 year' \
            AND to_tsvector('english', content) @@ \
                plainto_tsquery('english', 'neural networks') \
        ORDER BY rank DESC \
        LIMIT 25"
            .to_string(),
        // Proximity search using <-> operator
        "SELECT id, title, content, \
            ts_rank(to_tsvector('english', content), \
                to_tsquery('english', 'data <-> science')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            to_tsquery('english', 'data <-> science') \
        ORDER BY rank DESC \
        LIMIT 15"
            .to_string(),
        // Negation search with NOT operator
        "SELECT id, title, content, \
            ts_rank(to_tsvector('english', content), \
                to_tsquery('english', 'machine & learning & !tutorial')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            to_tsquery('english', 'machine & learning & !tutorial') \
        ORDER BY rank DESC \
        LIMIT 20"
            .to_string(),
        // Prefix search with wildcard
        "SELECT id, title, content, \
            ts_rank(to_tsvector('english', content), \
                to_tsquery('english', 'transform:* | attention:*')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            to_tsquery('english', 'transform:* | attention:*') \
        ORDER BY rank DESC \
        LIMIT 10"
            .to_string(),
        // FTS with ts_rank_cd (cover density ranking)
        "SELECT id, title, content, \
            ts_rank_cd(to_tsvector('english', content), \
                plainto_tsquery('english', 'reinforcement learning')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            plainto_tsquery('english', 'reinforcement learning') \
        ORDER BY rank DESC \
        LIMIT 15"
            .to_string(),
        // FTS with custom normalization
        "SELECT id, title, content, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'computer vision'), 1) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            plainto_tsquery('english', 'computer vision') \
        ORDER BY rank DESC \
        LIMIT 20"
            .to_string(),
        // Multi-language search
        "SELECT id, title, content, language, \
            CASE language \
                WHEN 'english' THEN ts_rank(to_tsvector('english', content), \
                    plainto_tsquery('english', 'machine learning')) \
                WHEN 'spanish' THEN ts_rank(to_tsvector('spanish', content), \
                    plainto_tsquery('spanish', 'aprendizaje automático')) \
                ELSE 0 \
            END AS rank \
        FROM documents \
        WHERE (language = 'english' AND to_tsvector('english', content) @@ \
                plainto_tsquery('english', 'machine learning')) \
            OR (language = 'spanish' AND to_tsvector('spanish', content) @@ \
                plainto_tsquery('spanish', 'aprendizaje automático')) \
        ORDER BY rank DESC \
        LIMIT 10"
            .to_string(),
        // FTS with headline generation
        "SELECT id, title, \
            ts_headline('english', content, \
                plainto_tsquery('english', 'artificial intelligence'), \
                'MaxWords=50, MinWords=20') AS snippet, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'artificial intelligence')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            plainto_tsquery('english', 'artificial intelligence') \
        ORDER BY rank DESC \
        LIMIT 15"
            .to_string(),
        // FTS with similarity threshold using pg_trgm
        "SELECT id, title, content, \
            similarity(title, 'machine learning') AS sim \
        FROM documents \
        WHERE similarity(title, 'machine learning') > 0.3 \
        ORDER BY sim DESC \
        LIMIT 20"
            .to_string(),
        // Combined FTS and trigram search
        "SELECT id, title, content, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'deep learning')) * 0.7 + \
            similarity(title, 'deep learning') * 0.3 AS score \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
                plainto_tsquery('english', 'deep learning') \
            OR similarity(title, 'deep learning') > 0.2 \
        ORDER BY score DESC \
        LIMIT 25"
            .to_string(),
    ];

    println!("Comparing {} full-text search queries...\n", queries.len());

    let report = compare_queries(&adapter, &queries)?;

    println!("{}", report.to_markdown());

    let json_output = "fts_comparison.json";
    std::fs::write(json_output, report.to_json()?)?;
    println!("\nJSON report saved to: {json_output}");

    let md_output = "fts_comparison.md";
    std::fs::write(md_output, report.to_markdown())?;
    println!("Markdown report saved to: {md_output}");

    Ok(())
}
