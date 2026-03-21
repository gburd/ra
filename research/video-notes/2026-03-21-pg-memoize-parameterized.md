# PostgreSQL v14: Memoize Node for Parameterized Scans

**Source:** PostgreSQL v14 Release Notes and Documentation
**Topic:** Memoize (result caching) for nested loop inner side

## Key Concepts

### Problem
In nested loop joins, the inner side is rescanned for each outer row.
When the outer side has many duplicate values in the join key, the
inner side produces identical results for duplicate keys.

**Without Memoize:**
- Outer has 1M rows, 1K distinct join keys
- Inner scan executed 1M times
- 999K scans produce duplicate results

**With Memoize:**
- First scan for each distinct key: execute and cache result
- Subsequent scans with same key: return cached result
- Only 1K actual scans, 999K cache hits

### Implementation
- LRU cache of (parameter values -> result set) entries
- Cache key: values of all parameterized expressions
- Cache size: bounded by work_mem
- Eviction: LRU when cache is full
- Statistics: cache hits, cache misses, cache evictions reported in EXPLAIN

### When Memoize Helps
1. Nested loop join with many duplicate keys on outer side
2. Parameterized index scan as inner side
3. Low cardinality of outer join key relative to row count
4. Inner scan cost is non-trivial (involves I/O or complex predicates)

### When Memoize Hurts
1. All outer keys are unique (0% hit rate, cache overhead wasted)
2. Inner result set is very large (cache memory pressure)
3. Cache eviction rate is high (working set exceeds work_mem)

### Cost Model
```
hit_ratio = 1 - (n_distinct / outer_rows)  -- estimated cache hit rate
cache_cost = cpu_tuple_cost * outer_rows * miss_ratio * inner_cost
           + cpu_tuple_cost * outer_rows * hit_ratio * cache_lookup_cost
total_cost = outer_cost + cache_cost
```

### EXPLAIN Output Example
```
Nested Loop (rows=100000)
  -> Seq Scan on orders (rows=100000)
  -> Memoize (cache key: orders.customer_id)
       Cache Hits: 95000  Cache Misses: 5000  Evictions: 0
       -> Index Scan on customers (rows=1)
            Index Cond: (id = orders.customer_id)
```

## Applicable to Ra

### New Rule
```
Rule: memoize-insertion
Pattern: Join(NestedLoop, outer=X, inner=ParameterizedScan(params=P))
Condition:
  - estimated hit ratio > threshold (e.g., 0.5)
  - inner result set fits in memory budget
  - inner scan cost > cache lookup cost
Result: Join(NestedLoop, outer=X,
             inner=Memoize(key=P, child=ParameterizedScan(params=P)))
```

### Prerequisites
- Parameterized scan detection
- Distinct count estimation for outer join key
- Memory budget tracking
- New Memoize plan node in algebra

### Impact
- 10x-1000x speedup for skewed nested loop joins
- Common in OLTP workloads (orders -> customers lookup pattern)
- PostgreSQL v14 showed major improvements on TPC-H Q2, Q17, Q20
