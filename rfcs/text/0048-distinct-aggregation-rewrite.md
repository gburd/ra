# RFC 0048: Distinct Aggregation Rewrite

- Start Date: 2026-03-22
- Author: RA Team
- Status: Proposed
- Tracking Issue: #TBD

## Summary

Implement rewrite rules that transform queries containing `COUNT(DISTINCT ...)` and multiple distinct aggregations into more efficient plans using grouping sets, expand-aggregate patterns, or split-and-union strategies. This avoids the costly approach of maintaining separate hash tables per distinct aggregation.

## Motivation

Queries with `COUNT(DISTINCT x)` are common in analytics and reporting. When a query contains multiple distinct aggregations, naive execution builds a separate hash table for each one -- each requiring a full pass over the data and its own memory allocation.

For example:

```sql
SELECT
    region,
    COUNT(DISTINCT customer_id) AS unique_customers,
    COUNT(DISTINCT product_id) AS unique_products,
    SUM(amount) AS total_amount
FROM sales
GROUP BY region;
```

Without optimization, this requires three separate aggregation passes (or three concurrent hash tables). With the distinct aggregation rewrite, the optimizer can transform this into a single pass using expanded grouping or into a union of simpler aggregations that can be computed in parallel.

The RA optimizer currently has no specific handling for distinct aggregations. The gap analysis (CHORES.md, Gap #4) identifies this as a missing optimization.

### Specific problems addressed

1. **Multiple COUNT(DISTINCT)**: Queries with 2+ distinct aggregations are common in dashboards and reports. Each additional distinct aggregation roughly doubles the memory and compute cost without optimization.

2. **COUNT(DISTINCT) + regular aggregates**: Mixing `COUNT(DISTINCT x)` with `SUM(y)` or `COUNT(*)` requires careful plan generation to avoid computing the regular aggregates multiple times.

3. **High-cardinality DISTINCT**: When the distinct column has high cardinality relative to the group-by columns, the hash table for DISTINCT elimination becomes large.

4. **Distributed execution**: In distributed settings, `COUNT(DISTINCT x)` cannot be decomposed into simple local/global phases like `COUNT(x)` can. Special handling is needed (the existing `distributed_agg.rs` marks it as non-decomposable).

## Guide-level explanation

The optimizer rewrites distinct aggregation queries into equivalent plans that execute more efficiently.

### Single COUNT(DISTINCT)

```sql
-- Original
SELECT region, COUNT(DISTINCT customer_id) FROM sales GROUP BY region;

-- Rewritten to expand-aggregate pattern
SELECT region, COUNT(customer_id)
FROM (
    SELECT DISTINCT region, customer_id FROM sales
) t
GROUP BY region;
```

The inner `SELECT DISTINCT` deduplicates `(region, customer_id)` pairs, then the outer `COUNT` is a simple (non-distinct) count. This is faster when the distinct set is much smaller than the input, and it enables two-phase distributed aggregation.

### Multiple COUNT(DISTINCT)

```sql
-- Original
SELECT
    region,
    COUNT(DISTINCT customer_id) AS unique_customers,
    COUNT(DISTINCT product_id) AS unique_products
FROM sales
GROUP BY region;

-- Rewritten to union-all strategy
SELECT region,
       SUM(CASE WHEN source = 1 THEN cnt ELSE 0 END) AS unique_customers,
       SUM(CASE WHEN source = 2 THEN cnt ELSE 0 END) AS unique_products
FROM (
    SELECT region, COUNT(*) AS cnt, 1 AS source
    FROM (SELECT DISTINCT region, customer_id FROM sales) t1
    GROUP BY region
    UNION ALL
    SELECT region, COUNT(*) AS cnt, 2 AS source
    FROM (SELECT DISTINCT region, product_id FROM sales) t2
    GROUP BY region
) combined
GROUP BY region;
```

Each distinct aggregation is computed independently, then combined. The individual branches can execute in parallel.

### API Usage

```rust
use ra_engine::distinct_agg::{
    DistinctAggRewriter, DistinctAggStrategy,
};

let rewriter = DistinctAggRewriter::new(&statistics);
let strategy = rewriter.choose_strategy(&aggregate_node);

match strategy {
    DistinctAggStrategy::ExpandAggregate => {
        // Single distinct: push distinct into subquery
    }
    DistinctAggStrategy::UnionAll => {
        // Multiple distinct: split into union branches
    }
    DistinctAggStrategy::GroupingSets => {
        // Use GROUPING SETS for combined computation
    }
    DistinctAggStrategy::NoRewrite => {
        // Keep original (e.g., single non-distinct aggregate)
    }
}
```

## Reference-level explanation

### Implementation Details

#### Strategy Selection

The optimizer selects among four strategies based on the query structure:

| Scenario | Strategy |
|----------|----------|
| Single `COUNT(DISTINCT x)`, no other aggs | Expand-Aggregate |
| Single `COUNT(DISTINCT x)` + regular aggs | Expand-Aggregate with pre-agg |
| Multiple `COUNT(DISTINCT ...)` | Union-All split |
| Multiple distinct + GROUP BY with few groups | Grouping Sets |
| Distinct on low-cardinality column | No rewrite (hash table is small) |

#### Expand-Aggregate Transformation

For a single distinct aggregation:

```
Aggregate([g1, g2], [COUNT(DISTINCT x), SUM(y)])
    Scan(T)

=>

Project([g1, g2, COUNT(x) AS count_distinct_x, SUM(pre_sum_y) AS sum_y])
    Aggregate([g1, g2], [COUNT(x), SUM(pre_sum_y)])
        Aggregate([g1, g2, x], [SUM(y) AS pre_sum_y])  -- pre-aggregate
            Scan(T)
```

The inner aggregate groups by `(g1, g2, x)` to deduplicate x values while pre-aggregating y. The outer aggregate computes the final `COUNT(x)` (now non-distinct) and `SUM(pre_sum_y)`.

#### Union-All Split

For multiple distinct aggregations:

```
Aggregate([g], [COUNT(DISTINCT x), COUNT(DISTINCT y)])
    Scan(T)

=>

Project([g, SUM(cnt_x), SUM(cnt_y)])
    Aggregate([g], [SUM(cnt_x), SUM(cnt_y)])
        UnionAll(
            Project([g, COUNT(*) AS cnt_x, 0 AS cnt_y])
                Aggregate([g], [COUNT(*)])
                    Distinct([g, x])
                        Scan(T)
            ,
            Project([g, 0 AS cnt_x, COUNT(*) AS cnt_y])
                Aggregate([g], [COUNT(*)])
                    Distinct([g, y])
                        Scan(T)
        )
```

#### Grouping Sets Transformation

When the database supports GROUPING SETS (PostgreSQL, Oracle, SQL Server):

```
Aggregate([g], [COUNT(DISTINCT x), COUNT(DISTINCT y)])
    Scan(T)

=>

Project([g,
    COUNT(x) FILTER (WHERE GROUPING(y) = 1),
    COUNT(y) FILTER (WHERE GROUPING(x) = 1)])
    Aggregate(
        GROUPING SETS ((g, x), (g, y)),
        [COUNT(x), COUNT(y)]
    )
        Scan(T)
```

This uses a single pass with grouping sets to compute both distinct counts simultaneously.

#### Data Structures

```rust
/// Strategy for rewriting distinct aggregations.
pub enum DistinctAggStrategy {
    /// Transform COUNT(DISTINCT x) into
    /// COUNT(x) over a pre-deduplicated subquery.
    ExpandAggregate,
    /// Split multiple distinct aggregations into
    /// separate branches combined with UNION ALL.
    UnionAll,
    /// Use GROUPING SETS to compute multiple distinct
    /// aggregations in a single pass.
    GroupingSets,
    /// Keep the original plan (rewrite not beneficial).
    NoRewrite,
}

/// Analysis result for a distinct aggregation node.
pub struct DistinctAggAnalysis {
    /// Number of distinct aggregation expressions.
    pub num_distinct_aggs: usize,
    /// Number of non-distinct aggregation expressions.
    pub num_regular_aggs: usize,
    /// Estimated cardinality of each distinct column.
    pub distinct_cardinalities: Vec<(ColumnRef, u64)>,
    /// Estimated number of groups from GROUP BY.
    pub num_groups: u64,
    /// Input cardinality.
    pub input_rows: u64,
}

/// Rewriter that transforms distinct aggregations.
pub struct DistinctAggRewriter {
    /// Threshold: skip rewrite if distinct cardinality
    /// is below this fraction of input rows.
    pub low_cardinality_threshold: f64,
    /// Whether the target dialect supports GROUPING SETS.
    pub supports_grouping_sets: bool,
    /// Maximum number of UNION ALL branches before
    /// falling back to hash-based execution.
    pub max_union_branches: usize,
}
```

#### Distributed Execution Support

A key benefit of the expand-aggregate transformation is enabling two-phase distributed execution for `COUNT(DISTINCT x)`:

```
-- Without rewrite: not decomposable
COUNT(DISTINCT x) GROUP BY g
  -> Must gather all data to one node

-- With expand-aggregate rewrite: decomposable
COUNT(x) GROUP BY g  -- outer: regular COUNT, decomposable!
    DISTINCT(g, x)   -- inner: can use two-phase distinct
```

The inner `DISTINCT(g, x)` uses local deduplication + global merge (similar to the existing `ThreePhase` strategy in `distributed_agg.rs`). The outer `COUNT(x)` is a regular, fully decomposable aggregation.

### Integration Points

- **E-graph rules** (`ra-engine/src/rewrite.rs`): Add rewrite rules that detect distinct aggregation patterns and introduce equivalent expanded plans.
- **Distributed aggregation** (`ra-core/src/distributed_agg.rs`): The expand-aggregate transformation converts non-decomposable distinct aggregations into decomposable ones, directly integrating with the existing two-phase/three-phase machinery.
- **Cost model** (`ra-engine/src/cost.rs`): Add cost estimates for the expanded plans vs. hash-based distinct aggregation.
- **Dialect translation** (`ra-dialect`): The grouping sets strategy requires dialect support detection.
- **Aggregate pushdown** (`ra-engine/tests/logical_aggregate_pushdown_test.rs`): Distinct aggregation rewrite should run before aggregate pushdown to enable additional pushdown opportunities.

### Error Handling

- **Missing statistics**: Without cardinality estimates for distinct columns, the optimizer conservatively assumes high cardinality and applies the rewrite. This is correct behavior -- the rewrite is never incorrect, only potentially suboptimal.
- **Excessive branches**: If a query has more than `max_union_branches` distinct aggregations, fall back to hash-based execution to avoid plan explosion.

### Performance Considerations

- **Memory**: The expand-aggregate strategy uses a single hash table for deduplication instead of N separate ones. For N=3 distinct aggregations, memory usage is roughly 3x lower.
- **CPU**: The union-all strategy scans the input N times but each scan does less work. With parallel execution, the branches run concurrently.
- **I/O**: The expand-aggregate strategy benefits from sequential scan patterns -- one pass to build the deduplicated intermediate, one pass to aggregate.
- **Distributed**: The primary benefit is enabling two-phase aggregation for distinct counts, reducing network traffic proportional to the reduction ratio.

## Drawbacks

- **Plan size increase**: The union-all strategy creates N copies of the scan operator, potentially increasing plan size and complicating plan caching.
- **Statistics requirements**: Choosing between strategies requires knowing distinct value counts per column, which may not be available without ANALYZE.
- **Regression risk**: Some queries may perform worse with the rewrite if cardinality estimates are wrong. For example, if a column has very few distinct values, the hash-based approach may be faster than the expand-aggregate approach.
- **Interaction with other optimizations**: The rewrite changes the plan structure, which may prevent other optimizations (like join reordering) from finding optimal plans.

## Rationale and alternatives

### Why This Design?

The three-strategy approach (expand-aggregate, union-all, grouping sets) covers the full spectrum of distinct aggregation patterns:

- Expand-aggregate is the simplest and most broadly applicable
- Union-all handles the multiple-distinct case cleanly and enables parallelism
- Grouping sets is the most efficient when supported

This mirrors the approach used in PostgreSQL, Calcite, and Spark.

### Alternative Approaches

1. **Hash-based with multiple hash tables**: Keep the naive approach but optimize hash table implementation (shared hash table with per-column distinct tracking). This avoids plan restructuring but doesn't help with distributed execution.

2. **Approximate distinct counts (HyperLogLog)**: Use HLL for `COUNT(DISTINCT x)` when exact counts are not required. This is orthogonal and can be combined with this RFC.

3. **Sketch-based aggregation**: Use Count-Min Sketch for frequency estimation and combine with HLL for distinct counts. Useful for streaming but adds approximation error.

### Impact of Not Doing This

Without distinct aggregation rewrite:
- Queries with multiple `COUNT(DISTINCT ...)` use 2-3x more memory than necessary
- `COUNT(DISTINCT x)` cannot be computed in two-phase distributed mode, requiring full data centralization
- Dashboard and reporting queries (which heavily use distinct counts) will be slower than competing systems

## Prior art

### Academic Research

- **Larson, P. (2001)**: "Data Reduction by Partial Preaggregation." Describes the expand-aggregate technique for distinct aggregations as a special case of partial preaggregation.

- **Chaudhuri, S. and Shim, K. (1994)**: "Including Group-By in Query Optimization." Foundational work on pushing aggregations down through joins, including handling of DISTINCT.

- **Galindo-Legaria, C. and Joshi, M. (2001)**: "Orthogonal Optimization of Subqueries and Aggregation." Describes how to systematically transform aggregation queries including distinct aggregations.

### Industry Solutions

- **PostgreSQL**: Starting with version 9.6, uses the "sorted grouping" approach for single `COUNT(DISTINCT x)` when the column has an available sort order. For multiple distinct aggregations, PostgreSQL uses separate HashAggregate nodes. PG does not automatically rewrite to the expand-aggregate pattern.

- **MySQL**: Does not optimize multiple distinct aggregations. Each `COUNT(DISTINCT ...)` uses a temporary table for deduplication.

- **Apache Calcite**: Has `AggregateExpandDistinctAggregatesRule` which implements the expand-aggregate and union-all transformations. The rule detects distinct aggregations and rewrites them using GROUPING SETS when possible.

- **Apache Spark**: Implements the expand-aggregate approach. Spark's `RewriteDistinctAggregates` rule transforms `COUNT(DISTINCT x)` into an Expand operator followed by regular aggregation, enabling distributed two-phase execution.

- **DuckDB**: Uses the expand-aggregate approach for distinct aggregations, combined with parallel hash aggregation for the deduplication step.

- **Presto/Trino**: Uses the expand-and-aggregate approach with `GROUPING SETS` for multiple distinct aggregations in a single query. This is one of the key optimizations for Presto's analytics performance.

### What We Can Learn

Calcite and Spark provide well-tested implementations of the expand-aggregate pattern. The key insight from Presto/Trino is that GROUPING SETS can handle multiple distinct aggregations in a single pass, avoiding the N-scan overhead of the union-all approach.

## Unresolved questions

- **Threshold tuning**: What is the right threshold for switching between strategies? Need empirical testing with TPC-H/TPC-DS workloads.

- **Interaction with aggregate pushdown**: Should distinct aggregation rewrite happen before or after aggregate pushdown through joins? The expand-aggregate transformation may enable additional pushdown opportunities.

- **APPROX_COUNT_DISTINCT**: Should this RFC also handle approximate distinct counts as a strategy option, or should that be a separate RFC?

- **Grouping sets support detection**: How should the optimizer detect whether the target execution engine supports GROUPING SETS? This may need integration with the dialect system.

## Future possibilities

### Natural Extensions

- **Approximate distinct counts**: Add HyperLogLog-based `APPROX_COUNT_DISTINCT` as an alternative when exact counts are not required. Many analytics use cases accept 1-2% error for 100x speedup.

- **Distinct-aware partitioning**: In distributed execution, partition data by the distinct column to enable local deduplication without network transfer.

- **Multi-query distinct sharing**: When multiple queries use `COUNT(DISTINCT customer_id)` on the same table, share the deduplication result.

### Long-term Vision

Distinct aggregation rewrite is part of the broader aggregation optimization story that includes partial aggregation (RFC 0049), eager aggregation (RFC 0043), and distributed aggregation strategies (`ra-core/src/distributed_agg.rs`). Together, these optimizations ensure RA produces competitive plans for analytics workloads dominated by aggregation queries.

## Implementation Plan

### Phase 1: Expand-Aggregate Rewrite (Week 1)
- Implement `DistinctAggAnalysis` to detect distinct aggregation patterns
- Implement the expand-aggregate transformation for single `COUNT(DISTINCT x)`
- Handle mixed distinct + regular aggregations
- Add e-graph rewrite rules
- Test with TPC-H Q1, Q16

### Phase 2: Union-All and Grouping Sets (Week 2)
- Implement the union-all split for multiple distinct aggregations
- Implement the grouping sets transformation
- Add strategy selection logic based on statistics
- Integrate with distributed aggregation for two-phase execution
- Test with multi-distinct dashboard queries

### Estimated Effort: 1-2 weeks
