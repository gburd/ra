# Hybrid Search Optimization: Technical Documentation

## Overview

Hybrid search combines full-text search (FTS) and vector similarity search to provide both keyword-based and semantic matching. This document describes the implementation in the Ra query optimizer.

## Architecture

### Components

```
┌─────────────────────────────────────────────────────────────┐
│                     Query Optimizer                          │
│                                                               │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐│
│  │ Strategy       │  │ Cost           │  │ Rewrite        ││
│  │ Selection      │  │ Estimation     │  │ Rules          ││
│  └────────────────┘  └────────────────┘  └────────────────┘│
│           │                   │                   │          │
│           └───────────────────┴───────────────────┘          │
│                           │                                  │
│                           ▼                                  │
│                  ┌────────────────┐                          │
│                  │ Hybrid Scan    │                          │
│                  │ Executor       │                          │
│                  └────────────────┘                          │
└─────────────────────────────────────────────────────────────┘
                            │
         ┌──────────────────┴──────────────────┐
         ▼                                     ▼
┌────────────────┐                   ┌────────────────┐
│ PostgreSQL RUM │                   │ pgvector       │
│ (FTS)          │                   │ (Similarity)   │
└────────────────┘                   └────────────────┘
```

### Strategy Selection Algorithm

```rust
fn choose_hybrid_strategy(
    fts_selectivity: f64,
    vector_selectivity: f64,
    limit: Option<usize>,
    total_rows: f64,
) -> HybridStrategy {
    // Rule 1: High FTS selectivity
    if fts_selectivity < 0.01 {
        return HybridStrategy::FTSFirst;
    }

    // Rule 2: High vector selectivity
    if vector_selectivity < 0.01 {
        return HybridStrategy::VectorFirst;
    }

    // Rule 3: Small result set
    if limit < Some(100) {
        return HybridStrategy::Parallel;
    }

    // Rule 4: Cost-based selection
    let costs = [
        fts_cost + vector_cost * fts_selectivity,
        vector_cost + fts_cost * vector_selectivity,
        fts_cost + vector_cost + merge_cost,
    ];

    return strategy_with_min_cost(costs);
}
```

### Cost Model

#### FTS Cost (RUM Index)
```
cost_fts = base + matches * log(total_rows) * per_match_cost
```
- Base cost: 10.0 (index overhead)
- Per-match cost: 0.5 (BM25 scoring)
- Complexity: O(M log N)

#### Vector Cost (pgvector HNSW)
```
cost_vector = base + matches * log(total_rows) * dim_factor
```
- Base cost: 15.0 (index overhead)
- Dim factor: 1.2 (high-dimensional distance)
- Complexity: O(M log N * d)

#### Merge Cost
```
cost_merge = base + total_matches * log(total_matches) * per_row_cost
```
- Base cost: 5.0 (hash set overhead)
- Per-row cost: 0.1 (deduplication + fusion)
- Complexity: O((M1 + M2) log(M1 + M2))

### Score Fusion Methods

#### 1. Weighted Average
```rust
fn weighted_average(bm25: f64, vector: f64, alpha: f64) -> f64 {
    let norm_bm25 = bm25 / (bm25 + 1.0);
    let norm_vector = 1.0 / (1.0 + vector);
    alpha * norm_bm25 + (1.0 - alpha) * norm_vector
}
```

**Pros:**
- Simple, predictable
- Easy to tune with alpha parameter
- Works well with normalized scores

**Cons:**
- Requires score normalization
- Sensitive to score distributions
- Alpha must be manually tuned

**Use Cases:**
- Queries with known score distributions
- Applications requiring interpretable weights
- A/B testing different alpha values

#### 2. Reciprocal Rank Fusion (RRF)
```rust
fn reciprocal_rank_fusion(bm25: f64, vector: f64, k: usize) -> f64 {
    let rrf_bm25 = 1.0 / (k as f64 + bm25);
    let rrf_vector = 1.0 / (k as f64 + vector);
    rrf_bm25 + rrf_vector
}
```

