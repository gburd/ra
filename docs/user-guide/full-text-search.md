# Full-Text Search

Full-text search (FTS) enables efficient keyword-based search with relevance ranking. Ra supports GIN, RUM, FULLTEXT, and fts5 indexes across PostgreSQL, MySQL, SQL Server, and SQLite.

## Overview

Full-text search breaks text into tokens, builds an inverted index, and ranks results by relevance. Unlike LIKE queries, FTS handles:

- Stemming: "running" matches "run"
- Stop words: Ignores common words like "the", "and"
- Ranking: Orders results by relevance (BM25, TF-IDF)
- Boolean operators: AND, OR, NOT, phrase search
- Language support: Dictionaries for 50+ languages

## Supported Index Types

### GIN (PostgreSQL)

Generalized Inverted Index. Standard PostgreSQL FTS index.

**Pros:**
- Fast lookup (O(log N))
- Compact storage
- Supports multiple operators (@@@, @@, @>, &&)
- Built-in to PostgreSQL

**Cons:**
- Slow ORDER BY ts_rank (requires sort)
- No direct ranking in index
- Large inserts are slow

**Use cases:**
- Standard FTS without ranking requirements
- Memory-constrained systems
- Read-heavy workloads

```sql
CREATE INDEX idx_content_gin ON documents
USING gin (to_tsvector('english', content));
```

### RUM (PostgreSQL)

RUM (Ranking Using Materialized views) extends GIN with ranking support.

**Pros:**
- Fast ORDER BY ts_rank (ranking in index)
- Better for top-K queries
- Supports additional operators (<=>)
- BM25-style ranking

**Cons:**
- Larger index than GIN (2-3x)
- Requires external extension
- Slower inserts than GIN

**Use cases:**
- Top-K ranked queries (ORDER BY ts_rank LIMIT 10)
- Production search systems
- Latency-critical applications

```sql
-- Install RUM extension
CREATE EXTENSION IF NOT EXISTS rum;

-- Create RUM index
CREATE INDEX idx_content_rum ON documents
USING rum (to_tsvector('english', content) rum_tsvector_ops);

-- Query with RUM ranking operator
SELECT * FROM documents
WHERE to_tsvector('english', content) @@ to_tsquery('english', 'search')
ORDER BY to_tsvector('english', content) <=> to_tsquery('english', 'search')
LIMIT 10;
```

### FULLTEXT (MySQL)

MySQL InnoDB full-text index.

**Pros:**
- Native MySQL support
- Fast for simple searches
- Boolean mode for complex queries

**Cons:**
- Limited ranking algorithms
- No phrase search with slop
- Slower than PostgreSQL RUM

**Use cases:**
- Existing MySQL deployments
- Simple keyword search
- Non-critical ranking requirements

```sql
CREATE TABLE documents (
    id INT PRIMARY KEY AUTO_INCREMENT,
    title VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    FULLTEXT idx_content (title, content)
) ENGINE=InnoDB;

-- Query
SELECT *, MATCH(title, content) AGAINST('search query' IN NATURAL LANGUAGE MODE) AS score
FROM documents
WHERE MATCH(title, content) AGAINST('search query' IN NATURAL LANGUAGE MODE)
ORDER BY score DESC
LIMIT 10;
```

### fts5 (SQLite)

SQLite virtual table for full-text search.

**Pros:**
- Built-in to SQLite
- Good performance for small datasets
- BM25 ranking
- Simple API

**Cons:**
- Limited to ~1M documents
- No external index maintenance
- Single-threaded

**Use cases:**
- Embedded applications
- Mobile apps
- Desktop software

```sql
-- Create FTS5 table
CREATE VIRTUAL TABLE documents_fts USING fts5(
    title,
    content,
    content='documents',
    content_rowid='id'
);

-- Query with BM25 ranking
SELECT * FROM documents_fts
WHERE documents_fts MATCH 'search query'
ORDER BY bm25(documents_fts)
LIMIT 10;
```

## Boolean Operators

### PostgreSQL tsquery Syntax

```sql
-- AND: Both terms must match
to_tsquery('english', 'machine & learning')

-- OR: Either term matches
to_tsquery('english', 'machine | computer')

-- NOT: Exclude term
to_tsquery('english', 'machine & !hardware')

-- Phrase: Terms in order (using <->)
to_tsquery('english', 'machine <-> learning')

-- Prefix search: Match prefix
to_tsquery('english', 'comput:*')  -- Matches: computer, computing, computation

-- Grouping: Control precedence
to_tsquery('english', '(machine | computer) & learning')
```

