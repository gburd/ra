# Hybrid Search Quick Reference

## CLI Commands

### Basic Usage

```bash
# Parse SQL to relational algebra
ra-cli explain <query>

# Optimize with default settings
ra-cli optimize <query>

# Optimize with verbose output
ra-cli optimize <query> --verbose

# Show applied optimization rules
ra-cli optimize <query> --rules-applied

# Show statistics
ra-cli optimize <query> --stats

# Combination (recommended for learning)
ra-cli optimize <query> --verbose --rules-applied --stats
```

### Plan Visualization

```bash
# Show diff (before vs after)
ra-cli optimize <query> --diff colored

# Side-by-side comparison
ra-cli optimize <query> --diff side-by-side

# Plain diff (no colors)
ra-cli optimize <query> --diff plain
```

### Hardware Profiles

```bash
# Mobile device
ra-cli optimize <query> --hardware-profile mobile

# Laptop
ra-cli optimize <query> --hardware-profile laptop

# Desktop
ra-cli optimize <query> --hardware-profile desktop

# Server
ra-cli optimize <query> --hardware-profile server

# GPU server
ra-cli optimize <query> --hardware-profile gpu-server

# Auto-detect (default)
ra-cli optimize <query> --hardware-profile auto
```

### Using Production Statistics

```bash
# 1. Capture snapshot from PostgreSQL
psql -c "SELECT ra.capture_snapshot_to_file('/tmp/snapshot.toml')"

# 2. Optimize with real statistics
ra-cli optimize <query> --timeline /tmp/snapshot.toml --verbose
```

### Rule Control

```bash
# Show all available rules
ra-cli list

# Show hybrid search rules only
ra-cli list | grep -E "(hybrid|vector|fts)"

# Show all rules evaluated during optimization
ra-cli optimize <query> --rules-evaluated

# Show rules that were successfully applied
ra-cli optimize <query> --rules-applied

# Show all rules (available + applied + skipped)
ra-cli optimize <query> --rules-all
```

### Resource Budgets

```bash
# Interactive mode (< 50ms optimization)
ra-cli optimize <query> --resource-budget interactive

# Standard mode (balanced)
ra-cli optimize <query> --resource-budget standard

# Batch mode (unlimited time)
ra-cli optimize <query> --resource-budget batch

# Custom limits
ra-cli optimize <query> \
  --max-time 5000 \
  --max-memory 2048 \
  --max-iterations 500
```

### Export Formats

```bash
# JSON export
ra-cli optimize <query> --explain-format json > plan.json

# GraphML export (for visualization tools)
ra-cli optimize <query> --explain-format graphml > plan.graphml

# DOT format (for Graphviz)
ra-cli optimize <query> --explain-format dot | dot -Tpng > plan.png

# Text format (default)
ra-cli optimize <query> --explain-format text
```

## Example Queries

### 1. Vector-Only Search

```sql
-- Find 10 nearest neighbors using HNSW index
SELECT id, title, embedding <-> '[0.1, 0.2, 0.3]'::vector AS distance
FROM articles
ORDER BY distance
LIMIT 10;
```

**Expected Optimization:**
- Sequential scan → HNSW index scan (100x faster)
- Direct top-K retrieval (no full sort)

### 2. Full-Text Search Only

```sql
-- Find documents matching query with BM25 ranking
SELECT id, title, ts_rank(body_tsv, query) AS score
FROM articles, to_tsquery('english', 'database & optimization') AS query
WHERE body_tsv @@ query
ORDER BY score DESC
LIMIT 10;
```

**Expected Optimization:**
- LIKE scan → RUM index scan (100x faster)
- Ranked retrieval (avoids heap fetch)
- Skip-list intersection for multi-term queries

### 3. Hybrid Search (FTS + Vector)

```sql
-- Combine BM25 and vector similarity
SELECT
    id,
    title,
    ts_rank(body_tsv, query) AS bm25,
    1 - (embedding <-> vec) AS vector_sim,
    0.7 * ts_rank(body_tsv, query) + 0.3 * (1 - (embedding <-> vec)) AS hybrid
FROM articles,
     to_tsquery('english', 'database & optimization') AS query,
     '[0.1, 0.2, 0.3]'::vector AS vec
WHERE body_tsv @@ query AND embedding <-> vec < 0.5
ORDER BY hybrid DESC
LIMIT 10;
```

**Expected Optimization:**
- Strategy selection (FTS-first or Vector-first based on selectivity)
- Index scan introduction for both modalities
- Score fusion optimization (inline computation)
- Top-K optimization (direct retrieval)

### 4. Filtered Vector Search

```sql
-- Vector search with metadata filter
SELECT id, title
FROM articles
WHERE category = 'technology'
  AND published_at > NOW() - INTERVAL '30 days'
  AND embedding <-> '[0.1, 0.2, 0.3]'::vector < 0.5
ORDER BY embedding <-> '[0.1, 0.2, 0.3]'::vector
LIMIT 10;
```

**Expected Optimization:**
- Pre-filter vs post-filter decision
- If filters very selective: filter → brute-force vector
- If filters not selective: HNSW → filter results

## Understanding Output

### Plan Notation

