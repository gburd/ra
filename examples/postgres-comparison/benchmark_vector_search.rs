//! Benchmark vector search: native PostgreSQL vs Ra optimization.
//!
//! This example compares vector similarity search performance using pgvector
//! across different configurations and query patterns.
//!
//! Run with: cargo run --example benchmark_vector_search --features postgres

use ra_adapters::{compare_queries, PostgresAdapter};
use std::env;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let db_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/benchmark_db".to_string());

    println!("Connecting to PostgreSQL at: {db_url}");

    let mut adapter = PostgresAdapter::new();
    adapter.connect(&db_url)?;

    println!("Checking pgvector extension...");
    let extensions = adapter.check_extensions()?;
    if !extensions.get("pgvector").unwrap_or(&false) {
        eprintln!("Warning: pgvector extension not installed");
    }

    println!("\nRunning vector search benchmarks...\n");

    let queries = vec![
        // Basic cosine similarity search
        "SELECT id, title, embedding <=> '[0.1, 0.2, 0.3]'::vector AS distance \
        FROM documents \
        ORDER BY embedding <=> '[0.1, 0.2, 0.3]'::vector \
        LIMIT 10"
            .to_string(),
        // Euclidean distance search
        "SELECT id, title, embedding <-> '[0.1, 0.2, 0.3]'::vector AS distance \
        FROM documents \
        ORDER BY embedding <-> '[0.1, 0.2, 0.3]'::vector \
        LIMIT 10"
            .to_string(),
        // Inner product search
        "SELECT id, title, embedding <#> '[0.1, 0.2, 0.3]'::vector AS distance \
        FROM documents \
        ORDER BY embedding <#> '[0.1, 0.2, 0.3]'::vector \
        LIMIT 10"
            .to_string(),
        // Vector search with filtering
        "SELECT id, title, category, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS distance \
        FROM documents \
        WHERE category = 'research' \
        ORDER BY embedding <-> '[0.1, 0.2, 0.3]'::vector \
        LIMIT 20"
            .to_string(),
        // Vector search with date range filtering
        "SELECT id, title, created_at, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS distance \
        FROM documents \
        WHERE created_at > NOW() - INTERVAL '1 year' \
        ORDER BY embedding <-> '[0.1, 0.2, 0.3]'::vector \
        LIMIT 15"
            .to_string(),
        // Vector search with similarity threshold
        "SELECT id, title, embedding <-> '[0.1, 0.2, 0.3]'::vector AS distance \
        FROM documents \
        WHERE embedding <-> '[0.1, 0.2, 0.3]'::vector < 0.5 \
        ORDER BY embedding <-> '[0.1, 0.2, 0.3]'::vector \
        LIMIT 25"
            .to_string(),
        // Multi-vector search with different embeddings
        "SELECT id, title, \
            LEAST( \
                embedding <-> '[0.1, 0.2, 0.3]'::vector, \
                embedding <-> '[0.2, 0.3, 0.4]'::vector \
            ) AS min_distance \
        FROM documents \
        ORDER BY min_distance \
        LIMIT 10"
            .to_string(),
        // Vector search with metadata scoring
        "SELECT id, title, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS vec_dist, \
            view_count, \
            (embedding <-> '[0.1, 0.2, 0.3]'::vector) * 0.7 + \
            (1.0 - LEAST(view_count / 10000.0, 1.0)) * 0.3 AS score \
        FROM documents \
        ORDER BY score \
        LIMIT 20"
            .to_string(),
        // Vector search with JOIN
        "SELECT d.id, d.title, u.name, \
            d.embedding <-> '[0.1, 0.2, 0.3]'::vector AS distance \
        FROM documents d \
        JOIN users u ON d.author_id = u.id \
        WHERE u.reputation > 1000 \
        ORDER BY d.embedding <-> '[0.1, 0.2, 0.3]'::vector \
        LIMIT 15"
            .to_string(),
        // Batch vector search with UNNEST
        "SELECT q.query_id, d.id, d.title, \
            d.embedding <-> q.query_vec AS distance \
        FROM documents d, \
            UNNEST(ARRAY[ \
                '[0.1, 0.2, 0.3]'::vector, \
                '[0.2, 0.3, 0.4]'::vector, \
                '[0.3, 0.4, 0.5]'::vector \
            ]) WITH ORDINALITY AS q(query_vec, query_id) \
        WHERE d.embedding <-> q.query_vec < 0.6 \
        ORDER BY q.query_id, distance \
        LIMIT 30"
            .to_string(),
        // Vector search with aggregation
        "SELECT category, COUNT(*) as count, \
            AVG(embedding <-> '[0.1, 0.2, 0.3]'::vector) AS avg_distance \
        FROM documents \
        WHERE embedding <-> '[0.1, 0.2, 0.3]'::vector < 0.7 \
        GROUP BY category \
        ORDER BY avg_distance \
        LIMIT 10"
            .to_string(),
        // Vector search with window functions
        "SELECT id, title, category, \
            embedding <-> '[0.1, 0.2, 0.3]'::vector AS distance, \
            ROW_NUMBER() OVER (PARTITION BY category \
                ORDER BY embedding <-> '[0.1, 0.2, 0.3]'::vector) AS rank \
        FROM documents \
        WHERE embedding <-> '[0.1, 0.2, 0.3]'::vector < 0.8"
            .to_string(),
    ];

    println!("Comparing {} vector search queries...\n", queries.len());

    let report = compare_queries(&adapter, &queries)?;

    println!("{}", report.to_markdown());

    let json_output = "vector_search_comparison.json";
    std::fs::write(json_output, report.to_json()?)?;
    println!("\nJSON report saved to: {json_output}");

    let md_output = "vector_search_comparison.md";
    std::fs::write(md_output, report.to_markdown())?;
    println!("Markdown report saved to: {md_output}");

    Ok(())
}
