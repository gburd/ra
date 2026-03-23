# Rule: "ClickHouse Partition-Independent Aggregation"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/partition-independent-aggregation.rra`

## Metadata

- **ID:** `clickhouse-partition-independent-aggregation`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** aggregation, partition, data-parallel, independent
- **Authors:** "RA Contributors"


# ClickHouse Partition-Independent Aggregation

## Metadata
- **Rule ID**: `clickhouse-partition-independent-aggregation`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Processors/QueryPlan/Optimizations/useDataParallelAggregation.cpp`
- **Complexity**: O(n/p) per partition with p partitions
- **Prerequisites**: GROUP BY key is a superset of PARTITION BY key
- **Alternatives**: Global aggregation with merge step

## Description

When the GROUP BY key includes all the PARTITION BY columns (or is
derived from them via injective functions), each partition can be
aggregated independently without a final merge step. ClickHouse detects
this by checking that the partition key expression is a deterministic
function of the GROUP BY keys, and that the GROUP BY keys are injective
functions of the partition key columns.

Each partition's data is read through a separate port and aggregated in
isolation. The `skipMerging()` flag tells the aggregation step that no
cross-partition merge is needed, reducing memory and CPU.

**When to apply:**
- GROUP BY includes or implies all PARTITION BY columns
- No GROUPING SETS
- GROUP BY expression is deterministic

**Why it works for OLAP:**
- Partitioned tables common in time-series OLAP
- Eliminates the merge phase (pipeline breaker)
- Enables parallel aggregation across partitions

## Relational Algebra

```
aggregate[groups, aggs](scan[T])
  -> union-all(
       aggregate[groups, aggs](scan[T, partition_1]),
       aggregate[groups, aggs](scan[T, partition_2]),
       ...)
     where partition_key $\subseteq$ groups
```

## Implementation (egg rewrite rules)

```lisp
;; Aggregate partitions independently when GROUP BY covers PARTITION BY
(rewrite (aggregate ?groups ?aggs (scan ?table))
  (union-all
    (map-partitions ?table
      (lambda (?part)
        (aggregate ?groups ?aggs (scan-partition ?table ?part)))))
  :if (partition-key-subset-of-group-by ?table ?groups)
  :if (not (has-grouping-sets ?groups))
  :if (deterministic-group-by ?groups))
```

## Cost Model

```rust
pub fn cost_partition_independent_agg(
    rows_per_partition: u64,
    num_partitions: u64,
    group_card_per_partition: u64,
    agg_count: usize,
    hardware: &HardwareModel,
) -> Cost {
    let per_partition = Cost::cpu(
        rows_per_partition * (10 + agg_count as u64 * 5)
    );
    let memory = Cost::memory(
        group_card_per_partition * agg_count as u64 * 8
    );
    (per_partition + memory) * num_partitions
}
```

**Typical benefit**: 30-70% for partitioned aggregation queries

## Test Cases

### Positive: GROUP BY includes partition key
```sql
CREATE TABLE events (
    date Date, user_id UInt64, event String
) ENGINE = MergeTree
PARTITION BY toYYYYMM(date)
ORDER BY (date, user_id);

SELECT toYYYYMM(date), count() FROM events
GROUP BY toYYYYMM(date);
-- Each monthly partition aggregated independently
-- No merge step needed
```

### Positive: GROUP BY superset of partition key
```sql
SELECT toYYYYMM(date), event, count() FROM events
GROUP BY toYYYYMM(date), event;
-- GROUP BY includes partition function; independent aggregation
```

### Negative: GROUP BY does not cover partition key
```sql
SELECT event, count() FROM events GROUP BY event;
-- Same event may appear in multiple partitions; merge required
```

## References

- ClickHouse: `src/Processors/QueryPlan/Optimizations/useDataParallelAggregation.cpp`
