# Rule: TiDB Coprocessor TOP-N Pushdown

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/coprocessor-topn-pushdown.rra`

## Metadata

- **ID:** `tidb-coprocessor-topn-pushdown`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** coprocessor, pushdown, topn, order, limit, tikv
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Coprocessor TOP-N Pushdown

## Description

Pushes ORDER BY + LIMIT (TOP-N) to TiKV coprocessor, maintaining only
top N rows at each region using a min-heap, then merging results at TiDB.

## Relational Algebra

```algebra
Limit[n](Sort[cols](Scan[table]))
  -> MergeSort[cols](CopTask(TopN[n, cols](Scan[table])))
```

## Implementation

```rust
fn push_topn_to_cop(limit: &Limit, sort: &Sort, scan: &Scan) -> CopTask {
    CopTask {
        table: scan.table,
        topn: Some(TopN {
            limit: limit.count,
            order_by: sort.columns,
        }),
    }
}
```

## Cost Model

Each TiKV region maintains heap of size n, dramatically reducing transfer.

## Test Cases

```sql
-- Top 1000 highest value orders
SELECT * FROM orders ORDER BY total_amount DESC LIMIT 1000;
-- Each TiKV region keeps top 1000, TiDB merges
```

## References
- Source: `pkg/planner/core/exhaust_physical_plans.go` (getTaskPlan)