### MySQL FULLTEXT Syntax

```sql
-- Natural language mode (default)
MATCH(content) AGAINST('machine learning')

-- Boolean mode
MATCH(content) AGAINST('+machine +learning' IN BOOLEAN MODE)  -- AND
MATCH(content) AGAINST('machine learning' IN BOOLEAN MODE)     -- OR
MATCH(content) AGAINST('+machine -hardware' IN BOOLEAN MODE)   -- NOT
MATCH(content) AGAINST('"machine learning"' IN BOOLEAN MODE)   -- Phrase
MATCH(content) AGAINST('comput*' IN BOOLEAN MODE)              -- Prefix
```

### SQLite fts5 Syntax

```sql
-- AND
documents_fts MATCH 'machine AND learning'

-- OR
documents_fts MATCH 'machine OR computer'

-- NOT
documents_fts MATCH 'machine NOT hardware'

-- Phrase
documents_fts MATCH '"machine learning"'

-- Prefix
documents_fts MATCH 'comput*'

-- Column-specific search
documents_fts MATCH 'title:machine content:learning'
```

## Phrase Search and Proximity

### PostgreSQL Phrase Search

```sql
-- Exact phrase (adjacent terms)
to_tsquery('english', 'machine <-> learning')

-- Proximity search (terms within N positions)
to_tsquery('english', 'machine <3> learning')  -- Within 3 words

-- Ordered phrase with gap
to_tsquery('english', 'machine <2> deep <-> learning')
```

### MySQL Phrase Search

```sql
-- Exact phrase
MATCH(content) AGAINST('"machine learning"' IN BOOLEAN MODE)

-- Proximity search not supported (use BOOLEAN MODE workaround)
MATCH(content) AGAINST('+machine +learning' IN BOOLEAN MODE)
```

### SQLite Proximity Search

```sql
-- Exact phrase
documents_fts MATCH '"machine learning"'

-- Proximity search (NEAR operator)
documents_fts MATCH 'NEAR(machine learning, 5)'  -- Within 5 tokens
```

## Ranking Algorithms

### BM25 (Best Match 25)

Probabilistic ranking function. Best general-purpose algorithm.

**PostgreSQL (RUM):**

```sql
-- RUM index automatically uses BM25-style ranking
SELECT *, to_tsvector('english', content) <=> to_tsquery('english', 'query') AS rank
FROM documents
WHERE to_tsvector('english', content) @@ to_tsquery('english', 'query')
ORDER BY rank
LIMIT 10;
```

**SQLite (fts5):**

```sql
SELECT *, bm25(documents_fts) AS rank
FROM documents_fts
WHERE documents_fts MATCH 'query'
ORDER BY rank
LIMIT 10;
```

BM25 formula:
```
score = sum over terms of:
  IDF(term) * (f(term) * (k1 + 1)) / (f(term) + k1 * (1 - b + b * (doc_length / avg_doc_length)))

where:
  IDF(term) = log((N - n(term) + 0.5) / (n(term) + 0.5))
  f(term) = term frequency in document
  k1 = 1.2 (term saturation parameter)
  b = 0.75 (length normalization parameter)
```

### TF-IDF (Term Frequency - Inverse Document Frequency)

Classic ranking algorithm. Simpler than BM25.

**PostgreSQL (GIN with ts_rank):**

```sql
SELECT *, ts_rank(to_tsvector('english', content), to_tsquery('english', 'query')) AS rank
FROM documents
WHERE to_tsvector('english', content) @@ to_tsquery('english', 'query')
ORDER BY rank DESC
LIMIT 10;
```

TF-IDF formula:
```
score = sum over terms of:
  TF(term) * IDF(term)

where:
  TF(term) = f(term) / max_term_frequency_in_doc
  IDF(term) = log(N / n(term))
```

### ts_rank (PostgreSQL)

PostgreSQL's built-in ranking function with normalization options.

