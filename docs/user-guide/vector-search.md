# Vector Search

Vector similarity search finds documents with similar semantic meaning using embeddings. Ra supports HNSW and IVFFlat indexes for efficient approximate nearest neighbor search.

## Overview

Vector search represents documents and queries as high-dimensional vectors (embeddings). Similar vectors cluster together in embedding space, enabling semantic search that goes beyond exact keyword matching.

Key concepts:
- **Embedding**: Dense vector representation of text (typically 384-1536 dimensions)
- **Distance metric**: Measure of similarity between vectors (L2, cosine, inner product)
- **Index**: Data structure for fast approximate nearest neighbor search (HNSW, IVFFlat)

## Supported Index Types

### HNSW (Hierarchical Navigable Small World)

Best all-purpose vector index. Builds a multi-layer graph for fast nearest neighbor search.

**Pros:**
- Fast queries (10-100ms for millions of vectors)
- Good recall (95%+ with default parameters)
- Scales to 10M+ vectors
- No training required

**Cons:**
- Large index size (2-3x vector data size)
- Slow inserts during index build
- Not ideal for very large datasets (> 100M vectors)

**Use cases:**
- Production search systems
- Real-time applications
- Medium-to-large datasets (10K-10M vectors)

```sql
CREATE INDEX idx_embedding_hnsw ON documents
USING hnsw (embedding vector_l2_ops)
WITH (m = 16, ef_construction = 64);
```

### IVFFlat (Inverted File with Flat Compression)

Divides vector space into clusters, searches only nearest clusters.

**Pros:**
- Smaller index size than HNSW
- Faster index build
- Scales to 100M+ vectors
- Adjustable speed/recall tradeoff

**Cons:**
- Requires training (CLUSTER step)
- Lower recall than HNSW at same speed
- Sensitive to cluster count parameter

**Use cases:**
- Very large datasets (> 10M vectors)
- Batch processing (non-real-time)
- Memory-constrained systems

```sql
CREATE INDEX idx_embedding_ivf ON documents
USING ivfflat (embedding vector_l2_ops)
WITH (lists = 100);
```

### sqlite-vec (SQLite Extension)

Vector search for SQLite using brute-force or approximate methods.

**Pros:**
- No PostgreSQL dependency
- Simple setup
- Good for small datasets (< 100K vectors)

**Cons:**
- Limited to 1M vectors
- Slower than PostgreSQL indexes
- No production-grade guarantees

**Use cases:**
- Embedded applications
- Prototypes and demos
- Edge devices

```sql
-- Install extension
.load ./sqlite-vec

-- Create table
CREATE TABLE documents (
    id INTEGER PRIMARY KEY,
    embedding BLOB NOT NULL
);

-- Query (brute force for < 10K vectors)
SELECT id,
       vec_distance_l2(embedding, ?) AS distance
FROM documents
ORDER BY distance
LIMIT 10;
```

## Distance Metrics

### L2 (Euclidean) Distance

Straight-line distance in embedding space.

```sql
-- PostgreSQL
embedding <-> '[0.1, 0.2, 0.3]'::vector

-- SQLite
vec_distance_l2(embedding, ?)
```

**Use when:**
- Embeddings are not normalized
- Model outputs L2-optimized vectors
- Distance magnitude matters

Formula: `sqrt(sum((a_i - b_i)^2))`

### Cosine Distance

Angle between vectors, ignoring magnitude.

```sql
-- PostgreSQL
embedding <=> '[0.1, 0.2, 0.3]'::vector

-- SQLite
vec_distance_cosine(embedding, ?)
```

**Use when:**
- Embeddings are normalized to unit length
- Only direction matters, not magnitude
- Model uses cosine similarity loss

Formula: `1 - (dot(a, b) / (||a|| * ||b||))`

Most sentence embedding models (BERT, Sentence-BERT, OpenAI, Cohere) use cosine distance.

### Inner Product

Dot product between vectors.

```sql
-- PostgreSQL (note: pgvector uses negative inner product for sorting)
embedding <#> '[0.1, 0.2, 0.3]'::vector

-- SQLite
vec_distance_ip(embedding, ?)
```

**Use when:**
- Model explicitly trained for max inner product
- Asymmetric embeddings (different dimensions for queries/documents)
- You need raw scores, not distances

Formula: `sum(a_i * b_i)`

### Choosing a Metric

| Embedding Model | Recommended Metric |
|-----------------|-------------------|
| OpenAI text-embedding-3 | Cosine |
| Cohere embed-v3 | Cosine |
| Sentence-BERT | Cosine |
| Universal Sentence Encoder | Cosine |
| Word2Vec | Cosine |
| Custom models | Check model docs |

Test with your data:

```sql
-- Compare metrics on a known similar pair
SELECT
    d1.embedding <-> d2.embedding AS l2,
    d1.embedding <=> d2.embedding AS cosine,
    d1.embedding <#> d2.embedding AS inner_product
FROM documents d1, documents d2
WHERE d1.id = 123 AND d2.id = 456;
```

