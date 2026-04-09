# PostgreSQL Performance Comparison Benchmarks

This directory contains benchmarks that compare native PostgreSQL execution vs Ra-optimized execution for various query types.

## Prerequisites

1. PostgreSQL database (version 12 or later recommended)
2. Required PostgreSQL extensions:
   - `pgvector` for vector similarity search
   - `pg_trgm` for trigram-based full-text search (built-in)
   - `rum` for advanced full-text search (optional)

## Setup

### 1. Install PostgreSQL Extensions

```sql
-- Create extensions in your benchmark database
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS rum;  -- optional
```

### 2. Set Environment Variable

```bash
export DATABASE_URL="postgresql://username:password@localhost/benchmark_db"
```

### 3. Create Test Schema

```sql
-- Create documents table for testing
CREATE TABLE documents (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    category VARCHAR(50),
    language VARCHAR(10) DEFAULT 'english',
    author_id INTEGER,
    author_reputation INTEGER DEFAULT 0,
    view_count INTEGER DEFAULT 0,
    created_at TIMESTAMP DEFAULT NOW(),
    embedding vector(3)  -- Using 3D vectors for demo, use higher dimensions in production
);

-- Create indexes
CREATE INDEX idx_documents_category ON documents(category);
CREATE INDEX idx_documents_created_at ON documents(created_at);
CREATE INDEX idx_documents_fts ON documents USING GIN(to_tsvector('english', content));
CREATE INDEX idx_documents_embedding ON documents USING ivfflat(embedding vector_cosine_ops) WITH (lists = 100);

-- Create users table for JOIN tests
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    reputation INTEGER DEFAULT 0
);

-- Insert sample data
INSERT INTO documents (title, content, category, author_id, view_count, embedding)
SELECT
    'Document ' || i,
    'This is sample content about machine learning, neural networks, and artificial intelligence. ' ||
    'Topics include deep learning, transformers, attention mechanisms, and natural language processing. ' ||
    'We also cover reinforcement learning, computer vision, and data science methodologies.',
    CASE (i % 3)
        WHEN 0 THEN 'research'
        WHEN 1 THEN 'blog'
        ELSE 'tutorial'
    END,
    (i % 10) + 1,
    (random() * 10000)::int,
    ARRAY[random()::float, random()::float, random()::float]::vector
FROM generate_series(1, 10000) i;

INSERT INTO users (name, reputation)
SELECT
    'User ' || i,
    (random() * 5000)::int
FROM generate_series(1, 100) i;

-- Analyze tables for accurate statistics
ANALYZE documents;
ANALYZE users;
```

## Running Benchmarks

### Hybrid Search Benchmark

Compares hybrid search combining vector similarity with full-text search:

```bash
cargo run --example benchmark_hybrid_search --features postgres
```

### Vector Search Benchmark

Compares pure vector similarity search:

```bash
cargo run --example benchmark_vector_search --features postgres
```

### Full-Text Search Benchmark

Compares PostgreSQL full-text search capabilities:

```bash
cargo run --example benchmark_fts --features postgres
```

## Output

Each benchmark generates:
- Console output with summary statistics
- JSON report (`*_comparison.json`) with detailed metrics
- Markdown report (`*_comparison.md`) for easy review

## Example Output

```
# PostgreSQL vs Ra Performance Comparison

**Generated:** 2026-04-06T10:00:00Z

## Summary

- **Total Queries:** 10
- **Improved:** 8 (80.0%)
- **Regressed:** 2 (20.0%)
- **Average Speedup:** 1.85x
- **Median Speedup:** 1.92x
- **Max Speedup:** 3.45x
- **Min Speedup:** 0.87x

## Detailed Results

| Query | Native (ms) | Ra (ms) | Speedup | Improvement |
|-------|-------------|---------|---------|-------------|
| SELECT ... | 125 | 65 | 1.92x | 48.0% |
```

## Understanding the Metrics

- **Execution Time**: Time taken to execute the query (milliseconds)
- **Rows Returned**: Number of result rows
- **Rows Scanned**: Estimated rows examined (from EXPLAIN)
- **Index Usage**: Indexes used during execution
- **Cost Estimate**: PostgreSQL's cost estimation
- **Speedup**: Ratio of native time to Ra time (>1.0 means improvement)
- **Improvement %**: Percentage improvement in execution time

## Integration Tests

Run integration tests against a live PostgreSQL database:

```bash
# Set test database URL
export TEST_POSTGRES_URL="postgresql://localhost/test_db"

# Run tests
cargo test -p ra-adapters --features postgres postgres_comparison
```

Note: Integration tests are marked `#[ignore]` by default. Use `--ignored` to run them:

```bash
cargo test -p ra-adapters --features postgres postgres_comparison -- --ignored
```

## Troubleshooting

### Extension Not Found

If you get errors about missing extensions:

```sql
-- Check installed extensions
SELECT * FROM pg_extension;

-- Check available extensions
SELECT * FROM pg_available_extensions WHERE name IN ('vector', 'pg_trgm', 'rum');
```

### Connection Issues

Ensure PostgreSQL is running and accepting connections:

```bash
psql -U postgres -c "SELECT version();"
```

### Performance Variations

Results may vary based on:
- Database size and data distribution
- Hardware resources (CPU, memory, disk)
- PostgreSQL configuration (`shared_buffers`, `work_mem`, etc.)
- Index statistics freshness (run `ANALYZE` regularly)
- Query plan cache state (first run may be slower)

## Customization

To add custom queries:

1. Edit the benchmark files (`benchmark_*.rs`)
2. Add queries to the `queries` vector
3. Rerun the benchmark

Example:

```rust
let queries = vec![
    "SELECT id, title FROM documents WHERE category = 'research' LIMIT 10".to_string(),
    // Add your custom queries here
];
```

## References

- [PostgreSQL Full-Text Search](https://www.postgresql.org/docs/current/textsearch.html)
- [pgvector Documentation](https://github.com/pgvector/pgvector)
- [pg_trgm Documentation](https://www.postgresql.org/docs/current/pgtrgm.html)
- [PostgreSQL EXPLAIN](https://www.postgresql.org/docs/current/using-explain.html)
