# Hybrid Search

Hybrid search combines full-text search (FTS) and vector similarity search to provide both keyword-based and semantic matching. Ra automatically selects the optimal execution strategy based on query characteristics and table statistics.

## What is Hybrid Search?

Hybrid search integrates two search modalities:

1. **Full-Text Search (FTS)** - Keyword-based search using BM25 ranking. Matches exact terms and phrases with relevance scoring.

2. **Vector Similarity Search** - Semantic search using embeddings. Finds conceptually similar documents even when exact terms don't match.

Combining these approaches produces better results than either alone:
- FTS catches exact terminology and rare terms
- Vector search handles synonyms, paraphrasing, and semantic concepts
- Hybrid fusion balances both signals

## When to Use Hybrid Search

Use hybrid search when:
- Users expect both exact-match and semantic results
- Document relevance depends on keywords and concepts
- You have both text content and embeddings
- Query understanding requires multiple modalities

Examples:
- Product search: match brand names (FTS) and similar features (vector)
- Document retrieval: find specific citations (FTS) and related papers (vector)
- Code search: match function names (FTS) and semantic intent (vector)

Skip hybrid search when:
- Only one modality is needed
- Embeddings are unavailable
- Query latency is critical (< 10ms)

## Supported Databases

| Database   | FTS Index | Vector Index | Status      |
|------------|-----------|--------------|-------------|
| PostgreSQL | RUM, GIN  | pgvector     | Supported   |
| MySQL      | FULLTEXT  | Not available| Partial     |
| SQL Server | FULLTEXT  | Not available| Partial     |
| SQLite     | fts5      | sqlite-vec   | Supported   |

PostgreSQL with RUM + pgvector provides the best performance. SQLite with fts5 + sqlite-vec works for smaller datasets (< 1M documents).

## Required Extensions

### PostgreSQL

Install pgvector and RUM:

```sql
-- Install extensions
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS rum;

-- Create table with text and vector columns
CREATE TABLE documents (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    content_tsvector tsvector GENERATED ALWAYS AS (to_tsvector('english', content)) STORED,
    embedding vector(384) NOT NULL
);

-- Create indexes
CREATE INDEX idx_content_rum ON documents USING rum (content_tsvector rum_tsvector_ops);
CREATE INDEX idx_embedding_hnsw ON documents USING hnsw (embedding vector_l2_ops);
```

### SQLite

Install fts5 (built-in) and sqlite-vec:

```sql
-- Enable extensions (requires loadable extensions enabled)
.load ./sqlite-vec

-- Create FTS5 table
CREATE VIRTUAL TABLE documents_fts USING fts5(
    title,
    content,
    content='documents',
    content_rowid='id'
);

-- Create vector table
CREATE TABLE documents (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL
);

CREATE TABLE document_embeddings (
    id INTEGER PRIMARY KEY REFERENCES documents(id),
    embedding BLOB NOT NULL
);

-- Create vector index
CREATE INDEX idx_embedding_vec ON document_embeddings USING vec_index(embedding);
```

### MySQL

MySQL supports FULLTEXT but lacks native vector search. Consider using PostgreSQL for hybrid search.

```sql
-- FTS only (no vector support)
CREATE TABLE documents (
    id INT PRIMARY KEY AUTO_INCREMENT,
    title VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    FULLTEXT idx_content (title, content)
) ENGINE=InnoDB;
```

## Query Examples

### PostgreSQL with RUM + pgvector

```sql
-- Basic hybrid search
SELECT title, content,
       ts_rank(content_tsvector, query) AS text_score,
       embedding <-> query_embedding AS vector_distance
FROM documents,
     to_tsquery('english', 'machine & learning') AS query,
     '[0.1, 0.2, 0.3, ...]'::vector AS query_embedding
WHERE content_tsvector @@ query
  AND embedding <-> query_embedding < 0.5
ORDER BY (
    0.7 * ts_rank(content_tsvector, query) +
    0.3 * (1.0 / (1.0 + embedding <-> query_embedding))
) DESC
LIMIT 20;

-- Using RRF fusion (recommended)
SELECT title, content,
       (1.0 / (60 + ts_rank(content_tsvector, query))) +
       (1.0 / (60 + embedding <-> query_embedding)) AS rrf_score
FROM documents,
     to_tsquery('english', 'neural & networks') AS query,
     '[...]'::vector AS query_embedding
WHERE content_tsvector @@ query
ORDER BY rrf_score DESC
LIMIT 20;

-- FTS-first strategy (highly selective text query)
SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'specific_rare_term')
ORDER BY embedding <-> '[...]'::vector
LIMIT 10;

-- Vector-first strategy (highly selective vector query)
SELECT * FROM documents
WHERE embedding <-> '[...]'::vector < 0.2
ORDER BY ts_rank(content_tsvector, to_tsquery('english', 'general_term')) DESC
LIMIT 10;
```

