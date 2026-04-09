# Hybrid Search with ra-cli: Complete Example

This document demonstrates how to use `ra-cli` to optimize hybrid search queries that combine vector similarity and full-text search.

## Query Example

```sql
SELECT
    id,
    title,
    ts_rank(body_tsv, to_tsquery('english', 'database & optimization')) AS bm25_score,
    1 - (embedding <-> '[0.1, 0.2, 0.3]'::vector) AS vector_score,
    (0.7 * ts_rank(body_tsv, to_tsquery('english', 'database & optimization')) +
     0.3 * (1 - (embedding <-> '[0.1, 0.2, 0.3]'::vector))) AS hybrid_score
FROM articles
WHERE
    body_tsv @@ to_tsquery('english', 'database & optimization')
    AND embedding <-> '[0.1, 0.2, 0.3]'::vector < 0.5
ORDER BY hybrid_score DESC
LIMIT 10;
```

## Basic Optimization

### 1. Parse SQL to Relational Algebra

```bash
ra-cli explain examples/hybrid-search-example.sql
```

**Expected Output:**
```
TopK(k=10, orderBy=[hybrid_score DESC])
├─ Project(
│    id,
│    title,
│    ts_rank(body_tsv, query) AS bm25_score,
│    1 - vector_distance(embedding, target, L2) AS vector_score,
│    0.7 * ts_rank(...) + 0.3 * (1 - vector_distance(...)) AS hybrid_score
│  )
│  └─ Filter(
│       body_tsv @@ query AND vector_distance(embedding, target, L2) < 0.5
│     )
│     └─ Scan(articles)
```

### 2. Optimize with Verbose Output

```bash
ra-cli optimize examples/hybrid-search-example.sql \
  --verbose \
  --rules-applied \
  --stats
```

**Expected Output:**
```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 OPTIMIZATION REPORT
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Original Plan:
──────────────
TopK(k=10, orderBy=[hybrid_score DESC])
└─ Project(...)
   └─ Filter(fts_match AND vector_distance < 0.5)
      └─ Scan(articles)

Optimized Plan:
───────────────
HybridSearchScan(
  strategy=FTSFirst(alpha=0.7),
  fts_index=idx_body_tsv_rum,
  vector_index=idx_embedding_hnsw,
  fusion=WeightedAverage(alpha=0.7),
  limit=10
)

Rules Applied (5):
──────────────────
1. introduce-vector-index-scan
   → Replaced sequential scan with HNSW index scan
   Cost reduction: 5000ms → 50ms (100x speedup)

2. introduce-fts-index-scan
   → Replaced LIKE scan with RUM index scan
   Cost reduction: 8000ms → 80ms (100x speedup)

3. fts-filter-selectivity-based-ordering
   → Chose FTS-first strategy (selectivity: 0.005)
   Reason: FTS is more selective, filters to 500 docs before vector search

4. hybrid-top-k-optimization
   → Combined index scans with direct top-K retrieval
   Avoided materializing full result set

5. score-fusion-optimization
   → Applied weighted average fusion inline during scan
   Avoided separate scoring pass

Optimization Statistics:
────────────────────────
Initial cost:     13,000 ms
Optimized cost:      125 ms
Speedup:          104.0x
Iterations:           12
Rules evaluated:     156
Rules applied:         5
Time elapsed:      15 ms

Cost Breakdown:
───────────────
FTS scan (RUM):          80 ms  (500 matches)
Vector scan (HNSW):      40 ms  (on 500 filtered docs)
Score fusion:             3 ms
Projection:               2 ms
──────────────────────────────
Total:                  125 ms
```

### 3. Show Plan Diff (Before vs After)

```bash
ra-cli optimize examples/hybrid-search-example.sql --diff colored
```

