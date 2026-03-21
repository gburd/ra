# Rule: TiDB Coprocessor LIMIT Pushdown

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/coprocessor-limit-pushdown.rra`

## Metadata

- **ID:** `tidb-coprocessor-limit-pushdown`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** coprocessor, pushdown, limit, tikv
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Coprocessor LIMIT Pushdown

## Description

Pushes LIMIT operations to TiKV coprocessor, allowing early termination
at the storage layer and dramatically reducing data transfer.

## Relational Algebra

```algebra
Limit[n](Scan[table])
  -> CopTask(Limit[n](Scan[table]))
```

## Implementation

```rust
fn push_limit_to_cop(limit: &Limit, scan: &Scan) -> CopTask {
    CopTask {
        table: scan.table,
        limit: Some(limit.count),
        filters: vec![],
    }
}
```

## Cost Model

Transfers only n rows instead of scanning entire table.

## Test Cases

```sql
-- Top 100 orders
SELECT * FROM orders ORDER BY created_at DESC LIMIT 100;
-- Coprocessor returns only 100 rows per region
```

## References
- Source: `pkg/planner/core/rule_push_down_sequence.go`