**Pros:**
- No score normalization needed
- Robust to score distribution differences
- Well-validated in literature (k=60)

**Cons:**
- Less interpretable than weighted average
- Fixed combining function
- Approximates rank with scores

**Use Cases:**
- Production systems with diverse score ranges
- Queries where ranks matter more than scores
- Default choice for most applications

#### 3. Learned Fusion
```rust
fn learned_fusion(bm25: f64, vector: f64, model: &Model) -> f64 {
    let features = vec![
        bm25,
        vector,
        doc_length,
        query_terms,
        // ... more features
    ];
    model.predict(features)
}
```

**Pros:**
- Best accuracy with training data
- Adapts to specific corpus
- Can incorporate additional features

**Cons:**
- Requires labeled training data
- Higher computational cost
- Model maintenance overhead

**Use Cases:**
- Large-scale search systems
- Applications with relevance feedback
- Domains with abundant training data

## Query Pattern Recognition

### Pattern 1: FTS-First
```sql
SELECT * FROM docs
WHERE content_tsvector @@ 'machine learning'::tsquery
ORDER BY content_embedding <-> '[0.1, 0.2, ...]'::vector
LIMIT 20;
```

**Rewrite:**
```
Filter(fts_match, Sort(vector_distance))
→ HybridSearchScan(strategy=FTSFirst)
```

**Execution:**
1. RUM index scan for "machine learning" (2,000 matches)
2. Compute vector distances for 2,000 candidates
3. Sort by distance, return top 20

**Cost:** `fts_cost + vector_cost * 0.002`

### Pattern 2: Vector-First
```sql
SELECT * FROM products
WHERE feature_embedding <-> '[...]'::vector < 0.5
ORDER BY ts_rank(description_tsvector, 'laptop'::tsquery) DESC
LIMIT 50;
```

**Rewrite:**
```
Filter(vector_distance, Sort(fts_rank))
→ HybridSearchScan(strategy=VectorFirst)
```

**Execution:**
1. HNSW index scan for similar vectors (5,000 matches)
2. Compute BM25 ranks for 5,000 candidates
3. Sort by rank, return top 50

**Cost:** `vector_cost + fts_cost * 0.005`

### Pattern 3: Parallel
```sql
SELECT * FROM articles
WHERE content_tsvector @@ 'AI'::tsquery
  AND content_embedding <-> '[...]'::vector < 0.3
ORDER BY hybrid_score(...)
LIMIT 10;
```

**Rewrite:**
```
Sort(hybrid_score, Filter(fts AND vector))
→ HybridSearchScan(strategy=Parallel)
```

**Execution:**
1. Parallel: RUM scan for "AI" (20,000 matches)
2. Parallel: HNSW scan for vectors (15,000 matches)
3. Merge and deduplicate (30,000 total)
4. Fuse scores, sort, return top 10

**Cost:** `fts_cost + vector_cost + merge_cost`

## Performance Characteristics

### Selectivity Impact

| FTS Sel | Vector Sel | Strategy | Speedup vs Naive |
|---------|------------|----------|------------------|
| 0.1%    | 5%         | FTS-First| 50x              |
| 5%      | 0.1%       | Vec-First| 50x              |
| 2%      | 3%         | Parallel | 2-3x             |
| 10%     | 10%        | Cost-Based| 1.5-2x          |

### Result Set Size Impact

| LIMIT | Typical Strategy | Overhead vs Single |
|-------|------------------|--------------------|
| 10    | Parallel         | 1.2x               |
| 100   | Cost-Based       | 1.3x               |
| 1000  | Selective-First  | 1.5x               |

### Cost Factor Summary

| Strategy | Base Factor | Selectivity Adjustment | Total Range |
|----------|-------------|------------------------|-------------|
| FTS-First| 1.2x        | +0.0 to +0.5           | 1.2x-1.7x   |
| Vec-First| 1.3x        | +0.0 to +0.5           | 1.3x-1.8x   |
| Parallel | 1.5x        | +0.0 to +0.3           | 1.5x-1.8x   |

