# Rule: Calcite ProjectTableScanRule

**Category:** logical/projection-pushdown
**File:** `rules/logical/projection-pushdown/project-table-scan.rra`

## Metadata

- **ID:** `calcite-project-table-scan`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql, oracle
- **Tags:** logical, calcite, project, table-scan, column-pruning
- **Authors:** "RA Contributors"


# Calcite ProjectTableScanRule

## Description

Pushes a projection into a table scan, enabling the storage engine
to read only the required columns. This is fundamental for columnar
storage formats where reading fewer columns means fewer I/O operations.

**When to apply**: A project sits above a table scan of a
ProjectableFilterableTable, and the project references a subset of
the table's columns.

**Why it works**: Columnar storage (Parquet, ORC) stores each column
separately. Reading 3 of 50 columns means 94% less I/O. Even in
row stores, narrower projections reduce buffer pool usage.

**Calcite class**: `org.apache.calcite.rel.rules.ProjectTableScanRule`

## Relational Algebra

```algebra
-- Before: project above full table scan
pi[a, b](Scan(T))

-- After: projected table scan
ProjectedScan(T, [a, b])
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-project-table-scan";
    "(project ?cols (table-scan ?table))" =>
    "(projected-table-scan ?table ?cols)"
    if table_is_projectable("?table")
),
```

## Preconditions

```rust
fn applicable(table: &Table) -> bool {
    table.is_projectable_filterable()
}
```

**Restrictions:**
- Table must implement ProjectableFilterableTable
- Expression projections (e.g., a + b) cannot be pushed to storage
- Only column references are pushed; computed columns stay above

## Cost Model

```rust
fn estimated_benefit(
    table_rows: f64,
    projected_cols: usize,
    total_cols: usize,
) -> f64 {
    if total_cols == 0 { return 0.0; }
    let col_ratio = 1.0 - (projected_cols as f64 / total_cols as f64);
    col_ratio * 0.8 // Up to 80% I/O reduction
}
```

**Typical benefit**: 10-80% I/O reduction for columnar storage.

## Test Cases

```sql
-- Positive: narrow projection from wide table
SELECT name, salary FROM employee;
-- Only 2 of potentially many columns read
```

```sql
-- Positive: star query with few columns
SELECT product_name, SUM(amount)
FROM sales JOIN products USING (product_id)
GROUP BY product_name;
-- Only product_id and amount needed from sales
```

```sql
-- Negative: SELECT * (all columns needed)
SELECT * FROM employee;
-- No column pruning possible
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/ProjectTableScanRule.java (commit af6367d)
