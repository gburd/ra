# RFC 0066: Advanced Index-Aware Planning

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should have deep awareness of PostgreSQL's advanced index types (BRIN,
GiST, SP-GiST, GIN, Bloom) and their interaction with query patterns,
enabling index type recommendation, covering index optimization, partial
index matching, and multi-index bitmap strategies. This RFC consolidates
index-aware optimization rules that span multiple extensions and query
patterns, complementing the extension-specific rules in RFCs 0061-0065.

## Motivation

PostgreSQL supports seven index types, but its planner has limited
ability to recommend which type to use for a given workload. A DBA
familiar with BRIN indexes knows they are ideal for append-only
time-series tables, but PostgreSQL will not suggest BRIN over B-tree.
Similarly, GIN indexes with `jsonb_path_ops` are dramatically more
efficient than standard `jsonb_ops` for containment queries, but the
planner does not recommend the appropriate operator class.

**Key optimization gaps:**

| Index Type | Gap | Impact |
|-----------|-----|--------|
| BRIN | Not recommended for sequential/append-only data | 100x storage savings over B-tree |
| GIN | Wrong operator class for JSONB patterns | 2-10x query speed difference |
| GiST | Not recommended for range type exclusion constraints | Missing constraint enforcement |
| SP-GiST | Never recommended despite advantages for specific data | 20-40% slower KNN on points |
| Bloom | Not considered for multi-column equality | 5-10x smaller than multi B-tree |
| Covering | INCLUDE columns not suggested for index-only scans | 2-5x from avoiding heap fetch |
| Partial | Not matched against WHERE clause patterns | 2-10x smaller index |

## Guide-level explanation

### Index type recommendation engine

Ra analyzes query workloads and table characteristics to recommend
the optimal index type:

```
For each unindexed predicate column:
  1. Classify the access pattern (equality, range, containment, KNN, regex)
  2. Classify the data characteristics (ordered, clustered, multi-valued)
  3. Select candidate index types
  4. Estimate index size and query cost for each candidate
  5. Recommend the best tradeoff
```

### BRIN index detection

BRIN indexes are ideal when data is physically ordered by the indexed
column. Ra detects this by checking correlation:

```sql
SELECT correlation
FROM pg_stats
WHERE tablename = 'my_table' AND attname = 'my_column';
```

If |correlation| > 0.9, BRIN is likely a good choice:

```
IF |correlation| > 0.9
   AND table_size > 100MB
   AND workload is range-scan heavy
   AND no BRIN index exists
THEN recommend:
  CREATE INDEX idx_{table}_{col}_brin
    ON {table} USING BRIN ({col})
    WITH (pages_per_range = 128);
```

### Covering index optimization

When an index-only scan is possible but the query selects columns not
in the index, Ra recommends adding INCLUDE columns:

```
IF query uses index on (a, b)
   AND query selects columns (a, b, c, d)
   AND c, d are not in any index
   AND visibility map coverage > 80%
THEN recommend:
  CREATE INDEX idx_{table}_{cols}_covering
    ON {table} (a, b) INCLUDE (c, d);
```

### Partial index matching

Ra detects when a query's WHERE clause matches a partial index:

```sql
-- Partial index:
CREATE INDEX idx_orders_active
  ON orders (customer_id)
  WHERE status = 'active';

-- Query:
SELECT * FROM orders
WHERE customer_id = 123 AND status = 'active';
-- Ra recognizes the partial index match
```

And recommends creating partial indexes for common filter patterns:

```
IF query has repeated WHERE clause (e.g., status = 'active')
   AND this predicate appears in > 50% of queries
   AND it filters out > 50% of rows
THEN recommend partial index
```

### Multi-index bitmap strategies

When no single index covers all predicates, Ra evaluates bitmap AND/OR
strategies:

```sql
-- Query with multiple predicates
SELECT * FROM products
WHERE category = 'electronics'
  AND price BETWEEN 100 AND 500
  AND rating > 4.0;
```

Ra evaluates:
1. Single B-tree on (category, price, rating) -- most efficient if
   all three are common
2. Bitmap AND of separate indexes -- flexible, works with existing indexes
3. Bloom index on (category, price, rating) -- smallest, equality only
4. Compound GIN via btree_gin -- if btree_gin is installed