## Creating Vector Indexes

### PostgreSQL with pgvector

Install pgvector extension:

```bash
# Install from package manager
sudo apt install postgresql-14-pgvector

# Or build from source
git clone https://github.com/pgvector/pgvector.git
cd pgvector
make
sudo make install
```

Enable in database:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
```

Create table with vector column:

```sql
CREATE TABLE documents (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    embedding vector(384) NOT NULL  -- Dimension depends on model
);
```

Create HNSW index:

```sql
-- Default parameters (good for most cases)
CREATE INDEX idx_embedding_hnsw ON documents
USING hnsw (embedding vector_l2_ops);

-- Tuned for recall
CREATE INDEX idx_embedding_hnsw ON documents
USING hnsw (embedding vector_cosine_ops)
WITH (m = 32, ef_construction = 128);

-- Tuned for speed
CREATE INDEX idx_embedding_hnsw ON documents
USING hnsw (embedding vector_l2_ops)
WITH (m = 8, ef_construction = 32);
```

Create IVFFlat index:

```sql
-- Choose lists = sqrt(rows)
CREATE INDEX idx_embedding_ivf ON documents
USING ivfflat (embedding vector_l2_ops)
WITH (lists = 100);  -- For ~10K rows
```

### SQLite with sqlite-vec

Download extension:

```bash
wget https://github.com/asg017/sqlite-vec/releases/download/v0.1.0/sqlite-vec.so
```

Load in SQLite:

```sql
.load ./sqlite-vec
```

Create table:

```sql
CREATE TABLE documents (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL
);

CREATE TABLE document_embeddings (
    id INTEGER PRIMARY KEY REFERENCES documents(id),
    embedding BLOB NOT NULL
);
```

Insert embeddings:

```python
import sqlite3
import numpy as np

conn = sqlite3.connect('documents.db')
embedding = np.array([0.1, 0.2, 0.3], dtype=np.float32)

conn.execute(
    'INSERT INTO document_embeddings (id, embedding) VALUES (?, ?)',
    (doc_id, embedding.tobytes())
)
```

## Query Examples

### Basic Nearest Neighbor Search

```sql
-- PostgreSQL: Find 10 nearest documents
SELECT id, title,
       embedding <-> '[0.1, 0.2, 0.3, ...]'::vector AS distance
FROM documents
ORDER BY distance
LIMIT 10;

-- SQLite: Find 10 nearest documents
SELECT id, title,
       vec_distance_l2(embedding, ?) AS distance
FROM document_embeddings
ORDER BY distance
LIMIT 10;
```

### Distance Threshold

```sql
-- PostgreSQL: Find all documents within distance 0.5
SELECT id, title,
       embedding <-> '[...]'::vector AS distance
FROM documents
WHERE embedding <-> '[...]'::vector < 0.5
ORDER BY distance;

-- SQLite: Find all documents within distance 0.5
SELECT id, title,
       vec_distance_l2(embedding, ?) AS distance
FROM document_embeddings
WHERE vec_distance_l2(embedding, ?) < 0.5
ORDER BY distance;
```

### Pre-Filtering

```sql
-- PostgreSQL: Filter by category before vector search
SELECT id, title,
       embedding <-> '[...]'::vector AS distance
FROM documents
WHERE category = 'research'  -- Pre-filter
  AND embedding <-> '[...]'::vector < 0.5
ORDER BY distance
LIMIT 10;

-- Add index on filter column
CREATE INDEX idx_category ON documents (category);
```

### Multi-Vector Query

```sql
-- Find documents similar to multiple query vectors
SELECT id, title,
       LEAST(
           embedding <-> '[query1...]'::vector,
           embedding <-> '[query2...]'::vector
       ) AS min_distance
FROM documents
ORDER BY min_distance
LIMIT 10;
```

## Performance Optimization

### HNSW Tuning

Parameter `m`: Number of connections per layer.

```sql
-- Default: m = 16 (balanced)
CREATE INDEX idx_hnsw ON documents USING hnsw (embedding vector_l2_ops)
WITH (m = 16);

-- Higher recall: m = 32
CREATE INDEX idx_hnsw ON documents USING hnsw (embedding vector_l2_ops)
WITH (m = 32);  -- +50% index size, +10% recall

-- Faster build: m = 8
CREATE INDEX idx_hnsw ON documents USING hnsw (embedding vector_l2_ops)
WITH (m = 8);  -- -50% index size, -5% recall
```

Parameter `ef_construction`: Search beam width during index build.

```sql
-- Default: ef_construction = 64
CREATE INDEX idx_hnsw ON documents USING hnsw (embedding vector_l2_ops)
WITH (ef_construction = 64);

-- Higher quality: ef_construction = 128
CREATE INDEX idx_hnsw ON documents USING hnsw (embedding vector_l2_ops)
WITH (ef_construction = 128);  -- 2x slower build, +5% recall

