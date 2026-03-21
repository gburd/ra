# Rule: "ClickHouse Read-in-Order Sort Elimination"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/read-in-order-sort-elimination.rra`

## Metadata

- **ID:** `clickhouse-read-in-order-sort-elimination`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** sort, elimination, order-by, mergetree, sort-key
- **Authors:** "RA Contributors"


# ClickHouse Read-in-Order Sort Elimination

## Metadata
- **Rule ID**: `clickhouse-read-in-order-sort-elimination`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Processors/QueryPlan/Optimizations/optimizeReadInOrder.cpp`
- **Complexity**: O(n) merge vs O(n log n) sort
- **Prerequisites**: ORDER BY matches prefix of MergeTree sorting key
- **Alternatives**: Full sort (materialize + quicksort/merge sort)

## Description

When a query's ORDER BY clause matches a prefix of the MergeTree table's
sorting key, ClickHouse reads data parts in key order and uses a merge
operation instead of a full sort. Each data part is already internally
sorted, so a k-way merge of parts produces globally sorted output in
O(n) time instead of O(n log n).

The optimizer also handles reverse order (DESC when key is ASC), partial
prefixes, and window functions that can reuse storage ordering.

**When to apply:**
- ORDER BY matches prefix of table's ORDER BY key
- Window functions with PARTITION BY / ORDER BY on sort key columns
- DISTINCT on sort key prefix

**Why it works for OLAP:**
- MergeTree data is physically sorted within parts
- Merge is streaming: no materialization needed
- Eliminates the sort pipeline breaker

## Relational Algebra

```
sort[key_prefix](scan[T])
  -> merge-sorted(read-in-order[T, parts])
     where T.sort_key starts with key_prefix
```

## Implementation (egg rewrite rules)

```lisp
;; Eliminate sort when ORDER BY matches sort key prefix
(rewrite (sort ?keys (scan ?table))
  (merge-sorted (read-in-order ?table ?keys))
  :if (is-prefix-of ?keys (sort-key ?table)))

;; Reverse read for DESC order
(rewrite (sort (desc ?keys) (scan ?table))
  (merge-sorted (read-in-reverse-order ?table ?keys))
  :if (is-prefix-of ?keys (sort-key ?table)))

;; Reuse storage ordering for window functions
(rewrite (window ?partition-by ?order-by ?fn (scan ?table))
  (window ?partition-by ?order-by ?fn
    (merge-sorted (read-in-order ?table
      (concat ?partition-by ?order-by))))
  :if (is-prefix-of (concat ?partition-by ?order-by) (sort-key ?table)))
```

## Cost Model

```rust
pub fn cost_read_in_order(
    total_rows: u64,
    num_parts: u64,
    hardware: &HardwareModel,
) -> Cost {
    let read_cost = Cost::io(
        total_rows as f64 * hardware.seq_read_cost()
    );
    let merge_cost = Cost::cpu(total_rows * (num_parts as f64).log2() as u64);
    read_cost + merge_cost
}
```

**Typical benefit**: 40-95% for large sorted scans (eliminates O(n log n) sort)

## Test Cases

### Positive: ORDER BY matches sort key
```sql
CREATE TABLE events (
    date Date, user_id UInt64, event String
) ENGINE = MergeTree ORDER BY (date, user_id);

SELECT * FROM events ORDER BY date, user_id LIMIT 1000;
-- Read in order: no sort needed, streaming merge of parts
```

### Positive: Prefix match
```sql
SELECT * FROM events ORDER BY date LIMIT 100;
-- date is prefix of (date, user_id); read-in-order applies
```

### Negative: Non-prefix column
```sql
SELECT * FROM events ORDER BY event;
-- event is not in sort key; full sort required
```

## References

- ClickHouse: `src/Processors/QueryPlan/Optimizations/optimizeReadInOrder.cpp`
- ClickHouse: `src/Storages/ReadInOrderOptimizer.cpp`
