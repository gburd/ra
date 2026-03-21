# Rule: "ClickHouse Lazy Column Materialization"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/lazy-materialization.rra`

## Metadata

- **ID:** `clickhouse-lazy-materialization`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** lazy, materialization, columnar, wide-table, late-read
- **Authors:** "RA Contributors"


# ClickHouse Lazy Column Materialization

## Metadata
- **Rule ID**: `clickhouse-lazy-materialization`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Processors/QueryPlan/Optimizations/optimizeLazyMaterialization.cpp`
- **Complexity**: O(n) with reduced I/O
- **Prerequisites**: MergeTree table; not FINAL; not sampling; filter or limit present
- **Alternatives**: Eager materialization (read all columns upfront)

## Description

Lazy materialization delays reading columns that are not needed for
filtering or sorting until the final result assembly. After PREWHERE
and WHERE eliminate rows, only surviving rows need the remaining columns
materialized. For LIMIT queries, this is even more effective since only
a small fraction of rows need full materialization.

ClickHouse implements this by splitting the ReadFromMergeTree step into
two phases: a "lazy" read that fetches only filter/sort columns and
produces row positions, then a "join" step that fetches remaining columns
only for the surviving row positions.

**When to apply:**
- Wide tables with many columns
- Queries with selective filters
- LIMIT queries (ORDER BY ... LIMIT N)
- Queries selecting few columns from many

**Why it works for OLAP:**
- OLAP tables often have 50-200+ columns
- Analytical queries typically filter on 1-3 columns
- Reading column data is the dominant cost

## Relational Algebra

```
project[out_cols](filter[pred](scan[T, all_cols]))
  -> join-lazy-cols(
       filter[pred](lazy-scan[T, pred_cols]),
       lazy-read[T, out_cols - pred_cols])
```

## Implementation (egg rewrite rules)

```lisp
;; Lazy materialization for filter queries
(rewrite (project ?out-cols
           (filter ?pred (scan ?table ?all-cols)))
  (join-lazy-columns
    (filter ?pred (lazy-scan ?table (pred-cols ?pred)))
    (lazy-read ?table (diff ?out-cols (pred-cols ?pred))))
  :if (is-mergetree-table ?table)
  :if (not (is-final-query))
  :if (> (columns-size (diff ?out-cols (pred-cols ?pred)))
         (* 2 (columns-size (pred-cols ?pred)))))

;; Lazy materialization for ORDER BY ... LIMIT
(rewrite (limit ?n
           (sort ?keys
             (scan ?table ?all-cols)))
  (join-lazy-columns
    (limit ?n
      (sort ?keys (lazy-scan ?table (sort-cols ?keys))))
    (lazy-read ?table (diff ?all-cols (sort-cols ?keys))))
  :if (is-mergetree-table ?table)
  :if (< ?n (* 0.01 (row-count ?table))))
```

## Cost Model

```rust
pub fn cost_lazy_materialization(
    rows: u64,
    filter_col_bytes: u64,
    remaining_col_bytes: u64,
    selectivity: f64,
    hardware: &HardwareModel,
) -> Cost {
    let phase1_io = Cost::io(filter_col_bytes as f64 * hardware.seq_read_cost());
    let phase1_cpu = Cost::cpu(rows * 8);
    let surviving = (rows as f64 * selectivity) as u64;
    let phase2_io = Cost::io(
        remaining_col_bytes as f64 * selectivity * hardware.random_read_cost()
    );
    let join_cpu = Cost::cpu(surviving * 3);
    phase1_io + phase1_cpu + phase2_io + join_cpu
}
```

**Typical benefit**: 20-80% I/O reduction; highest for wide tables with selective queries

## Test Cases

### Positive: Wide table with selective filter
```sql
SELECT user_id, name, address, phone, email, bio, avatar_url
FROM user_profiles
WHERE country = 'US' AND age > 25
LIMIT 100;

-- Phase 1: read only country and age columns
-- Phase 2: for 100 surviving rows, read remaining 5 columns
-- Avoids reading bio, avatar_url for millions of non-matching rows
```

### Positive: ORDER BY LIMIT on wide table
```sql
SELECT * FROM products ORDER BY price DESC LIMIT 10;

-- Phase 1: read only price column, sort, take top 10
-- Phase 2: materialize all columns for just 10 rows
```

### Negative: No filter, no limit
```sql
SELECT * FROM events;

-- All rows need all columns; lazy materialization adds overhead
-- Random I/O in phase 2 worse than sequential scan
```

## References

- ClickHouse: `src/Processors/QueryPlan/Optimizations/optimizeLazyMaterialization.cpp`
- ClickHouse: `src/Processors/QueryPlan/LazilyReadFromMergeTree.h`
- Abadi et al., "Column-Stores vs. Row-Stores: How Different Are They Really?", SIGMOD 2008
