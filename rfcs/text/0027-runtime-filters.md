# RFC 0027: Runtime Filters and Sideways Information Passing

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Accepted
- Tracking Issue: TBD

## Summary

Add runtime filter infrastructure that passes bloom filters and semi-join reducers from hash join build phases to probe-side scan operators, dramatically reducing data volume for selective joins (particularly star schema queries).

## Motivation

RA has no mechanism for passing information between operators during execution. Hash join build phases produce bloom filters that could reduce scan output on the probe side by 10-100x, but this optimization is not modeled. Star schema queries (fact table filtered by dimension joins) suffer because the full fact table is scanned even when only a small fraction of rows match the dimension filters.

This technique is used in production by Snowflake, StarRocks, Spark, Presto, and DataFusion.

## Guide-level explanation

When a hash join builds its hash table from the smaller (build) side, it simultaneously constructs a bloom filter on the join key columns. This bloom filter is pushed down to the scan operator on the larger (probe) side, filtering out rows that cannot possibly match before they enter the join.

```sql
-- Star schema: dimension filter reduces fact table scan
SELECT SUM(f.amount)
FROM fact_sales f
JOIN dim_product p ON f.product_id = p.id
WHERE p.category = 'Electronics';

-- Without runtime filter: full scan of fact_sales
-- With runtime filter: bloom filter on product_id skips ~90% of fact rows
```

## Reference-level explanation

### Implementation Details

New optimization rules:
- `hash-join-bloom-filter-generation`: Generate bloom filter during hash build
- `bloom-filter-pushdown-to-scan`: Push generated filter to scan operators
- `semi-join-reduction-insertion`: Insert semi-join filter before full join
- `dynamic-partition-pruning`: Use runtime filter to eliminate partitions

```rust
pub struct RuntimeFilter {
    pub source_join: JoinId,
    pub build_column: ColumnRef,
    pub probe_column: ColumnRef,
    pub filter_type: RuntimeFilterType,
}

pub enum RuntimeFilterType {
    BloomFilter { fpp: f64, expected_items: usize },
    MinMax { min: Value, max: Value },
    InList { values: Vec<Value>, max_size: usize },
}
```

### Cost Model Extension

- Bloom filter creation cost: O(n_build) with small constant
- Filter evaluation cost: O(1) per probe row
- Selectivity benefit: `1 - (n_matching / n_total)` of probe rows eliminated
- False positive overhead: configurable FPP (default 1%)

### Integration Points

- Hash join operator: generate bloom filter during build phase
- Scan operator: accept and evaluate pushed-down filters
- Partition pruning: use min/max filters to skip entire partitions

## Drawbacks

- Memory overhead for bloom filters (typically 10-100KB per filter)
- False positives add CPU cost without benefit
- Complex interaction with parallel execution
- Not beneficial when probe side is small or join is non-selective

## Rationale and alternatives

### Why This Design?

Bloom filter pushdown is the most widely adopted runtime filter technique, with proven 5-50x improvements for star schema queries in production systems.

### Alternative Approaches

- **Materialized semi-join**: Pre-compute dimension keys; requires maintenance
- **Bitmap index intersection**: Only works with existing indexes
- **Predicate pushdown only**: Static; cannot use runtime information

## Prior art

- Snowflake: Runtime filter documentation
- Apache Spark: Dynamic Partition Pruning (Spark 3.0)
- Presto/Trino: Dynamic filtering
- DataFusion: RuntimeFilter integration
- Bloom, "Space/Time Trade-offs in Hash Coding with Allowable Errors" (1970)

## Unresolved questions

- Optimal bloom filter sizing strategy
- Multi-join filter composition (chaining filters from multiple joins)
- Interaction with adaptive query execution

## Future possibilities

- Range filters for ordered data
- Multi-column bloom filters
- Cross-fragment runtime filters in distributed queries
- Adaptive filter sizing based on execution feedback