**Expected Output:**
```
Original Plan                           Optimized Plan
─────────────────────────────────────────────────────────────────

TopK(k=10)                              HybridSearchScan
├─ Project                              ├─ strategy: FTSFirst
│  ├─ bm25_score                        │  ├─ fts_index: rum
│  ├─ vector_score                      │  ├─ vector_index: hnsw
│  └─ hybrid_score                      │  ├─ alpha: 0.7
└─ Filter                               │  └─ selectivity: 0.005
   ├─ FTS: @@ query                     ├─ limit: 10
   └─ Vector: distance < 0.5            └─ fusion: WeightedAverage
      └─ Scan(articles)

Cost: 13,000ms → 125ms (104x faster)
```

### 4. Explain with Different Hardware Profiles

```bash
# Mobile device (limited resources)
ra-cli optimize examples/hybrid-search-example.sql \
  --hardware-profile mobile

# GPU-accelerated server
ra-cli optimize examples/hybrid-search-example.sql \
  --hardware-profile gpu-server
```

**Mobile Output:**
```
Strategy: VectorFirst (uses less memory)
- GPU acceleration: not available
- Batch size: 50 (memory-constrained)
- Estimated time: 350ms
```

**GPU Server Output:**
```
Strategy: Parallel (GPU + CPU)
- GPU: Vector search on 768-dim embeddings
- CPU: Full-text search in parallel
- Batch size: 1000
- Estimated time: 45ms
```

### 5. Show All Available Hybrid Search Rules

```bash
ra-cli optimize examples/hybrid-search-example.sql \
  --rules-available \
  | grep -E "(hybrid|vector|fts)"
```

**Expected Output:**
```
Hybrid Search Rules (8):
  • hybrid-strategy-selection
  • hybrid-fts-first-when-selective
  • hybrid-vector-first-when-selective
  • hybrid-parallel-for-small-limits
  • hybrid-score-fusion-optimization

Vector Search Rules (6):
  • introduce-vector-index-scan
  • vector-top-k-optimization
  • vector-pre-filter-when-selective
  • vector-post-filter-when-large
  • hnsw-vs-ivfflat-selection

Full-Text Search Rules (7):
  • introduce-fts-index-scan
  • rum-vs-gin-selection
  • fts-top-k-ranking-optimization
  • skip-list-intersection-for-and
  • phrase-search-with-positions
```

### 6. Use Timeline Snapshot for Real Statistics

```bash
# First, capture a snapshot from your PostgreSQL database
# (requires pg_ra_planner extension installed)
psql -c "SELECT ra.capture_snapshot_to_file('/tmp/production.toml')"

# Then optimize with real production statistics
ra-cli optimize examples/hybrid-search-example.sql \
  --timeline /tmp/production.toml \
  --verbose
```

**Output with Real Stats:**
```
Using snapshot: production-2026-04-06-12:30:00
Database: articles_db
Table stats:
  • articles: 1.2M rows, 15GB
  • Index idx_embedding_hnsw: HNSW(m=16, ef_construction=128)
  • Index idx_body_tsv_rum: RUM (positions enabled)

FTS query: "database & optimization"
  • Estimated matches: 6,400 docs (0.53% selectivity)
  • RUM scan cost: 250ms

Vector query: distance < 0.5
  • Estimated matches: 120,000 docs (10% selectivity)
  • HNSW scan cost: 1,200ms

Strategy chosen: FTS-first
Reason: FTS is 20x more selective (0.53% vs 10%)

Plan: FTS scan → Vector brute-force on 6,400 docs
Cost: 250ms (RUM) + 80ms (brute-force vector on 6.4K) = 330ms

Alternative (Vector-first): 1,200ms (HNSW) + 3,600ms (FTS on 120K) = 4,800ms
Improvement: 14.5x better
```

## Advanced Usage

### Show E-graph Exploration

```bash
ra-cli optimize examples/hybrid-search-example.sql \
  --verbose \
  --rules-evaluated
```

This shows all rules considered during optimization:

```
E-graph iterations:

Iteration 1: (3 rules applied)
  ✓ introduce-vector-index-scan
  ✓ introduce-fts-index-scan
  ✗ join-commutativity (not applicable)

Iteration 2: (2 rules applied)
  ✓ hybrid-strategy-selection
  ✓ vector-pre-filter-optimization
  ✗ fts-phrase-optimization (no phrase search)

Iteration 3: (no rules applied - reached fixpoint)
  ✗ join-associativity (no joins)
  ✗ aggregation-pushdown (no aggregates)

Total: 5 rules applied, 12 evaluated
```

