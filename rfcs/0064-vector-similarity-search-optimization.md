# RFC 0064: Vector Similarity Search Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should optimize vector similarity search queries using pgvector (and
documentdb's vector search capability) by understanding approximate
nearest neighbor (ANN) index types, distance operators, pre-filter vs
post-filter strategies, and hybrid search patterns that combine vector
similarity with scalar predicates. Vector search is fundamentally
different from traditional queries: it uses ordering-based access (KNN)
rather than predicate-based filtering, and approximate indexes trade
recall for speed. Ra can improve plan quality by selecting between HNSW
and IVFFlat indexes, choosing pre-filtering vs post-filtering strategies,
and optimizing hybrid vector+scalar queries.

## Motivation

pgvector has become the standard for vector similarity search in
PostgreSQL, powering AI/ML applications, recommendation systems, and
semantic search. DocumentDB also integrates vector search via its
`vector_index_kind_impl.c` module.

Current PostgreSQL planner limitations for vector queries:

**1. No filter strategy selection.** When a vector query has both a
similarity search and scalar filters (e.g., `WHERE category = 'tech'
ORDER BY embedding <-> query_vec LIMIT 10`), the planner must choose:
- **Pre-filter**: Apply scalar filter first, then ANN search on filtered
  set. Better when filter is selective.
- **Post-filter**: Do ANN search first, then filter results. Better when
  filter is not selective.

PostgreSQL's planner does not make this choice optimally because it does
not understand ANN index recall characteristics.

**2. Index type selection.** HNSW and IVFFlat have different tradeoffs:
- HNSW: Higher build cost, better recall, faster queries, more memory
- IVFFlat: Lower build cost, requires training, good for large datasets

The planner does not recommend one over the other based on workload.

**3. Dimension-aware cost model.** Vector operations scale with dimension
count. A 1536-dimension embedding (OpenAI) costs ~10x more to compare
than a 128-dimension embedding. The cost model should reflect this.

## Guide-level explanation

### Vector query pattern recognition

Ra recognizes vector similarity queries by detecting pgvector's distance
operators:

| Operator | Distance Metric | Cost Factor |
|----------|----------------|-------------|
| `<->` | L2 (Euclidean) | dimensions * 0.001 |
| `<#>` | Inner product | dimensions * 0.001 |
| `<=>` | Cosine | dimensions * 0.0015 |

### Hybrid search optimization

For a hybrid query:

```sql
SELECT id, title
FROM articles
WHERE category = 'science'
ORDER BY embedding <-> '[0.1, 0.2, ...]'::vector
LIMIT 10;
```

Ra chooses the filter strategy:

```
IF scalar_selectivity < 0.01 (very selective)
THEN pre-filter: index scan on category, then brute-force vector sort
ELSE IF scalar_selectivity > 0.5 (not selective)
THEN post-filter: HNSW scan, then filter by category
ELSE partition-filter: use partitioned HNSW if available
```

### Index recommendation

Ra recommends vector index types based on workload:

```
IF table_size < 100K rows
THEN no vector index needed (brute force is fast enough)
ELSE IF query_frequency > 100/hour AND recall_requirement > 0.95
THEN HNSW with m=16, ef_construction=200
ELSE IF table_size > 10M rows AND memory is constrained
THEN IVFFlat with lists = sqrt(table_size)
ELSE HNSW with default parameters
```

## Reference-level explanation

### Vector operation cost model

```rust
fn vector_distance_cost(dimensions: usize, metric: DistanceMetric) -> f64 {
    let base_cost = match metric {
        DistanceMetric::L2 => dimensions as f64 * 0.001,
        DistanceMetric::InnerProduct => dimensions as f64 * 0.001,
        DistanceMetric::Cosine => dimensions as f64 * 0.0015,
    };
    base_cost
}

fn hnsw_scan_cost(
    table_size: usize,
    dimensions: usize,
    ef_search: usize,
    k: usize,
) -> f64 {
    // HNSW visits ~ef_search * log2(table_size) nodes
    let nodes_visited =
        ef_search as f64 * (table_size as f64).log2();
    let per_node_cost = vector_distance_cost(dimensions, metric);
    nodes_visited * per_node_cost
}

fn ivfflat_scan_cost(
    table_size: usize,
    dimensions: usize,
    n_lists: usize,
    n_probes: usize,
    k: usize,
) -> f64 {
    // IVFFlat scans n_probes lists, each with ~table_size/n_lists vectors
    let vectors_per_list = table_size as f64 / n_lists as f64;
    let total_vectors = vectors_per_list * n_probes as f64;
    let per_vector_cost = vector_distance_cost(dimensions, metric);
    total_vectors * per_vector_cost
}
```

### Pre-filter vs post-filter decision