-- Faster build: ef_construction = 32
CREATE INDEX idx_hnsw ON documents USING hnsw (embedding vector_l2_ops)
WITH (ef_construction = 32);  -- 2x faster build, -5% recall
```

Query-time parameter `ef_search`: Search beam width during query.

```sql
-- Default: 40 (fast, ~90% recall)
SET hnsw.ef_search = 40;

-- Balanced: 100 (~95% recall)
SET hnsw.ef_search = 100;

-- High recall: 200 (~99% recall)
SET hnsw.ef_search = 200;  -- 3x slower than default
```

### IVFFlat Tuning

Parameter `lists`: Number of clusters (choose sqrt(rows)).

```sql
-- 10K rows
CREATE INDEX idx_ivf ON documents USING ivfflat (embedding vector_l2_ops)
WITH (lists = 100);

-- 100K rows
CREATE INDEX idx_ivf ON documents USING ivfflat (embedding vector_l2_ops)
WITH (lists = 316);

-- 1M rows
CREATE INDEX idx_ivf ON documents USING ivfflat (embedding vector_l2_ops)
WITH (lists = 1000);
```

Query-time parameter `probes`: Number of clusters to search.

```sql
-- Fast: probes = 1 (~70% recall)
SET ivfflat.probes = 1;

-- Balanced: probes = 10 (~90% recall)
SET ivfflat.probes = 10;

-- High recall: probes = 50 (~99% recall)
SET ivfflat.probes = 50;  -- 5x slower than probes=10
```

### Memory Configuration

Increase shared_buffers for better caching:

```sql
-- In postgresql.conf
shared_buffers = 4GB  -- 25% of RAM
work_mem = 256MB      -- For sorting/merging
maintenance_work_mem = 2GB  -- For index builds
```

### Parallel Queries

Enable parallel execution for vector scans:

```sql
-- In postgresql.conf
max_parallel_workers_per_gather = 4
parallel_setup_cost = 100
parallel_tuple_cost = 0.001

-- Query hint
SET max_parallel_workers_per_gather = 4;
```

## Choosing Between HNSW and IVFFlat

| Factor | HNSW | IVFFlat |
|--------|------|---------|
| Dataset size | < 10M vectors | > 10M vectors |
| Query latency | < 50ms | < 200ms |
| Recall target | 95%+ | 85-95% |
| Index size | 2-3x data | 1-1.5x data |
| Build time | Slow | Fast |
| Memory usage | High | Medium |
| Training required | No | Yes |

Decision tree:

1. Dataset < 1M vectors → HNSW with default params
2. Dataset 1M-10M vectors → HNSW with m=16, ef_search=100
3. Dataset 10M-100M vectors → IVFFlat with lists=sqrt(rows), probes=10
4. Dataset > 100M vectors → Consider distributed search or dimensionality reduction

## Common Issues

### Index not used

Check if index is valid:

```sql
SELECT indexname, indexdef
FROM pg_indexes
WHERE tablename = 'documents';

-- Rebuild if needed
REINDEX INDEX idx_embedding_hnsw;
```

Force index usage:

```sql
SET enable_seqscan = off;
```

### Slow queries

Check execution plan:

```sql
EXPLAIN (ANALYZE, BUFFERS)
SELECT * FROM documents
ORDER BY embedding <-> '[...]'::vector
LIMIT 10;
```

Increase ef_search:

```sql
SET hnsw.ef_search = 200;
```

### Poor recall

Measure recall with ground truth:

```sql
-- Get ground truth (brute force search)
SET enable_indexscan = off;
SELECT id FROM documents
ORDER BY embedding <-> '[...]'::vector
LIMIT 10;

-- Get approximate results
SET enable_indexscan = on;
SELECT id FROM documents
ORDER BY embedding <-> '[...]'::vector
LIMIT 10;

-- Count overlap
```

Increase m and ef_construction:

```sql
DROP INDEX idx_embedding_hnsw;
CREATE INDEX idx_embedding_hnsw ON documents
USING hnsw (embedding vector_l2_ops)
WITH (m = 32, ef_construction = 128);
```

### Out of memory during index build

Reduce maintenance_work_mem:

```sql
SET maintenance_work_mem = '1GB';
REINDEX INDEX idx_embedding_hnsw;
```

Build index in batches:

```sql
-- Create table with partial index
CREATE INDEX idx_embedding_hnsw ON documents
USING hnsw (embedding vector_l2_ops)
WHERE id <= 1000000;

-- Extend index
DROP INDEX idx_embedding_hnsw;
CREATE INDEX idx_embedding_hnsw ON documents
USING hnsw (embedding vector_l2_ops);
```

## See Also

- [Hybrid Search](hybrid-search.md) - Combining vector and full-text search
- [Full-Text Search](full-text-search.md) - Keyword-based search
- [Hybrid Search Quickstart](../tutorials/hybrid-search-quickstart.md) - Step-by-step tutorial
