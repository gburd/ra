# Rule: "ClickHouse Column Pruning and Unused Column Removal"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/column-pruning-unused-removal.rra`

## Metadata

- **ID:** `clickhouse-column-pruning-unused-removal`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** column-pruning, projection-pushdown, unused-columns, columnar
- **Authors:** "RA Contributors"


# ClickHouse Column Pruning and Unused Column Removal

## Metadata
- **Rule ID**: `clickhouse-column-pruning-unused-removal`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Processors/QueryPlan/Optimizations/removeUnusedColumns.cpp`
- **Complexity**: O(n) with reduced I/O proportional to pruned columns
- **Prerequisites**: Query reads fewer columns than table has
- **Alternatives**: Read all columns (row-store behavior)

## Description

In columnar storage, each column is stored independently. ClickHouse
aggressively prunes columns that are not needed by downstream operators.
After optimizations like filter pushdown through JOINs, some columns may
become entirely unused. The removeUnusedColumns optimization traverses the
plan bottom-up, computing required columns at each step, and inserts
discarding expression steps to drop unreferenced columns.

This is the fundamental columnar advantage: only read what you need.
Combined with PREWHERE and lazy materialization, column pruning can
reduce I/O by orders of magnitude for wide tables.

**When to apply:**
- SELECT references subset of table columns
- After JOIN, some columns become unused
- After filter pushdown, filter columns may be the only ones needed

**Why it works for OLAP:**
- OLAP tables routinely have 50-500 columns
- Analytical queries typically touch 5-10 columns
- Each pruned column saves proportional I/O

## Relational Algebra

```
project[a,b](scan[T, {a,b,c,d,...,z}])
  -> scan[T, {a,b}]
```

## Implementation (egg rewrite rules)

```lisp
;; Prune columns at scan level
(rewrite (project ?needed-cols (scan ?table ?all-cols))
  (scan ?table ?needed-cols)
  :if (subset ?needed-cols ?all-cols))

;; Push column pruning through join
(rewrite (project ?cols (join ?type ?cond ?left ?right))
  (join ?type ?cond
    (project (needed-left-cols ?cols ?cond) ?left)
    (project (needed-right-cols ?cols ?cond) ?right)))

;; Push column pruning through aggregation
(rewrite (project ?cols (aggregate ?groups ?aggs ?input))
  (aggregate ?groups
    (prune-aggs ?aggs ?cols)
    (project (agg-input-cols ?groups ?aggs) ?input)))

;; Remove discarding step after optimization
(rewrite (discard-columns ?cols (scan ?table ?scan-cols))
  (scan ?table (diff ?scan-cols ?cols)))
```

## Cost Model

```rust
pub fn cost_column_pruning(
    rows: u64,
    total_columns: usize,
    needed_columns: usize,
    avg_col_bytes: u64,
    hardware: &HardwareModel,
) -> Cost {
    let pruned = total_columns - needed_columns;
    let io_saved = Cost::io(
        rows as f64 * pruned as f64 * avg_col_bytes as f64
        * hardware.seq_read_cost()
    );
    Cost::zero() - io_saved
}
```

**Typical benefit**: 20-90% I/O reduction depending on column ratio

## Test Cases

### Positive: SELECT few columns from wide table
```sql
CREATE TABLE analytics (
    date Date, user_id UInt64,
    col1 String, col2 String, ..., col100 String
) ENGINE = MergeTree ORDER BY (date, user_id);

SELECT date, user_id FROM analytics WHERE date = today();
-- Reads 2 columns instead of 102; ~98% I/O savings
```

### Positive: Post-JOIN column pruning
```sql
SELECT a.id, a.name FROM table_a a
JOIN table_b b ON a.id = b.a_id
WHERE b.status = 'active';

-- After filter pushdown, b.status used only for filter
-- Column pruning removes b.status from join output
```

### Negative: SELECT *
```sql
SELECT * FROM analytics;
-- All columns needed; no pruning possible
```

## References

- ClickHouse: `src/Processors/QueryPlan/Optimizations/removeUnusedColumns.cpp`
- Abadi et al., "Column-Stores vs. Row-Stores", SIGMOD 2008