```rust
fn choose_filter_strategy(
    scalar_selectivity: f64,
    table_size: usize,
    k: usize,
    has_vector_index: bool,
) -> FilterStrategy {
    let filtered_size =
        (scalar_selectivity * table_size as f64) as usize;

    if !has_vector_index {
        // No vector index: always pre-filter to reduce brute force
        return FilterStrategy::PreFilter;
    }

    if filtered_size < k * 10 {
        // Very selective filter: pre-filter and brute force
        FilterStrategy::PreFilter
    } else if scalar_selectivity > 0.5 {
        // Non-selective filter: use vector index, post-filter
        FilterStrategy::PostFilter
    } else {
        // Middle ground: estimate both strategies and pick cheaper
        let pre_cost = filtered_size as f64
            * vector_distance_cost(dims, metric);
        let post_cost = hnsw_scan_cost(table_size, dims, ef, k)
            + (1.0 - scalar_selectivity)
                * k as f64 * 10.0; // wasted work
        if pre_cost < post_cost {
            FilterStrategy::PreFilter
        } else {
            FilterStrategy::PostFilter
        }
    }
}
```

### Index parameter recommendation

Ra recommends HNSW parameters based on dataset characteristics:

| Dataset Size | m | ef_construction | ef_search | Expected Recall |
|-------------|---|-----------------|-----------|-----------------|
| < 10K | 16 | 64 | 40 | > 0.99 |
| 10K - 100K | 16 | 128 | 80 | > 0.98 |
| 100K - 1M | 24 | 200 | 100 | > 0.97 |
| 1M - 10M | 32 | 256 | 200 | > 0.95 |

For IVFFlat:

| Dataset Size | lists | probes | Expected Recall |
|-------------|-------|--------|-----------------|
| 10K - 100K | 100 | 10 | > 0.95 |
| 100K - 1M | 1000 | 20 | > 0.93 |
| 1M - 10M | 3162 | 50 | > 0.90 |

### DocumentDB vector integration

DocumentDB's vector search uses `vector_index_kind_impl.c` to create
pgvector-compatible indexes. Ra detects documentdb vector indexes through
the documentdb catalog and applies the same optimization rules. The
custom scan node (`custom_query_scan.c`) wraps the vector index scan;
Ra optimizes the inner plan while respecting the custom scan boundary.

### Error handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum VectorError {
    #[error(
        "Vector dimension mismatch: query has {query_dims} dimensions \
         but column has {column_dims}"
    )]
    DimensionMismatch {
        query_dims: usize,
        column_dims: usize,
    },

    #[error(
        "No vector index on {table}.{column}; brute-force scan \
         on {row_count} rows will be slow"
    )]
    MissingIndex {
        table: String,
        column: String,
        row_count: usize,
    },
}
```

## Drawbacks

**Recall estimation uncertainty.** ANN recall depends on data distribution,
which Ra cannot fully characterize without running actual searches. The
recall estimates in the parameter tables are based on typical benchmarks
and may not match all datasets.

**Index build cost.** HNSW index build is expensive (hours for large
datasets). Recommending index creation has a significant cost that Ra
should communicate to users.

**pgvector version dependency.** HNSW was added in pgvector 0.5.0.
Older versions only support IVFFlat. Ra must check the extension version.

## Rationale and alternatives

### Why not rely on pgvector's default parameters

pgvector's default HNSW parameters (m=16, ef_construction=64) are
conservative. For high-recall applications, higher ef_construction
values are needed. Ra can recommend parameters based on the observed
query patterns and dataset size.

### Alternative: external vector search

Some applications use dedicated vector databases (Pinecone, Milvus)
instead of pgvector. Ra's optimization applies only to in-PostgreSQL
vector search, which is increasingly preferred for operational simplicity.

## Prior art

- **Milvus**: Provides automatic index type selection based on dataset
  size and query patterns. Ra applies similar heuristics.
- **Pinecone**: Uses adaptive probing for IVFFlat-like indexes. Ra's
  pre-filter/post-filter strategy is analogous.
- **FAISS**: Facebook's library provides detailed cost models for HNSW
  and IVFFlat. Ra's cost formulas are based on FAISS benchmarks.

## Unresolved questions

1. How to estimate recall without running actual queries? Should Ra
   sample a few queries during ANALYZE?
2. Should Ra recommend pgvector's newer index types (e.g., HNSW with
   quantization) when they become available?
3. How to handle multi-vector queries (e.g., search across multiple
   embedding columns)?

## Future possibilities

- **Quantization-aware optimization**: When pgvector supports product
  quantization, Ra can account for reduced precision in cost estimates.
- **Hybrid re-ranking**: For two-stage retrieval (ANN + exact re-rank),
  Ra can optimize the re-ranking threshold.
- **Cross-modal search**: Optimize queries that combine text search
  (tsvector) with vector similarity in a single ranking function.