```
TopK(k=10, orderBy=[score DESC])          ← Top-K operation
├─ Project(id, title, score)               ← Column projection
│  └─ VectorScan(                          ← Index scan
│       index=idx_embedding_hnsw,          ← Index used
│       metric=L2,                         ← Distance metric
│       k=10                                ← Limit pushed down
│     )
│     └─ Filter(category='tech')           ← Predicate
│        └─ Scan(articles)                 ← Table scan
```

### Cost Notation

```
Cost: 13,000ms → 125ms (104x faster)
```
- Left side: Original plan cost
- Right side: Optimized plan cost
- Multiplier: Speedup factor

### Strategy Notation

```
Strategy: FTSFirst(alpha=0.7)
```
- **FTSFirst**: Run full-text search first, then vector on results
- **VectorFirst**: Run vector search first, then FTS on results
- **Parallel**: Run both in parallel, merge with fusion
- **alpha**: Weight for FTS vs vector (0.7 = 70% FTS, 30% vector)

### Selectivity Notation

```
FTS selectivity: 0.005 (0.5%)
Vector selectivity: 0.10 (10%)
```
- Lower selectivity = more selective = fewer results
- Ra chooses more selective filter first

## Common Patterns

### Pattern 1: Semantic Search

```sql
-- Find semantically similar documents
SELECT id, title, embedding <=> query_vec AS similarity
FROM articles
WHERE embedding <=> query_vec > 0.8  -- High similarity
ORDER BY similarity DESC
LIMIT 20;
```

**Optimization:** Cosine similarity index scan

### Pattern 2: Keyword + Semantic

```sql
-- Must match keywords, ranked by semantic similarity
SELECT id, title
FROM articles
WHERE body_tsv @@ to_tsquery('rust | postgres | database')
ORDER BY embedding <-> query_vec
LIMIT 10;
```

**Optimization:** FTS filter → vector ranking on small result set

### Pattern 3: Multi-Field FTS

```sql
-- Search across title and body with weights
SELECT id,
       setweight(to_tsvector(title), 'A') ||
       setweight(to_tsvector(body), 'B') AS document,
       ts_rank(...) AS score
FROM articles
WHERE document @@ query
ORDER BY score DESC;
```

**Optimization:** Multi-column RUM index

### Pattern 4: Approximate K-NN

```sql
-- Find approximate nearest neighbors (faster, slightly less accurate)
SELECT id, embedding <-> query AS dist
FROM articles
ORDER BY dist
LIMIT 100;  -- Large K favors IVFFlat over HNSW
```

**Optimization:** HNSW (K<100) vs IVFFlat (K≥100) selection

## Troubleshooting

### Query Too Slow

```bash
# Check if indexes are being used
ra-cli optimize <query> --rules-applied --stats

# Look for:
# ✓ introduce-vector-index-scan ← Should be present
# ✓ introduce-fts-index-scan ← Should be present
```

**If indexes not used:**
1. Check index exists: `\d+ table_name`
2. Check statistics: `ANALYZE table_name`
3. Try with --timeline from real database

### Wrong Strategy Selected

```bash
# Check selectivity estimates
ra-cli optimize <query> --verbose --stats

# Look for:
# FTS selectivity: X
# Vector selectivity: Y
# Strategy chosen: ... (reason: ...)
```

**If wrong strategy:**
1. Verify statistics are up-to-date
2. Check predicate complexity
3. Consider manual index hints (in actual SQL)

### High Memory Usage

```bash
# Use memory-constrained profile
ra-cli optimize <query> --resource-budget memory-constrained

# Or set explicit limits
ra-cli optimize <query> --max-memory 1024
```

## Integration with PostgreSQL

### Enable Ra Extension

```sql
-- Enable extension
CREATE EXTENSION pg_ra_planner;

-- Configure for session
SET ra.enabled = true;
SET ra.log_level = 'debug';

-- Verify
SHOW ra.enabled;
```

### Capture Snapshot

```sql
-- Basic snapshot
SELECT ra.capture_snapshot_to_file('/tmp/snapshot.toml');

-- Detailed snapshot
SELECT ra.capture_snapshot_to_file(
    '/tmp/detailed.toml',
    include_indexes := true,
    include_stats := true,
    include_hardware := true
);

-- Verify snapshot
\! cat /tmp/snapshot.toml
```

### Use in Queries

```sql
-- Run query with Ra optimization
EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)
SELECT ... FROM ... WHERE ...;

-- Ra will automatically optimize if enabled
```

## Performance Expectations

| Query Type | Without Indexes | With Indexes | Ra Optimized |
|------------|----------------|--------------|--------------|
| Vector (seq) | 5,000ms | N/A | N/A |
| Vector (HNSW) | N/A | 50ms | 50ms |
| FTS (LIKE) | 8,000ms | N/A | N/A |
| FTS (RUM) | N/A | 80ms | 80ms |
| Hybrid (naive) | 13,000ms | 13,000ms | 125ms |
| Hybrid (wrong order) | 13,000ms | 4,800ms | 125ms |

**Key Insight:** Ra's strategy selection provides an additional **38x** improvement over naive hybrid search!

## See Also

- Full example: `/home/gburd/ws/ra/docs/examples/HYBRID_SEARCH_CLI_EXAMPLE.md`
- User guide: `/home/gburd/ws/ra/docs/user-guide/hybrid-search.md`
- API reference: `/home/gburd/ws/ra/docs/reference/hybrid-search-api.md`
- Status report: `/home/gburd/ws/ra/HYBRID_SEARCH_AND_NEXT_STEPS.md`