## Reference-level explanation

### Index type cost model

```rust
struct IndexTypeProfile {
    /// Storage overhead relative to table size
    size_ratio: f64,
    /// Lookup cost per row
    lookup_cost: f64,
    /// Range scan cost per row
    range_cost: f64,
    /// Supports index-only scan
    supports_ios: bool,
    /// Supports ordering
    supports_order: bool,
    /// Build cost relative to table scan
    build_cost: f64,
    /// Insert cost per row (for write-heavy workloads)
    insert_cost: f64,
}

const INDEX_PROFILES: &[(&str, IndexTypeProfile)] = &[
    ("btree", IndexTypeProfile {
        size_ratio: 0.3,
        lookup_cost: 2.0,
        range_cost: 0.1,
        supports_ios: true,
        supports_order: true,
        build_cost: 3.0,
        insert_cost: 0.5,
    }),
    ("brin", IndexTypeProfile {
        size_ratio: 0.001,  // tiny
        lookup_cost: 1.0,
        range_cost: 0.05,
        supports_ios: false,
        supports_order: false,
        build_cost: 1.0,
        insert_cost: 0.01,
    }),
    ("gin", IndexTypeProfile {
        size_ratio: 1.0,    // can be larger than table
        lookup_cost: 3.0,
        range_cost: 0.5,
        supports_ios: false,
        supports_order: false,
        build_cost: 5.0,
        insert_cost: 2.0,
    }),
    ("gist", IndexTypeProfile {
        size_ratio: 0.4,
        lookup_cost: 5.0,
        range_cost: 1.0,
        supports_ios: false,
        supports_order: true, // via KNN
        build_cost: 4.0,
        insert_cost: 1.0,
    }),
    ("spgist", IndexTypeProfile {
        size_ratio: 0.35,
        lookup_cost: 4.0,
        range_cost: 0.8,
        supports_ios: false,
        supports_order: true, // via KNN
        build_cost: 3.5,
        insert_cost: 0.8,
    }),
    ("bloom", IndexTypeProfile {
        size_ratio: 0.05,
        lookup_cost: 1.5,
        range_cost: 0.0,  // no range support
        supports_ios: false,
        supports_order: false,
        build_cost: 2.0,
        insert_cost: 0.1,
    }),
];
```

### BRIN effectiveness estimation

BRIN effectiveness depends on physical correlation between column values
and tuple position:

```rust
fn estimate_brin_effectiveness(
    correlation: f64,
    pages_per_range: u32,
    table_pages: u64,
    selectivity: f64,
) -> f64 {
    // BRIN scans ranges, not individual tuples
    let n_ranges = table_pages / pages_per_range as u64;

    // With perfect correlation, selectivity directly maps to ranges
    // With no correlation, all ranges must be scanned
    let range_selectivity =
        selectivity * correlation.abs()
        + (1.0 - correlation.abs()); // worst case: all ranges

    let scanned_ranges =
        (n_ranges as f64 * range_selectivity).ceil() as u64;
    let scanned_pages =
        scanned_ranges * pages_per_range as u64;

    // Effectiveness = fraction of table NOT scanned
    1.0 - (scanned_pages as f64 / table_pages as f64)
}
```

### GIN operator class recommendation

For JSONB columns, the operator class dramatically affects performance:

| Operator Class | Supports | Size | Speed |
|---------------|----------|------|-------|
| `jsonb_ops` (default) | `?`, `?\|`, `?&`, `@>` | Larger | General |
| `jsonb_path_ops` | `@>` only | 2-3x smaller | 2-3x faster for `@>` |

Ra recommends:
```
IF all JSONB queries use only @> (containment)
THEN recommend jsonb_path_ops
ELSE recommend jsonb_ops
```

### Covering index analysis

Ra identifies index-only scan opportunities:

```rust
fn suggest_covering_columns(
    index: &IndexDef,
    query_columns: &[String],
    table_stats: &TableStats,
) -> Vec<String> {
    // Columns needed by query but not in index
    let missing: Vec<_> = query_columns
        .iter()
        .filter(|c| !index.columns.contains(c))
        .collect();

    if missing.is_empty() {
        return vec![]; // Already covering
    }

    // Only suggest if visibility map is largely set
    // (index-only scan requires all-visible pages)
    if table_stats.visible_ratio < 0.8 {
        return vec![]; // Too many dead tuples
    }

    // Only suggest for small columns (avoid bloating index)
    missing
        .into_iter()
        .filter(|c| table_stats.avg_width(c) < 64)
        .cloned()
        .collect()
}
```

