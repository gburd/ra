# Rule: TiDB Coprocessor Projection Pushdown

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/coprocessor-projection-pushdown.rra`

## Metadata

- **ID:** `tidb-coprocessor-projection-pushdown`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** coprocessor, pushdown, projection, tikv
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Coprocessor Projection Pushdown

## Description

Pushes column projections to TiKV coprocessor, transferring only needed
columns over the network instead of full rows.

## Relational Algebra

```algebra
Project[cols](Scan[table])
  -> CopTask(Project[cols](Scan[table]))
  where all_columns_pushable(cols)
```

## Implementation

```rust
fn push_projection_to_cop(proj: &Projection, scan: &Scan) -> CopTask {
    CopTask {
        projections: proj.columns,
        table: scan.table,
        filters: vec![],
    }
}
```

## Cost Model

Reduces network transfer by sending only required columns.

## Test Cases

```sql
-- Select specific columns
SELECT id, name FROM customers WHERE region = 'US';
-- Projection pushdown: TiKV sends only id, name columns
```

## References
- Source: `pkg/planner/core/find_best_task.go` (buildCopTask)
- TiDB Docs: https://docs.pingcap.com/tidb/stable/tidb-operator-pushdown
