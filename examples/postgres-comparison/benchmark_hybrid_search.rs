//! Benchmark hybrid search: native PostgreSQL vs Ra optimization.
//!
//! This example compares hybrid search performance combining vector similarity
//! with keyword search across different parameter configurations.
//!
//! Run with: cargo run --example benchmark_hybrid_search --features postgres

use ra_adapters::{compare_queries, PostgresAdapter};
use std::env;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let db_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/benchmark_db".to_string());

    println!("Connecting to PostgreSQL at: {db_url}");

    let mut adapter = PostgresAdapter::new();
    adapter.connect(&db_url)?;

    println!("Checking extensions...");
    let extensions = adapter.check_extensions()?;
    println!("  pgvector: {}", extensions.get("pgvector").unwrap_or(&false));
    println!("  pg_trgm: {}", extensions.get("pg_trgm").unwrap_or(&false));
    println!("  rum: {}", extensions.get("rum").unwrap_or(&false));

    println!("\nRunning hybrid search benchmarks...\n");

    let queries = vec![
        // Basic hybrid search combining vector similarity with keyword match
        "SELECT id, title, content, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'machine learning')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            plainto_tsquery('english', 'machine learning') \
        ORDER BY (vec_dist * 0.7 + (1 - rank) * 0.3) \
        LIMIT 10"
            .to_string(),
        // Hybrid search with metadata filtering
        "SELECT id, title, content, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'neural networks')) AS rank \
        FROM documents \
        WHERE category = 'research' \
            AND created_at > NOW() - INTERVAL '1 year' \
            AND to_tsvector('english', content) @@ \
                plainto_tsquery('english', 'neural networks') \
        ORDER BY (vec_dist * 0.6 + (1 - rank) * 0.4) \
        LIMIT 20"
            .to_string(),
        // Multi-field hybrid search with different weights
        "SELECT id, title, content, \
            (embedding <-> '[0.1, 0.2, 0.3]'::vector) AS vec_dist, \
            ts_rank(to_tsvector('english', title || ' ' || content), \
                plainto_tsquery('english', 'deep learning')) AS rank, \
            author_reputation * 0.1 AS authority \
        FROM documents \
        WHERE to_tsvector('english', title || ' ' || content) @@ \
            plainto_tsquery('english', 'deep learning') \
        ORDER BY (vec_dist * 0.5 + (1 - rank) * 0.3 + (1 - authority) * 0.2) \
        LIMIT 15"
            .to_string(),
        // Hybrid search with range filtering on similarity
        "SELECT id, title, content, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'artificial intelligence')) AS rank \
        FROM documents \
        WHERE (embedding <-> '[0.1, 0.2, 0.3]'::vector) < 0.5 \
            AND to_tsvector('english', content) @@ \
                plainto_tsquery('english', 'artificial intelligence') \
        ORDER BY (vec_dist * 0.7 + (1 - rank) * 0.3) \
        LIMIT 25"
            .to_string(),
        // Hybrid search with phrase matching
        "SELECT id, title, content, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            ts_rank(to_tsvector('english', content), \
                phraseto_tsquery('english', 'natural language processing')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            phraseto_tsquery('english', 'natural language processing') \
        ORDER BY (vec_dist * 0.8 + (1 - rank) * 0.2) \
        LIMIT 10"
            .to_string(),
        // Hybrid search with multiple query terms
        "SELECT id, title, content, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            ts_rank(to_tsvector('english', content), \
                to_tsquery('english', 'transformer & attention & mechanism')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            to_tsquery('english', 'transformer & attention & mechanism') \
        ORDER BY (vec_dist * 0.6 + (1 - rank) * 0.4) \
        LIMIT 20"
            .to_string(),
        // Hybrid search with boosting on specific fields
        "SELECT id, title, content, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            setweight(to_tsvector('english', title), 'A') || \
            setweight(to_tsvector('english', content), 'B') AS document, \
            ts_rank(setweight(to_tsvector('english', title), 'A') || \
                setweight(to_tsvector('english', content), 'B'), \
                plainto_tsquery('english', 'computer vision')) AS rank \
        FROM documents \
        WHERE (setweight(to_tsvector('english', title), 'A') || \
            setweight(to_tsvector('english', content), 'B')) @@ \
            plainto_tsquery('english', 'computer vision') \
        ORDER BY (vec_dist * 0.5 + (1 - rank) * 0.5) \
        LIMIT 15"
            .to_string(),
        // Hybrid search with similarity threshold and recency bias
        "SELECT id, title, content, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'reinforcement learning')) AS rank, \
            EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400 AS days_old \
        FROM documents \
        WHERE (embedding <-> '[0.1, 0.2, 0.3]'::vector) < 0.6 \
            AND to_tsvector('english', content) @@ \
                plainto_tsquery('english', 'reinforcement learning') \
            AND created_at > NOW() - INTERVAL '2 years' \
        ORDER BY (vec_dist * 0.5 + (1 - rank) * 0.3 + (days_old / 730) * 0.2) \
        LIMIT 20"
            .to_string(),
        // Hybrid search with category-specific embeddings
        "SELECT id, title, content, category, \
            CASE category \
                WHEN 'research' THEN embedding <-> '[0.1, 0.2, 0.3]'::vector \
                WHEN 'blog' THEN embedding <-> '[0.2, 0.3, 0.4]'::vector \
                ELSE embedding <-> '[0.3, 0.4, 0.5]'::vector \
            END AS vec_dist, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'data science')) AS rank \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            plainto_tsquery('english', 'data science') \
        ORDER BY (vec_dist * 0.7 + (1 - rank) * 0.3) \
        LIMIT 30"
            .to_string(),
        // Hybrid search with aggregated scoring
        "SELECT id, title, content, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'machine learning')) AS text_rank, \
            view_count / 1000.0 AS popularity, \
            (embedding <-> '[0.1, 0.2, 0.3]'::vector) * 0.4 + \
            (1 - ts_rank(to_tsvector('english', content), \
                plainto_tsquery('english', 'machine learning'))) * 0.4 + \
            (1 - LEAST(view_count / 1000.0, 1.0)) * 0.2 AS final_score \
        FROM documents \
        WHERE to_tsvector('english', content) @@ \
            plainto_tsquery('english', 'machine learning') \
        ORDER BY final_score \
        LIMIT 25"
            .to_string(),
    ];

    println!("Comparing {} hybrid search queries...\n", queries.len());

    let report = compare_queries(&adapter, &queries)?;

    println!("{}", report.to_markdown());

    let json_output = "hybrid_search_comparison.json";
    std::fs::write(json_output, report.to_json()?)?;
    println!("\nJSON report saved to: {json_output}");

    let md_output = "hybrid_search_comparison.md";
    std::fs::write(md_output, report.to_markdown())?;
    println!("Markdown report saved to: {md_output}");

    Ok(())
}
