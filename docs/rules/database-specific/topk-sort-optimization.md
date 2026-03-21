# Rule: "ClickHouse Top-K Sort Optimization"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/topk-sort-optimization.rra`

## Metadata

- **ID:** `clickhouse-topk-sort-optimization`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** topk, sort, limit, order-by, dynamic-filter
- **Authors:** "RA Contributors"


# ClickHouse Top-K Sort Optimization

## Metadata
- **Rule ID**: `clickhouse-topk-sort-optimization`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Processors/QueryPlan/Optimizations/optimizeTopK.cpp`
- **Complexity**: O(n) with early termination
- **Prerequisites**: ORDER BY + LIMIT on numeric column; MergeTree table
- **Alternatives**: Full sort + limit

## Description

For ORDER BY ... LIMIT N queries on MergeTree tables, ClickHouse injects
a TopK threshold filter into the scan pipeline. As the top-N heap fills,
the threshold tracker maintains the current K-th value. Subsequent granules
are evaluated against this threshold using skip indexes or PREWHERE, and
granules that cannot contain values better than the current K-th are
skipped.

The optimization requires numeric, non-nullable sort columns. It converts
a SORT(LIMIT(N)) into a streaming top-K with progressive filtering.

**When to apply:**
- ORDER BY col LIMIT N with small N (typically < 10000)
- Sort column is numeric and non-nullable
- MergeTree table (for granule-level skipping)

**Why it works for OLAP:**
- OLAP often has "top N" dashboard queries
- Progressive threshold avoids reading most data
- Skip indexes amplify the benefit at granule level

## Relational Algebra

```
limit[N](sort[col](scan[T]))
  -> topk[N, col](
       threshold-filter[col, dynamic_threshold](scan[T]))
```

## Implementation (egg rewrite rules)

```lisp
;; Convert SORT+LIMIT to Top-K with dynamic filtering
(rewrite (limit ?n (sort ?key (scan ?table)))
  (topk ?n ?key
    (threshold-filter ?key (topk-tracker ?n ?key)
      (scan ?table)))
  :if (is-mergetree-table ?table)
  :if (< ?n 10000)
  :if (is-numeric-non-nullable ?key))

;; Top-K with existing filter
(rewrite (limit ?n (sort ?key (filter ?pred (scan ?table))))
  (topk ?n ?key
    (threshold-filter ?key (topk-tracker ?n ?key)
      (filter ?pred (scan ?table))))
  :if (is-mergetree-table ?table)
  :if (< ?n 10000)
  :if (is-numeric-non-nullable ?key))
```

## Cost Model

```rust
pub fn cost_topk(
    total_rows: u64,
    k: u64,
    num_granules: u64,
    hardware: &HardwareModel,
) -> Cost {
    let initial_fill = Cost::cpu(k * 20);
    let threshold_checks = Cost::cpu(num_granules * 5);
    let scanned_fraction = (k as f64 / total_rows as f64).sqrt().min(1.0);
    let io_cost = Cost::io(
        total_rows as f64 * scanned_fraction * hardware.seq_read_cost()
    );
    initial_fill + threshold_checks + io_cost
}
```

**Typical benefit**: 50-95% for small LIMIT on large tables

## Test Cases

### Positive: Small LIMIT on large table
```sql
SELECT * FROM trades ORDER BY price DESC LIMIT 10;

-- Threshold filter: after first few granules, threshold = ~highest prices
-- Remaining granules with max(price) < threshold are skipped
-- Reads ~1% of data for 10 results from 1B rows
```

### Negative: Large LIMIT
```sql
SELECT * FROM trades ORDER BY price DESC LIMIT 1000000;

-- Threshold not selective enough; most granules must be read
-- Overhead of threshold tracking not worth it
```

### Negative: Non-numeric sort key
```sql
SELECT * FROM users ORDER BY name LIMIT 10;

-- String comparison; threshold filtering not supported
-- Falls back to standard sort + limit
```

## References

- ClickHouse: `src/Processors/QueryPlan/Optimizations/optimizeTopK.cpp`