### Partial index detection

Ra tracks repeated WHERE clause patterns across queries:

```rust
struct PartialIndexCandidate {
    table: String,
    predicate: String,
    /// What fraction of queries include this predicate
    query_coverage: f64,
    /// What fraction of rows satisfy this predicate
    row_selectivity: f64,
    /// Estimated index size reduction
    size_reduction: f64,
}

fn evaluate_partial_index(
    candidate: &PartialIndexCandidate,
) -> bool {
    // Recommend partial index when:
    // 1. Predicate appears in most queries on this table
    // 2. Predicate is selective (filters many rows)
    // 3. Size reduction is significant
    candidate.query_coverage > 0.5
        && candidate.row_selectivity < 0.5
        && candidate.size_reduction > 0.3
}
```

### Bitmap index scan strategy

When multiple indexes exist on a table, Ra evaluates whether bitmap
AND/OR is cheaper than a single index scan:

```
bitmap_and_cost =
    sum(index_scan_cost for each index)
  + bitmap_and_merge_cost
  + matching_rows * heap_fetch_cost

single_index_cost =
    best_single_index_scan_cost
  + best_selectivity * total_rows * filter_cost  -- recheck others
```

Ra recommends bitmap AND when:
```
IF bitmap_and_cost < single_index_cost
   AND all predicates have usable indexes
THEN plan: BitmapAnd(IndexScan1, IndexScan2, ...) -> BitmapHeapScan
```

### Integration with other RFCs

- **RFC 0021 (Index Advisor)**: This RFC provides the index type selection
  logic that the advisor uses.
- **RFC 0039 (Operator Class Aware Indexing)**: Complementary RFC for
  B-tree operator class selection.
- **RFC 0061 (Extension-Aware Optimization)**: Extension-specific index
  types (PostGIS GiST, pg_trgm GIN, bloom) are recommended through
  this framework.
- **RFC 0018 (Bitmap Index Scan)**: This RFC extends bitmap strategies
  with multi-index awareness.

## Drawbacks

**Index recommendation complexity.** Evaluating all possible index
configurations for a workload is combinatorial. Ra must use heuristics
to prune the search space.

**Write amplification.** Every recommended index increases write cost.
Ra should model the write amplification and include it in
recommendations.

**Statistics dependency.** BRIN effectiveness depends on correlation
statistics that may be stale. Recommendations based on stale statistics
may be suboptimal.

## Rationale and alternatives

### Why a unified index framework

Each extension (PostGIS, pgvector, documentdb, pg_trgm) recommends its
own index types. A unified framework avoids duplicate logic and ensures
consistent cost modeling across all index types.

### Alternative: per-extension index recommendation

The alternative is to have each extension RFC handle its own index
recommendations independently. This was rejected because:
- Cost comparison across index types requires a common framework
- Bloom vs GIN vs multi-B-tree tradeoffs span extension boundaries
- Covering index and partial index logic is universal

## Prior art

- **Oracle Auto Index**: Automatically creates and drops indexes based
  on workload analysis. Uses a similar cost-benefit framework.
- **SQL Server Database Engine Tuning Advisor**: Recommends indexes,
  indexed views, and partitioning based on workload.
- **Dexter (PostgreSQL)**: Hypothetical index analysis using
  HypoPG. Ra could integrate with HypoPG for what-if analysis.

## Unresolved questions

1. Should Ra integrate with HypoPG for hypothetical index evaluation?
2. How to handle index maintenance cost in long-running OLTP workloads?
3. Should Ra recommend dropping unused indexes?

## Future possibilities

- **Automatic index lifecycle management**: Create, monitor, and drop
  indexes based on workload changes.
- **Index compression recommendation**: For PostgreSQL 16+ with
  deduplication, recommend B-tree deduplication for low-cardinality
  columns.
- **Workload-aware index selection**: Use pg_stat_statements to track
  which queries benefit from which indexes and adjust recommendations.
