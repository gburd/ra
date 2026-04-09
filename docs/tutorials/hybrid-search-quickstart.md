# Hybrid Search Quickstart

This tutorial walks through setting up and using hybrid search with PostgreSQL, pgvector, and RUM indexes. By the end, you'll have a working search system combining keyword matching and semantic similarity.

## Prerequisites

- PostgreSQL 14+ installed
- Basic SQL knowledge
- 30 minutes

## Step 1: Install Extensions

Install pgvector and RUM extensions.

### Install pgvector

```bash
# Ubuntu/Debian
sudo apt install postgresql-14-pgvector

# Or build from source
git clone https://github.com/pgvector/pgvector.git
cd pgvector
make
sudo make install
```

### Install RUM

```bash
# Download source
git clone https://github.com/postgrespro/rum.git
cd rum
make USE_PGXS=1
sudo make USE_PGXS=1 install
```

### Enable Extensions

```sql
-- Connect to your database
psql -U postgres -d mydatabase

-- Enable extensions
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS rum;

-- Verify installation
\dx
```

Expected output:
```
                                List of installed extensions
  Name   | Version |   Schema   |                         Description
---------+---------+------------+--------------------------------------------------------------
 rum     | 1.3     | public     | RUM index access method
 vector  | 0.5.1   | public     | vector data type and ivfflat and hnsw access methods
```

## Step 2: Create Table Schema

Create a table for documents with text content and embeddings.

```sql
CREATE TABLE articles (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    author TEXT,
    published_at TIMESTAMP DEFAULT NOW(),

    -- Full-text search: Store tsvector for faster queries
    content_tsvector tsvector GENERATED ALWAYS AS
        (to_tsvector('english', title || ' ' || content)) STORED,

    -- Vector search: 384-dimensional embeddings (all-MiniLM-L6-v2 model)
    embedding vector(384) NOT NULL
);
```

## Step 3: Create Indexes

Create RUM index for full-text search and HNSW index for vector similarity.

```sql
-- RUM index for fast ranked full-text search
CREATE INDEX idx_articles_rum ON articles
USING rum (content_tsvector rum_tsvector_ops);

-- HNSW index for fast vector similarity search
CREATE INDEX idx_articles_hnsw ON articles
USING hnsw (embedding vector_cosine_ops)
WITH (m = 16, ef_construction = 64);

-- B-tree index for filtering
CREATE INDEX idx_articles_author ON articles (author);
CREATE INDEX idx_articles_published ON articles (published_at);
```

Wait for indexes to build:

```sql
-- Check index build progress
SELECT
    schemaname,
    tablename,
    indexname,
    idx_scan,
    idx_tup_read,
    idx_tup_fetch
FROM pg_stat_user_indexes
WHERE tablename = 'articles';
```

## Step 4: Load Sample Data

Insert sample articles with embeddings. In production, generate embeddings using a model like Sentence-BERT or OpenAI's text-embedding-3-small.

```sql
-- Sample data with mock embeddings (replace with real embeddings in production)
INSERT INTO articles (title, content, author, embedding) VALUES
(
    'Introduction to Machine Learning',
    'Machine learning is a subset of artificial intelligence that focuses on enabling computers to learn from data without being explicitly programmed. It uses statistical techniques to give computers the ability to learn patterns and make decisions.',
    'Alice Chen',
    -- Mock 384-dim embedding (use real embeddings in production)
    array_fill(0.1::float, ARRAY[384])::vector
),
(
    'Deep Learning Fundamentals',
    'Deep learning is a specialized branch of machine learning that uses neural networks with multiple layers. These deep neural networks can automatically learn hierarchical representations of data, making them particularly effective for tasks like image recognition and natural language processing.',
    'Bob Smith',
    array_fill(0.2::float, ARRAY[384])::vector
),
(
    'Natural Language Processing Guide',
    'Natural language processing (NLP) enables computers to understand, interpret, and generate human language. Modern NLP systems use transformer models and attention mechanisms to achieve state-of-the-art results on tasks like translation, summarization, and question answering.',
    'Carol Wu',
    array_fill(0.15::float, ARRAY[384])::vector
),
(
    'Python for Data Science',
    'Python has become the dominant language for data science due to its simplicity and rich ecosystem of libraries. Key libraries include NumPy for numerical computing, Pandas for data manipulation, and Scikit-learn for machine learning.',
    'David Lee',
    array_fill(0.3::float, ARRAY[384])::vector
),
(
    'Computer Vision Techniques',
    'Computer vision enables machines to interpret and understand visual information from the world. Modern techniques use convolutional neural networks to recognize objects, detect faces, segment images, and track motion across video frames.',
    'Eve Martinez',
    array_fill(0.25::float, ARRAY[384])::vector
);

-- Verify data loaded
SELECT id, title, author FROM articles;
```