### SQLite with fts5 + sqlite-vec

```sql
-- Basic hybrid search
SELECT d.title, d.content,
       fts.rank AS text_score,
       vec_distance_l2(e.embedding, ?) AS vector_distance
FROM documents d
JOIN documents_fts fts ON d.id = fts.rowid
JOIN document_embeddings e ON d.id = e.id
WHERE documents_fts MATCH 'machine learning'
  AND vec_distance_l2(e.embedding, ?) < 0.5
ORDER BY (
    0.7 * (-fts.rank) +
    0.3 * (1.0 / (1.0 + vec_distance_l2(e.embedding, ?)))
) DESC
LIMIT 20;

-- Note: Bind the same embedding vector to all ? placeholders
```

## Performance Tuning

### Alpha Weight (Weighted Average Fusion)

Controls the balance between FTS and vector scores:

```sql
-- alpha = 0.7: Prefer FTS (keyword-heavy queries)
alpha * ts_rank(...) + (1 - alpha) * vector_similarity(...)

-- alpha = 0.3: Prefer vector (semantic-heavy queries)
0.3 * ts_rank(...) + 0.7 * vector_similarity(...)
```

Tune alpha based on your use case:
- E-commerce: alpha = 0.7 (exact product names matter)
- Research papers: alpha = 0.5 (balanced)
- General search: alpha = 0.6 (slight keyword preference)

### HNSW Parameters (pgvector)

Control HNSW index performance vs recall:

```sql
-- Build time parameters
CREATE INDEX idx_embedding_hnsw ON documents
USING hnsw (embedding vector_l2_ops)
WITH (m = 16, ef_construction = 64);

-- Query time parameters
SET hnsw.ef_search = 100;  -- Higher = better recall, slower
```

Guidelines:
- `m = 16`: Default, good balance
- `m = 32`: Better recall, larger index
- `ef_construction = 64`: Default build quality
- `ef_search = 40`: Fast, 90% recall
- `ef_search = 100`: Balanced, 95% recall
- `ef_search = 200`: Slow, 99% recall

### IVFFlat Parameters (pgvector)

Alternative to HNSW for larger datasets:

```sql
-- Create IVFFlat index
CREATE INDEX idx_embedding_ivf ON documents
USING ivfflat (embedding vector_l2_ops)
WITH (lists = 100);

-- Query time parameters
SET ivfflat.probes = 10;  -- Number of lists to search
```

Guidelines:
- `lists = sqrt(rows)`: Rule of thumb
- `lists = 100`: Good for 10K-1M rows
- `lists = 1000`: Good for 1M-10M rows
- `probes = 1`: Fastest, ~70% recall
- `probes = 10`: Balanced, ~90% recall
- `probes = 50`: Slow, ~99% recall

### RUM Index Parameters (PostgreSQL)

Tune RUM index for FTS performance:

```sql
-- Create RUM index with custom operators
CREATE INDEX idx_content_rum ON documents
USING rum (content_tsvector rum_tsvector_addon_ops, id)
WITH (attach = 'id', to = 'content_tsvector');

-- Query using RUM ordering
SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'query')
ORDER BY content_tsvector <=> to_tsquery('english', 'query')
LIMIT 20;
```

RUM advantages over GIN:
- Supports ORDER BY on ts_rank directly
- Faster for top-K queries
- Better for BM25-style ranking

## Common Pitfalls

### Pitfall 1: Missing VACUUM ANALYZE

Symptom: Optimizer chooses wrong strategy, slow queries.

Solution: Keep statistics fresh.

```sql
-- After bulk inserts or updates
VACUUM ANALYZE documents;

-- Check statistics age
SELECT schemaname, tablename, last_analyze, last_autoanalyze
FROM pg_stat_user_tables
WHERE tablename = 'documents';
```

### Pitfall 2: Wrong Distance Metric

Symptom: Poor semantic relevance.

Solution: Match distance metric to embedding model.

