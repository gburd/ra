# Rule: "Columnar Aggregation with Column-at-a-Time Processing"

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/columnar-aggregation.rra`

## Metadata

- **ID:** `columnar-aggregation`
- **Version:** "1.0.0"
- **Databases:** clickhouse, duckdb, monetdb
- **Tags:** aggregation, columnar, column-at-a-time, hash, olap
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(aggregate (scan ?table) ?groups ?aggs)"
    description: "Aggregation on columnar storage"
  - type: "capability"
    database: "current"
    requires: "columnar_storage"
    description: "Table must use columnar storage format"
  - type: "predicate"
    condition: "is_columnar(?table)"
    description: "Table must be stored in columnar format"
```


# Columnar Aggregation with Column-at-a-Time Processing

## Metadata
- **Rule ID**: `columnar-aggregation`
- **Category**: Physical / Aggregation Strategies
- **Source**: ClickHouse `src/Interpreters/Aggregator.cpp`
- **Complexity**: O(n) with cache-friendly access
- **Prerequisites**: Columnar storage; GROUP BY query
- **Alternatives**: Row-at-a-time hash aggregation

## Description

Columnar aggregation processes GROUP BY keys and aggregate inputs as
separate column vectors rather than tuple-at-a-time. The hash table
is built by hashing the GROUP BY key columns (processed as arrays),
then aggregate function states are updated by iterating through the
value columns.

ClickHouse's Aggregator selects specialized hash table implementations
based on key types: single UInt8/16/32/64 keys use direct-mapped
arrays, string keys use StringHashMap, and composite keys use
serialized concatenation. This type-specialization avoids generic
hashing overhead.

The JIT compilation path can fuse multiple aggregate function updates
into a single loop over the input batch, further reducing interpretation
overhead.

**When to apply:**
- GROUP BY queries on columnar storage
- Moderate group cardinality (fits in memory)
- Queries with multiple aggregate functions

**Why it works for OLAP:**
- Column vectors are cache-friendly (sequential access)
- Type-specialized hash tables avoid boxing/unboxing
- Batch processing amortizes function call overhead

## Relational Algebra

```
aggregate[groups, aggs](column-scan[T])
  -> columnar-hash-aggregate[groups, aggs](
       column-vectors(T, groups ∪ agg_cols))
```

## Implementation (egg rewrite rules)

```lisp
;; Use columnar hash aggregation for column-at-a-time input
(rewrite (aggregate ?groups ?aggs (column-scan ?table ?cols))
  (columnar-hash-aggregate ?groups ?aggs
    (column-scan ?table (union ?groups (agg-input-cols ?aggs))))
  :if (< (group-cardinality ?table ?groups) (available-memory-groups)))

;; Specialize for single-key aggregation
(rewrite (columnar-hash-aggregate (list ?key) ?aggs ?input)
  (direct-mapped-aggregate ?key ?aggs ?input)
  :if (is-small-int-type ?key)
  :if (< (distinct-count ?key) 65536))

;; JIT-compile aggregate loop
(rewrite (columnar-hash-aggregate ?groups ?aggs ?input)
  (jit-aggregate ?groups ?aggs ?input)
  :if (> (count ?aggs) 2)
  :if (all-jit-compilable ?aggs))
```

## Cost Model

```rust
pub fn cost_columnar_aggregation(
    input_rows: u64,
    group_card: u64,
    num_aggs: usize,
    key_width: usize,
    hardware: &HardwareModel,
) -> Cost {
    let hash_cost = Cost::cpu(input_rows * 8);
    let update_cost = Cost::cpu(input_rows * num_aggs as u64 * 4);
    let memory = Cost::memory(group_card * (key_width as u64 + num_aggs as u64 * 8));
    let cache_benefit = if memory.bytes() < hardware.cache_size_l3() {
        0.7
    } else {
        1.0
    };
    (hash_cost + update_cost) * cache_benefit + memory
}
```

**Typical benefit**: 30-70% over row-at-a-time aggregation

## Test Cases

### Positive: Low-cardinality GROUP BY
```sql
SELECT status, count(*), sum(amount), avg(amount)
FROM orders
GROUP BY status;

-- status has ~5 values: direct-mapped array (no hashing)
-- Three aggregates computed in single columnar pass
```

### Positive: Multiple aggregates with JIT
```sql
SELECT date, count(*), sum(revenue), sum(cost),
       min(price), max(price), avg(quantity)
FROM sales GROUP BY date;

-- 6 aggregates JIT-compiled into single fused loop
-- Column vectors processed sequentially
```

### Negative: Very high cardinality
```sql
SELECT user_id, count(*) FROM events GROUP BY user_id;

-- 100M distinct users: hash table exceeds L3 cache
-- Random hash table access becomes bottleneck
```

## References

- ClickHouse: `src/Interpreters/Aggregator.cpp`
- ClickHouse: `src/Interpreters/AggregationMethod.h` (type specialization)
- Boncz et al., "MonetDB/X100: Hyper-Pipelining Query Execution", CIDR 2005
