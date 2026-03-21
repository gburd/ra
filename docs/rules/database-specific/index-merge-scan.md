# Rule: Index Merge Scan (Multi-Index OR)

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/index-merge-scan.rra`

## Metadata

- **ID:** `tidb-index-merge-scan`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** distributed, index, merge, or-predicate, multi-index, tidb
- **Authors:** "RA Contributors"


# Index Merge Scan (Multi-Index OR)

## Description

Uses multiple indexes simultaneously to satisfy an OR predicate, then
merges the results. When a query has disjunctive predicates (OR) where
each side can use a different index, TiDB's index merge reads from
both indexes in parallel and combines the row IDs, avoiding a full
table scan.

**When to apply**: A query has a WHERE clause with OR predicates, and
different indexes can satisfy each side of the OR. The table must
support index merge (enabled by default since TiDB 5.4).

**Why it works**: Without index merge, an OR predicate forces a full
table scan because no single index covers both sides. Index merge
reads matching row IDs from each index, takes their union (for OR)
or intersection (for AND), and then fetches the actual rows only for
the merged set.

## Relational Algebra

```algebra
sigma[a = 1 OR b = 2](Scan(T))
  -> IndexMerge(
       Union(
         IndexRangeScan(T, idx_a, a = 1),
         IndexRangeScan(T, idx_b, b = 2)
       ),
       TableRowIDScan(T)
     )
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("index-merge-or";
    "(filter (or ?pred_a ?pred_b) (scan ?table))" =>
    "(index_merge union
        (index_range_scan ?table ?idx_a ?pred_a)
        (index_range_scan ?table ?idx_b ?pred_b)
        (table_rowid_scan ?table))"
    if has_index_for("?table", "?pred_a", "?idx_a")
    if has_index_for("?table", "?pred_b", "?idx_b")
),

rw!("index-merge-and";
    "(filter (and ?pred_a ?pred_b) (scan ?table))" =>
    "(index_merge intersect
        (index_range_scan ?table ?idx_a ?pred_a)
        (index_range_scan ?table ?idx_b ?pred_b)
        (table_rowid_scan ?table))"
    if has_index_for("?table", "?pred_a", "?idx_a")
    if has_index_for("?table", "?pred_b", "?idx_b")
    if index_merge_cheaper_than_single_index("?table", "?pred_a", "?pred_b")
),
```

## Preconditions

```rust
fn applicable(
    table: &DataSource,
    predicates: &[Expression],
    is_or: bool,
) -> bool {
    // At least two predicates with different optimal indexes
    let idx_plans: Vec<_> = predicates.iter()
        .filter_map(|p| table.best_index_for(p))
        .collect();
    idx_plans.len() >= 2
    // Different indexes (same index doesn't benefit)
    && idx_plans[0].index_id != idx_plans[1].index_id
    // Index merge must be enabled
    && session.tidb_enable_index_merge()
}
```

**Restrictions:**
- Requires distinct indexes for each predicate side
- The union/intersection of row IDs adds overhead for sorting
  and deduplication
- For AND predicates, index merge is only beneficial when neither
  index alone is selective enough
- TiFlash does not support index merge (columnar store handles OR
  predicates differently)
- Session variable `tidb_enable_index_merge` must be enabled

## Cost Model

```rust
fn index_merge_cost(
    idx_a_rows: f64,
    idx_b_rows: f64,
    table_rows: f64,
    row_fetch_cost: f64,
    index_scan_cost: f64,
) -> (f64, f64) {
    // Index merge cost
    let merge_rows = (idx_a_rows + idx_b_rows).min(table_rows);
    let merge_cost = (idx_a_rows + idx_b_rows) * index_scan_cost
        + merge_rows * row_fetch_cost;

    // Full scan cost
    let scan_cost = table_rows * row_fetch_cost;

    (merge_cost, scan_cost)
}
```

## Test Cases

```sql
-- Positive: OR with two indexed columns
-- t has INDEX(a), INDEX(b)
SELECT * FROM t WHERE a = 1 OR b = 2;

-- Plan: IndexMerge(Union)
--   IndexRangeScan(idx_a, a = 1)  -- returns row IDs
--   IndexRangeScan(idx_b, b = 2)  -- returns row IDs
--   TableRowIDScan(t)             -- fetch actual rows
```

```sql
-- Positive: AND index merge (intersection)
-- Very selective when combined
SELECT * FROM t WHERE a BETWEEN 1 AND 1000 AND b BETWEEN 1 AND 1000;
-- Each index matches 1000 rows; intersection may be much smaller
```

```sql
-- Negative: both predicates use same index
SELECT * FROM t WHERE a = 1 OR a = 2;
-- Same index handles both; IN (1, 2) is better than index merge
```

## References

TiDB: pkg/planner/core/indexmerge_path.go - index merge path generation (commit e2184a2)
TiDB: pkg/planner/core/indexmerge_unfinished_path.go - partial index merge
TiDB docs: "Index Merge" documentation