```sql
-- Default normalization
ts_rank(tsvector, tsquery)

-- Normalization options (bitmask):
-- 0: No normalization
-- 1: Divide by 1 + log(document length)
-- 2: Divide by document length
-- 4: Divide by mean harmonic distance between extents
-- 8: Divide by number of unique words in document
-- 16: Divide by 1 + log(number of unique words)
-- 32: Divide by rank itself + 1

-- Example: Length normalization + unique words
ts_rank(to_tsvector('english', content), to_tsquery('english', 'query'), 2|8)
```

### Custom Ranking

Combine multiple signals:

```sql
-- PostgreSQL: Boost title matches
SELECT *,
       (
           2.0 * ts_rank(to_tsvector('english', title), query) +
           1.0 * ts_rank(to_tsvector('english', content), query)
       ) AS combined_rank
FROM documents, to_tsquery('english', 'search') AS query
WHERE to_tsvector('english', title) @@ query
   OR to_tsvector('english', content) @@ query
ORDER BY combined_rank DESC
LIMIT 10;

-- Incorporate recency
SELECT *,
       ts_rank(to_tsvector('english', content), query) *
       (1.0 / (1.0 + EXTRACT(EPOCH FROM NOW() - created_at) / 86400)) AS rank
FROM documents, to_tsquery('english', 'search') AS query
WHERE to_tsvector('english', content) @@ query
ORDER BY rank DESC;
```

## Query Syntax

### PostgreSQL

```sql
-- Basic search
SELECT * FROM documents
WHERE to_tsvector('english', content) @@ to_tsquery('english', 'machine learning');

-- Stored tsvector column (recommended for performance)
CREATE TABLE documents (
    id SERIAL PRIMARY KEY,
    content TEXT,
    content_tsvector tsvector GENERATED ALWAYS AS (to_tsvector('english', content)) STORED
);

CREATE INDEX idx_content_gin ON documents USING gin (content_tsvector);

SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'machine learning');

-- Plain text query (converts to tsquery automatically)
SELECT * FROM documents
WHERE to_tsvector('english', content) @@ plainto_tsquery('english', 'machine learning');

-- Phrase search query (handles phrases better than plainto_tsquery)
SELECT * FROM documents
WHERE to_tsvector('english', content) @@ phraseto_tsquery('english', 'machine learning');

-- Websearch query (user-friendly syntax)
SELECT * FROM documents
WHERE to_tsvector('english', content) @@ websearch_to_tsquery('english', '"machine learning" -hardware');
```

### MySQL

```sql
-- Natural language mode
SELECT * FROM documents
WHERE MATCH(content) AGAINST('machine learning');

-- Boolean mode
SELECT * FROM documents
WHERE MATCH(content) AGAINST('+machine +learning -hardware' IN BOOLEAN MODE);

-- Query expansion (automatic synonym expansion)
SELECT * FROM documents
WHERE MATCH(content) AGAINST('machine learning' WITH QUERY EXPANSION);
```

### SQLite

```sql
-- Basic search
SELECT * FROM documents_fts
WHERE documents_fts MATCH 'machine learning';

-- Column-specific
SELECT * FROM documents_fts
WHERE documents_fts MATCH 'title:machine content:learning';

-- Phrase search
SELECT * FROM documents_fts
WHERE documents_fts MATCH '"machine learning"';

-- Boolean operators
SELECT * FROM documents_fts
WHERE documents_fts MATCH 'machine AND learning NOT hardware';
```

## Performance Optimization

### Index Tuning

**PostgreSQL GIN:**

```sql
-- Fast updates at the cost of query speed
CREATE INDEX idx_content_gin ON documents
USING gin (to_tsvector('english', content))
WITH (fastupdate = on);

-- Optimize for query speed
CREATE INDEX idx_content_gin ON documents
USING gin (to_tsvector('english', content))
WITH (fastupdate = off, gin_pending_list_limit = 4096);
```

**PostgreSQL RUM:**

```sql
-- Default parameters
CREATE INDEX idx_content_rum ON documents
USING rum (to_tsvector('english', content) rum_tsvector_ops);

-- Include additional data in index for faster sorting
CREATE INDEX idx_content_rum ON documents
USING rum (to_tsvector('english', content) rum_tsvector_addon_ops, created_at)
WITH (attach = 'created_at', to = 'to_tsvector');
```

**MySQL FULLTEXT:**