### Generating Real Embeddings

In production, generate embeddings using a sentence embedding model:

```python
# Python example using sentence-transformers
from sentence_transformers import SentenceTransformer
import psycopg2
import numpy as np

# Load model
model = SentenceTransformer('sentence-transformers/all-MiniLM-L6-v2')

# Connect to database
conn = psycopg2.connect("dbname=mydatabase user=postgres")
cur = conn.cursor()

# Fetch articles
cur.execute("SELECT id, title, content FROM articles")
articles = cur.fetchall()

# Generate and update embeddings
for article_id, title, content in articles:
    text = f"{title} {content}"
    embedding = model.encode(text)

    cur.execute(
        "UPDATE articles SET embedding = %s WHERE id = %s",
        (embedding.tolist(), article_id)
    )

conn.commit()
```

## Step 5: Run Hybrid Queries

Now run hybrid search queries combining FTS and vector similarity.

### Query 1: Basic Hybrid Search with Weighted Fusion

```sql
-- Search for "neural networks" using hybrid approach
WITH query_params AS (
    SELECT
        to_tsquery('english', 'neural & networks') AS text_query,
        '[0.15, 0.15, 0.15, ...]'::vector AS query_embedding,  -- Replace with actual embedding
        0.6 AS alpha  -- Weight: 60% FTS, 40% vector
)
SELECT
    a.id,
    a.title,
    a.author,

    -- Individual scores
    ts_rank(a.content_tsvector, qp.text_query) AS fts_score,
    1.0 / (1.0 + a.embedding <=> qp.query_embedding) AS vector_score,

    -- Combined score (weighted average)
    qp.alpha * (ts_rank(a.content_tsvector, qp.text_query) / (ts_rank(a.content_tsvector, qp.text_query) + 1)) +
    (1 - qp.alpha) * (1.0 / (1.0 + a.embedding <=> qp.query_embedding)) AS hybrid_score
FROM articles a, query_params qp
WHERE a.content_tsvector @@ qp.text_query  -- FTS filter
  AND a.embedding <=> qp.query_embedding < 0.5  -- Vector distance threshold
ORDER BY hybrid_score DESC
LIMIT 10;
```

Expected output:
```
 id |            title             |    author     | fts_score | vector_score | hybrid_score
----+------------------------------+---------------+-----------+--------------+--------------
  2 | Deep Learning Fundamentals   | Bob Smith     |     0.607 |        0.800 |        0.684
  3 | Natural Language Processing  | Carol Wu      |     0.304 |        0.870 |        0.531
  1 | Introduction to ML           | Alice Chen    |     0.152 |        0.909 |        0.455
```

### Query 2: RRF (Reciprocal Rank Fusion)

RRF is more robust than weighted averaging and doesn't require score normalization.

```sql
WITH query_params AS (
    SELECT
        to_tsquery('english', 'neural & networks') AS text_query,
        '[0.15, 0.15, 0.15, ...]'::vector AS query_embedding,
        60 AS rrf_k  -- RRF constant
)
SELECT
    a.id,
    a.title,
    a.author,

    -- RRF score
    (1.0 / (qp.rrf_k + ts_rank(a.content_tsvector, qp.text_query))) +
    (1.0 / (qp.rrf_k + a.embedding <=> qp.query_embedding)) AS rrf_score
FROM articles a, query_params qp
WHERE a.content_tsvector @@ qp.text_query
ORDER BY rrf_score DESC
LIMIT 10;
```

### Query 3: FTS-First Strategy

When text query is highly selective, execute FTS first then compute vector distances only for matches.