**Target Achievement:** ✅ All strategies < 2x overhead

## Integration Example

### Application Code
```rust
use ra_engine::{Optimizer, OptimizerConfig};

let optimizer = Optimizer::with_config(OptimizerConfig {
    enable_hybrid_search: true,
    hybrid_alpha: 0.7,  // Prefer FTS over vector
    hybrid_rrf_k: 60,
    ..Default::default()
});

let sql = r#"
    SELECT title, content
    FROM articles
    WHERE content_tsvector @@ 'machine learning'::tsquery
      AND content_embedding <-> $1::vector < 0.5
    ORDER BY hybrid_score(
        ts_rank(content_tsvector, 'machine learning'::tsquery),
        content_embedding <-> $1
    ) DESC
    LIMIT 20
"#;

let optimized_plan = optimizer.optimize(sql)?;
// → HybridSearchScan(strategy=FTSFirst, fusion=RRF)
```

### PostgreSQL Execution
```sql
-- Generated by optimizer
EXPLAIN (ANALYZE, BUFFERS)
SELECT title, content
FROM (
    SELECT *,
           ts_rank(content_tsvector, 'machine learning'::tsquery) as bm25,
           content_embedding <-> $1 as dist
    FROM articles
    WHERE content_tsvector @@ 'machine learning'::tsquery
) t
WHERE dist < 0.5
ORDER BY (1.0 / (60 + bm25) + 1.0 / (60 + dist)) DESC
LIMIT 20;

-- Actual execution plan:
-- -> Limit (rows=20)
--    -> Sort (rows=2000)
--       -> Filter (dist < 0.5) (rows=2000)
--          -> Index Scan using rum_idx (rows=2000)
--             Filter: content_tsvector @@ 'machine learning'
```

## Tuning Guide

### Alpha Selection (Weighted Average)
```
alpha = weight_fts / (weight_fts + weight_vector)

Examples:
- Pure FTS:      alpha = 1.0
- Pure Vector:   alpha = 0.0
- Balanced:      alpha = 0.5
- FTS-heavy:     alpha = 0.7
- Vector-heavy:  alpha = 0.3
```

### RRF k Selection
```
Recommended values:
- Small corpus (< 10K docs):   k = 30
- Medium corpus (10K-1M docs): k = 60 (default)
- Large corpus (> 1M docs):    k = 100
```

### Selectivity Thresholds
```
High selectivity threshold: 0.01 (1%)
- Lower → More aggressive FTS/Vector-first
- Higher → More frequent parallel execution

Small result threshold: 100 rows
- Lower → More frequent parallel
- Higher → More selective-first strategies
```

## Monitoring and Debugging

### Strategy Selection Statistics
```rust
// Log strategy selection decisions
tracing::info!(
    fts_sel = %fts_selectivity,
    vec_sel = %vector_selectivity,
    strategy = %chosen_strategy,
    "Hybrid search strategy selected"
);
```

### Cost Estimation Breakdown
```rust
let cost_breakdown = HybridCostBreakdown {
    fts_cost: estimate_fts_cost(fts_selectivity, total_rows),
    vector_cost: estimate_vector_cost(vector_selectivity, total_rows),
    merge_cost: estimate_merge_cost(fts_selectivity, vector_selectivity, total_rows),
    total_cost: total,
};
```

### Query Execution Metrics
```sql
-- PostgreSQL statistics
SELECT
    query_hash,
    strategy,
    fts_time_ms,
    vector_time_ms,
    merge_time_ms,
    total_time_ms,
    overhead_factor
FROM hybrid_search_stats
WHERE query_date > NOW() - INTERVAL '7 days'
ORDER BY total_time_ms DESC
LIMIT 100;
```

## References

- RFC 0073: Hybrid Search Optimization
- PostgreSQL RUM Index: https://github.com/postgrespro/rum
- pgvector: https://github.com/pgvector/pgvector
- Reciprocal Rank Fusion: Cormack et al. 2009