### Export Optimized Plan

```bash
# Export as JSON
ra-cli optimize examples/hybrid-search-example.sql \
  --explain-format json \
  > optimized-plan.json

# Export as GraphML (for visualization)
ra-cli optimize examples/hybrid-search-example.sql \
  --explain-format graphml \
  > plan.graphml

# Visualize with Graphviz
ra-cli optimize examples/hybrid-search-example.sql \
  --explain-format dot \
  | dot -Tpng > plan.png
```

### Resource Budget Control

```bash
# Interactive mode (< 50ms optimization time)
ra-cli optimize examples/hybrid-search-example.sql \
  --resource-budget interactive

# Batch mode (unlimited optimization time)
ra-cli optimize examples/hybrid-search-example.sql \
  --resource-budget batch \
  --max-iterations 1000

# Custom budget
ra-cli optimize examples/hybrid-search-example.sql \
  --max-time 5000 \
  --max-memory 2048 \
  --max-iterations 500
```

## Integration with PostgreSQL

### 1. Enable Ra Extension

```sql
CREATE EXTENSION pg_ra_planner;

-- Configure Ra for your session
SET ra.enabled = true;
SET ra.log_level = 'debug';
```

### 2. Run Query with Ra Planning

```sql
EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)
SELECT
    id,
    title,
    ts_rank(body_tsv, to_tsquery('english', 'database & optimization')) AS bm25_score,
    1 - (embedding <-> '[0.1, 0.2, 0.3]'::vector) AS vector_score
FROM articles
WHERE
    body_tsv @@ to_tsquery('english', 'database & optimization')
    AND embedding <-> '[0.1, 0.2, 0.3]'::vector < 0.5
ORDER BY (0.7 * ts_rank(...) + 0.3 * (1 - (...))) DESC
LIMIT 10;
```

**PostgreSQL will use Ra's optimized plan automatically!**

### 3. Capture Production Snapshot

```sql
-- Capture current state
SELECT ra.capture_snapshot_to_file('/tmp/snapshot.toml');

-- Include all statistics
SELECT ra.capture_snapshot_to_file(
    '/tmp/detailed-snapshot.toml',
    include_indexes := true,
    include_stats := true,
    include_hardware := true
);
```

### 4. Analyze Snapshot with ra-cli

```bash
# Optimize against production snapshot
ra-cli optimize examples/hybrid-search-example.sql \
  --timeline /tmp/snapshot.toml \
  --verbose \
  --stats

# Compare multiple snapshots
ra-cli optimize examples/hybrid-search-example.sql \
  --timeline /tmp/morning-snapshot.toml \
  --diff colored

ra-cli optimize examples/hybrid-search-example.sql \
  --timeline /tmp/evening-snapshot.toml \
  --diff colored
```

## Performance Expectations

Based on our benchmarks:

| Query Type | Naive Plan | Ra Optimized | Speedup |
|------------|------------|--------------|---------|
| Vector only (HNSW) | 5,000ms (seq scan) | 50ms (HNSW) | 100x |
| FTS only (RUM) | 8,000ms (LIKE) | 80ms (RUM) | 100x |
| Hybrid (FTS-first) | 13,000ms | 125ms | 104x |
| Hybrid (wrong order) | 13,000ms | 4,800ms | 2.7x |
| Hybrid (Ra optimized) | 13,000ms | 125ms | 104x |

**Key Insight**: Ra's strategy selection (FTS-first vs Vector-first) provides an additional **38.4x** speedup over naive hybrid search that doesn't consider selectivity.

## See Also

- [Hybrid Search User Guide](../user-guide/hybrid-search.md)
- [Vector Search User Guide](../user-guide/vector-search.md)
- [Full-Text Search User Guide](../user-guide/full-text-search.md)
- [Hybrid Search API Reference](../reference/hybrid-search-api.md)