```sql
-- Tune minimum word length
SET GLOBAL innodb_ft_min_token_size = 3;  -- Default: 3
SET GLOBAL innodb_ft_max_token_size = 84; -- Default: 84

-- Rebuild index after parameter change
ALTER TABLE documents DROP INDEX idx_content;
ALTER TABLE documents ADD FULLTEXT idx_content (content);
```

### Query Optimization

Cache tsvector column:

```sql
-- Bad: Recomputes tsvector on every query
SELECT * FROM documents
WHERE to_tsvector('english', content) @@ to_tsquery('english', 'query');

-- Good: Use stored tsvector column
CREATE TABLE documents (
    content TEXT,
    content_tsvector tsvector GENERATED ALWAYS AS (to_tsvector('english', content)) STORED
);
CREATE INDEX idx_content ON documents USING gin (content_tsvector);

SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'query');
```

Use covering indexes:

```sql
-- Include frequently accessed columns in index
CREATE INDEX idx_content_covering ON documents
USING gin (to_tsvector('english', content))
INCLUDE (title, created_at);

-- Query only needs index
SELECT id, title, created_at FROM documents
WHERE to_tsvector('english', content) @@ to_tsquery('english', 'query');
```

Limit result set:

```sql
-- Always use LIMIT for ranked search
SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'query')
ORDER BY ts_rank(content_tsvector, to_tsquery('english', 'query')) DESC
LIMIT 100;  -- Prevents sorting entire result set
```

### Statistics and VACUUM

Keep statistics fresh:

```sql
-- After bulk inserts/updates
VACUUM ANALYZE documents;

-- Check statistics age
SELECT schemaname, tablename, last_analyze, last_autoanalyze
FROM pg_stat_user_tables
WHERE tablename = 'documents';

-- Auto-vacuum settings (in postgresql.conf)
autovacuum = on
autovacuum_analyze_scale_factor = 0.1
```

## Common Issues

### Query returns no results

Check text search configuration:

```sql
-- Verify configuration
SHOW default_text_search_config;  -- Should be 'pg_catalog.english' or your language

-- Test tokenization
SELECT to_tsvector('english', 'your text');
SELECT to_tsquery('english', 'your query');

-- Test match
SELECT to_tsvector('english', 'machine learning') @@ to_tsquery('english', 'machine & learning');
```

### Poor ranking

Tune ranking parameters:

```sql
-- Add length normalization
ts_rank(content_tsvector, query, 1)  -- Divide by 1 + log(doc_length)

-- Boost title matches
2.0 * ts_rank(title_tsvector, query) + ts_rank(content_tsvector, query)

-- Add recency factor
ts_rank(content_tsvector, query) * exp(-age_days / 365.0)
```

### Slow queries

Check execution plan:

```sql
EXPLAIN (ANALYZE, BUFFERS)
SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'query')
ORDER BY ts_rank(content_tsvector, to_tsquery('english', 'query')) DESC
LIMIT 10;
```

Switch to RUM index for ranked queries:

```sql
-- GIN requires sorting (slow for large result sets)
-- RUM supports ranking in index (fast)
CREATE INDEX idx_content_rum ON documents
USING rum (content_tsvector rum_tsvector_ops);

SELECT * FROM documents
WHERE content_tsvector @@ to_tsquery('english', 'query')
ORDER BY content_tsvector <=> to_tsquery('english', 'query')
LIMIT 10;
```

### Stop words ignored

Stop words are filtered by default. To keep them:

```sql
-- Create custom text search configuration
CREATE TEXT SEARCH CONFIGURATION my_config (COPY = pg_catalog.english);

-- Remove stop word dictionary
ALTER TEXT SEARCH CONFIGURATION my_config
    ALTER MAPPING FOR asciiword, asciihword, hword_asciipart, word, hword, hword_part
    WITH simple;  -- No stop words

-- Use custom configuration
CREATE INDEX idx_content ON documents
USING gin (to_tsvector('my_config', content));

SELECT * FROM documents
WHERE to_tsvector('my_config', content) @@ to_tsquery('my_config', 'the');
```

## See Also

- [Hybrid Search](hybrid-search.md) - Combining FTS with vector search
- [Vector Search](vector-search.md) - Semantic similarity search
- [Hybrid Search Quickstart](../tutorials/hybrid-search-quickstart.md) - Step-by-step tutorial