```sql
-- FTS returns few matches, so compute vector distances only for those
SELECT
    a.id,
    a.title,
    a.author,
    ts_rank(a.content_tsvector, query) AS fts_score,
    a.embedding <=> query_embedding AS vector_distance
FROM articles a,
     to_tsquery('english', 'convolutional & neural & networks') AS query,
     '[0.25, 0.25, 0.25, ...]'::vector AS query_embedding
WHERE a.content_tsvector @@ query  -- Highly selective FTS
ORDER BY a.embedding <=> query_embedding  -- Sort by vector distance
LIMIT 10;
```

### Query 4: Vector-First Strategy

When vector query is highly selective, execute vector search first then compute FTS ranks.

```sql
-- Vector search returns few matches, so compute FTS ranks only for those
SELECT
    a.id,
    a.title,
    a.author,
    a.embedding <=> query_embedding AS vector_distance,
    ts_rank(a.content_tsvector, query) AS fts_score
FROM articles a,
     '[0.2, 0.2, 0.2, ...]'::vector AS query_embedding,
     to_tsquery('english', 'machine | learning') AS query
WHERE a.embedding <=> query_embedding < 0.3  -- Highly selective vector search
ORDER BY ts_rank(a.content_tsvector, query) DESC  -- Sort by FTS rank
LIMIT 10;
```

### Query 5: Parallel Strategy with Filters

For small result sets, execute both searches in parallel and merge.

```sql
-- Pre-filter by author, then hybrid search
WITH query_params AS (
    SELECT
        to_tsquery('english', 'machine & learning') AS text_query,
        '[0.1, 0.1, 0.1, ...]'::vector AS query_embedding
)
SELECT
    a.id,
    a.title,
    a.author,
    a.published_at,

    -- RRF fusion
    (1.0 / (60 + ts_rank(a.content_tsvector, qp.text_query))) +
    (1.0 / (60 + a.embedding <=> qp.query_embedding)) AS rrf_score
FROM articles a, query_params qp
WHERE a.author = 'Alice Chen'  -- Pre-filter
  AND (a.content_tsvector @@ qp.text_query OR a.embedding <=> qp.query_embedding < 0.5)
ORDER BY rrf_score DESC
LIMIT 10;
```

## Step 6: Analyze Query Performance

Check execution plans to verify indexes are used.

```sql
EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT
    a.id,
    a.title,
    (1.0 / (60 + ts_rank(a.content_tsvector, query))) +
    (1.0 / (60 + a.embedding <=> query_embedding)) AS rrf_score
FROM articles a,
     to_tsquery('english', 'neural & networks') AS query,
     '[0.15, 0.15, 0.15, ...]'::vector AS query_embedding
WHERE a.content_tsvector @@ query
ORDER BY rrf_score DESC
LIMIT 10;
```

Expected plan:
```
Limit  (cost=X..Y rows=10)
  ->  Sort  (cost=X..Y rows=Z)
        Sort Key: rrf_score DESC
        ->  Bitmap Heap Scan on articles  (cost=X..Y rows=Z)
              Recheck Cond: (content_tsvector @@ query)
              ->  Bitmap Index Scan on idx_articles_rum  (cost=X..Y rows=Z)
                    Index Cond: (content_tsvector @@ query)
```

Key indicators of good performance:
- "Bitmap Index Scan on idx_articles_rum" (RUM index used)
- "Index Scan using idx_articles_hnsw" (HNSW index used)
- Low "Buffers: shared read" (data cached in memory)
- Short execution time (< 50ms for thousands of documents)

## Step 7: Tune Performance

Adjust parameters for better speed or recall.

### Tune HNSW ef_search

```sql
-- Fast queries (90% recall)
SET hnsw.ef_search = 40;

-- Balanced (95% recall)
SET hnsw.ef_search = 100;

-- High recall (99% recall)
SET hnsw.ef_search = 200;

-- Test query with different settings
SELECT
    set_config('hnsw.ef_search', '100', false),
    a.title,
    a.embedding <=> '[...]'::vector AS distance
FROM articles a
ORDER BY distance
LIMIT 10;
```

### Tune Alpha Weight

Experiment with different alpha values:

```sql
-- Test different alpha values
WITH query_params AS (
    SELECT
        to_tsquery('english', 'neural & networks') AS text_query,
        '[0.15, 0.15, 0.15, ...]'::vector AS query_embedding
),
scores AS (
    SELECT
        a.title,
        ts_rank(a.content_tsvector, qp.text_query) AS fts,
        1.0 / (1.0 + a.embedding <=> qp.query_embedding) AS vec
    FROM articles a, query_params qp
    WHERE a.content_tsvector @@ qp.text_query
)
SELECT
    title,
    0.9 * fts + 0.1 * vec AS fts_heavy,
    0.7 * fts + 0.3 * vec AS fts_preferred,
    0.5 * fts + 0.5 * vec AS balanced,
    0.3 * fts + 0.7 * vec AS vector_preferred,
    0.1 * fts + 0.9 * vec AS vector_heavy
FROM scores
ORDER BY balanced DESC;
```

### Add Statistics Monitoring

Track query performance:

```sql
-- Create monitoring table
CREATE TABLE hybrid_search_stats (
    id SERIAL PRIMARY KEY,
    query_text TEXT,
    query_embedding_sample TEXT,  -- First 10 dims for reference
    fts_matches INT,
    vector_candidates INT,
    final_results INT,
    execution_time_ms FLOAT,
    strategy TEXT,  -- 'fts_first', 'vector_first', 'parallel'
    created_at TIMESTAMP DEFAULT NOW()
);

-- Log query in application code
INSERT INTO hybrid_search_stats (
    query_text,
    fts_matches,
    vector_candidates,
    final_results,
    execution_time_ms,
    strategy
) VALUES (
    'neural networks',
    2347,
    8921,
    10,
    45.3,
    'fts_first'
);

-- Analyze performance
SELECT
    strategy,
    COUNT(*) AS query_count,
    AVG(execution_time_ms) AS avg_time_ms,
    PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY execution_time_ms) AS p95_time_ms
FROM hybrid_search_stats
WHERE created_at > NOW() - INTERVAL '7 days'
GROUP BY strategy;
```

## Common Issues and Solutions

### Issue 1: Queries Return No Results

Check that both FTS and vector filters are not too restrictive:

```sql
-- Debug: Check FTS matches
SELECT COUNT(*) FROM articles
WHERE content_tsvector @@ to_tsquery('english', 'your_query');

-- Debug: Check vector matches
SELECT COUNT(*) FROM articles
WHERE embedding <=> '[...]'::vector < 0.5;

-- Solution: Relax filters
SELECT * FROM articles
WHERE content_tsvector @@ to_tsquery('english', 'your_query')
   OR embedding <=> '[...]'::vector < 0.8  -- Increased threshold
LIMIT 10;
```

### Issue 2: Slow Queries

Verify indexes are being used:

```sql
-- Check index usage
EXPLAIN SELECT * FROM articles
WHERE content_tsvector @@ to_tsquery('english', 'query')
ORDER BY embedding <=> '[...]'::vector
LIMIT 10;

-- If sequential scan appears, rebuild indexes
REINDEX INDEX idx_articles_rum;
REINDEX INDEX idx_articles_hnsw;

-- Update statistics
VACUUM ANALYZE articles;
```

### Issue 3: Poor Relevance

Try different score fusion methods:

```sql
-- If weighted average gives poor results, try RRF
SELECT title,
       (1.0 / (60 + ts_rank(content_tsvector, query))) +
       (1.0 / (60 + embedding <=> query_embedding)) AS rrf_score
FROM articles, to_tsquery('english', 'query') AS query
WHERE content_tsvector @@ query
ORDER BY rrf_score DESC
LIMIT 10;
```

## Next Steps

- [Hybrid Search User Guide](../user-guide/hybrid-search.md) - Detailed documentation
- [Vector Search Guide](../user-guide/vector-search.md) - Vector search optimization
- [Full-Text Search Guide](../user-guide/full-text-search.md) - FTS optimization
- [Hybrid Search API Reference](../reference/hybrid-search-api.md) - API documentation

## Further Reading

- [pgvector Documentation](https://github.com/pgvector/pgvector)
- [RUM Index Documentation](https://github.com/postgrespro/rum)
- [PostgreSQL Full-Text Search](https://www.postgresql.org/docs/current/textsearch.html)
- [Sentence Transformers](https://www.sbert.net/)