```sql
-- L2 (Euclidean) distance
CREATE INDEX idx_embed_l2 ON documents USING hnsw (embedding vector_l2_ops);
SELECT * FROM documents ORDER BY embedding <-> '[...]'::vector;

-- Cosine distance (normalized embeddings)
CREATE INDEX idx_embed_cos ON documents USING hnsw (embedding vector_cosine_ops);
SELECT * FROM documents ORDER BY embedding <=> '[...]'::vector;

-- Inner product (for max-inner-product embeddings)
CREATE INDEX idx_embed_ip ON documents USING hnsw (embedding vector_ip_ops);
SELECT * FROM documents ORDER BY embedding <#> '[...]'::vector;
```

Most sentence embedding models use cosine distance after normalization.

### Pitfall 3: Unnormalized Score Fusion

Symptom: One modality dominates the combined score.

Solution: Normalize scores before fusion.

```sql
-- BAD: Raw scores have different ranges
ts_rank(...) + embedding <-> '[...]'::vector  -- BM25 is 0-20, distance is 0-2

-- GOOD: Normalize to [0, 1]
(ts_rank(...) / (ts_rank(...) + 1)) + (1 / (1 + embedding <-> '[...]'::vector))

-- BETTER: Use RRF (no normalization needed)
(1 / (60 + ts_rank(...))) + (1 / (60 + embedding <-> '[...]'::vector))
```

### Pitfall 4: Missing Index on Filter Predicates

Symptom: Hybrid query is fast, but adding WHERE filters causes sequential scan.

Solution: Create indexes on filter columns.

```sql
-- Slow: Sequential scan on category filter
SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'machine learning')
  AND category = 'research'  -- No index!
ORDER BY embedding <-> '[...]'::vector
LIMIT 10;

-- Fast: Add index on category
CREATE INDEX idx_category ON documents (category);
```

### Pitfall 5: Over-Fetching Candidates

Symptom: Hybrid query retrieves too many candidates, slowing the merge phase.

Solution: Add selectivity filters on both modalities.

```sql
-- BAD: FTS returns 100K rows, vector computes distances for all
SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'the')  -- Too broad!
ORDER BY embedding <-> '[...]'::vector
LIMIT 10;

-- GOOD: Add distance threshold
SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'specific & terms')
  AND embedding <-> '[...]'::vector < 0.5  -- Prune distant vectors
ORDER BY hybrid_score DESC
LIMIT 10;
```

## Troubleshooting

### Query is slow

Check execution plan:

```sql
EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'machine learning')
  AND embedding <-> '[...]'::vector < 0.5
ORDER BY hybrid_score DESC
LIMIT 20;
```

Look for:
- Sequential scans (missing indexes)
- High "Rows Removed by Filter" (poor selectivity)
- "Buffers: shared read" (disk I/O, increase shared_buffers)

### Index not used

Force index usage for testing:

```sql
-- Disable sequential scans temporarily
SET enable_seqscan = off;

-- Re-run query and check plan
EXPLAIN SELECT * FROM documents WHERE ...;

-- Re-enable sequential scans
SET enable_seqscan = on;
```

If index is still not used:
1. Check index exists: `\d documents`
2. Run ANALYZE: `ANALYZE documents;`
3. Increase work_mem: `SET work_mem = '256MB';`

### Poor relevance

Tune score fusion:

```sql
-- Experiment with different alpha values
WITH scores AS (
  SELECT title,
         ts_rank(content_tsvector, query) AS fts,
         1.0 / (1.0 + embedding <-> query_embedding) AS vec
  FROM documents, to_tsquery('english', 'query') AS query
  WHERE content_tsvector @@ query
)
SELECT title,
       0.9 * fts + 0.1 * vec AS alpha_0_9,
       0.7 * fts + 0.3 * vec AS alpha_0_7,
       0.5 * fts + 0.5 * vec AS alpha_0_5,
       0.3 * fts + 0.7 * vec AS alpha_0_3
FROM scores
ORDER BY alpha_0_7 DESC
LIMIT 10;
```

Check embedding quality:

```sql
-- Find nearest neighbors of a document
SELECT d2.title,
       d1.embedding <-> d2.embedding AS distance
FROM documents d1, documents d2
WHERE d1.id = 123
  AND d2.id != 123
ORDER BY distance
LIMIT 10;
```

If nearest neighbors are unrelated, re-train embeddings or try a different model.

## See Also

- [Vector Search](vector-search.md) - Vector similarity search details
- [Full-Text Search](full-text-search.md) - FTS optimization guide
- [Hybrid Search API Reference](../reference/hybrid-search-api.md) - Complete API documentation
- [Hybrid Search Quickstart](../tutorials/hybrid-search-quickstart.md) - Step-by-step tutorial
