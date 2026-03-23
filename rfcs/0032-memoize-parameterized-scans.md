# RFC 0032: Memoize for Parameterized Scans

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Accepted
- Tracking Issue: TBD

## Summary

Add a Memoize plan node that caches results of parameterized inner scans in nested loop joins, avoiding redundant rescans when outer join key values repeat. This provides 10-1000x speedup for joins with low cardinality outer keys.

## Motivation

In nested loop joins, the inner side is rescanned for each outer row. When outer join key values repeat (common in OLTP: orders -> customers, line_items -> products), the same inner scan produces identical results. PostgreSQL's Memoize node (v14) caches these results with an LRU cache.

Without memoization:
- 1M orders joining to 10K customers: inner scan executes 1M times
- With memoization: inner scan executes only 10K times (100x reduction)

## Guide-level explanation

```sql
-- OLTP pattern: many orders per customer
SELECT o.id, c.name
FROM orders o
JOIN customers c ON o.customer_id = c.id;

-- Plan without Memoize:
-- Nested Loop
--   -> Seq Scan on orders (1M rows)
--   -> Index Scan on customers (executed 1M times)

-- Plan with Memoize:
-- Nested Loop
--   -> Seq Scan on orders (1M rows)
--   -> Memoize (key: customer_id, hits: 990K, misses: 10K)
--     -> Index Scan on customers (executed 10K times)
```

## Reference-level explanation

### Implementation Details

```rust
pub struct Memoize {
    pub cache_key: Vec<ColumnRef>,
    pub child: Box<PlanNode>,
    pub cache_capacity: usize,
}

pub struct MemoizeState {
    pub cache: LruCache<Vec<Value>, Vec<Row>>,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}
```

### Memoize Insertion Rule

Rule: `memoize-parameterized-scan`
- Pattern: `NestedLoop(outer, ParameterizedScan(params, child))`
- Condition: estimated hit ratio > 0.5
- Hit ratio: `1 - (n_distinct_join_key / outer_rows)`
- Result: `NestedLoop(outer, Memoize(key=params, child))`

### Cost Model

```
miss_cost = inner_scan_cost  (per distinct key)
hit_cost  = cache_lookup_cost  (near-zero CPU, zero I/O)
total     = outer_rows * (miss_ratio * miss_cost + hit_ratio * hit_cost)
memory    = cache_entry_size * min(n_distinct, cache_capacity)
```

### Cache Sizing

- Default capacity: `work_mem / avg_result_set_size`
- When cache fills, LRU eviction
- If n_distinct > capacity, effective hit ratio decreases
- Cost model accounts for evictions

## Drawbacks

- Memory overhead for cache entries
- Cache management adds per-tuple CPU overhead
- Ineffective when outer key values are mostly unique
- Cache invalidation complexity if underlying data changes mid-query

## Rationale and alternatives

### Why This Design?

LRU caching of parameterized scan results is the standard approach, proven in PostgreSQL v14 with TPC-H improvements. The design is simple and the benefit/cost ratio is high.

### Alternative Approaches

- **Hash join instead**: Not always possible (e.g., non-equi conditions, LATERAL)
- **Materialized CTE**: Requires query rewriting; not transparent
- **Batch key lookups**: More efficient for index scans but more complex

## Prior art

- PostgreSQL v14: Memoize node
- PostgreSQL EXPLAIN output: Cache Hits/Misses/Evictions/Peak Memory
- Oracle: RESULT_CACHE hint for function results
- SQL Server: Adaptive Memory Grant for cached subqueries

## Unresolved questions

- Optimal cache eviction policy (LRU vs LFU vs ARC)
- Interaction with parallel nested loop joins
- Cache sharing between multiple Memoize nodes in the same query

## Future possibilities

- Memoize for scalar subqueries
- Cross-query result caching
- Adaptive cache sizing based on observed hit rate
- Shared cache for correlated subqueries
