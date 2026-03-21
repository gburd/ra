# Rule: "ClickHouse Sparse Index Granule Pruning"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/sparse-index-granule-pruning.rra`

## Metadata

- **ID:** `clickhouse-sparse-index-granule-pruning`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** index, sparse, mergetree, primary-key, granule, pruning
- **Authors:** "RA Contributors"


# ClickHouse Sparse Index Granule Pruning

## Metadata
- **Rule ID**: `clickhouse-sparse-index-granule-pruning`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Storages/MergeTree/KeyCondition.cpp`
- **Complexity**: O(log n) index lookup + O(k) granule reads
- **Prerequisites**: MergeTree table with ORDER BY clause; condition on prefix of sort key
- **Alternatives**: Full table scan, skip indexes

## Description

MergeTree stores data sorted by the primary key and maintains a sparse index
that records the min/max value of the primary key for each granule (typically
8192 rows). When a query filters on a prefix of the sort key, ClickHouse uses
the KeyCondition evaluator to convert the predicate into Reverse Polish
Notation (RPN), then evaluates it against the sparse index to identify which
granules may contain matching rows.

The KeyCondition supports equality, range, IN-set, and monotonic function
transformations. It handles space-filling curves (Morton/Hilbert encoding)
for multi-dimensional range queries. Non-matching granules are skipped
entirely, avoiding both I/O and CPU.

**When to apply:**
- Filter on leading columns of ORDER BY key
- Point lookups or range scans on sort key
- Monotonic transformations of sort key columns (e.g., toDate(timestamp))

**Why it works for OLAP:**
- Sparse index is tiny (one entry per 8192 rows)
- Entire granules skipped = massive I/O savings
- Sorted data enables range-based pruning

## Relational Algebra

```
filter[pk_pred](scan[T])
  -> granule-scan[T, matching-granules(pk_pred, sparse-index(T))]
```

## Implementation (egg rewrite rules)

```lisp
;; Use sparse index for primary key prefix condition
(rewrite (filter ?pred (scan ?table))
  (granule-scan ?table
    (index-lookup (sparse-index ?table) ?pred))
  :if (is-mergetree-table ?table)
  :if (matches-sort-key-prefix ?pred (sort-key ?table)))

;; Monotonic function on sort key still uses index
(rewrite (filter (= (monotonic-fn ?col) ?val) (scan ?table))
  (granule-scan ?table
    (index-lookup (sparse-index ?table)
      (range ?col (inverse-lower ?val) (inverse-upper ?val))))
  :if (is-sort-key-column ?col ?table)
  :if (is-monotonic ?monotonic-fn))

;; Combine sparse index with PREWHERE
(rewrite (prewhere ?pred (granule-scan ?table ?granules))
  (prewhere ?pred (granule-scan ?table ?granules)))
```

## Cost Model

```rust
pub fn cost_sparse_index_scan(
    total_granules: u64,
    matching_granules: u64,
    granule_size: u64,
    column_bytes_per_row: u64,
    hardware: &HardwareModel,
) -> Cost {
    let index_cost = Cost::cpu(total_granules * 10);
    let io_cost = Cost::io(
        matching_granules as f64 * granule_size as f64
        * column_bytes_per_row as f64
        * hardware.seq_read_cost()
    );
    index_cost + io_cost
}
```

**Typical benefit**: 50-99% granule elimination for point/range queries

## Test Cases

### Positive: Equality on sort key
```sql
CREATE TABLE events (
    date Date, user_id UInt64, event String
) ENGINE = MergeTree ORDER BY (date, user_id);

SELECT * FROM events WHERE date = '2024-01-15';
-- Sparse index prunes to granules containing 2024-01-15
-- Skips all other dates entirely
```

### Positive: Range on sort key prefix
```sql
SELECT * FROM events
WHERE date BETWEEN '2024-01-01' AND '2024-01-31';
-- Contiguous granule range identified via sparse index
```

### Positive: Monotonic function on sort key
```sql
SELECT * FROM events WHERE toMonth(date) = 3;
-- toMonth is monotonic; ClickHouse inverts to date range
```

### Negative: Non-prefix column
```sql
SELECT * FROM events WHERE event = 'click';
-- event is not in sort key prefix; sparse index cannot help
-- Falls back to full scan (or skip index if defined)
```

## References

- ClickHouse: `src/Storages/MergeTree/KeyCondition.cpp`
- ClickHouse: `src/Processors/QueryPlan/Optimizations/optimizePrimaryKeyConditionAndLimit.cpp`
- Yandex, "ClickHouse: MergeTree Primary Key and Index"
