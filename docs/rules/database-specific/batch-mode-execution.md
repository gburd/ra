# Rule: mssql Batch Mode Execution

**Category:** database-specific/mssql
**File:** `rules/database-specific/mssql/batch-mode-execution.rra`

## Metadata

- **ID:** `mssql-batch-mode-execution`
- **Version:** "1.0.0"
- **Databases:** mssql
- **Tags:** database-specific, mssql, batch-mode, columnstore, vectorized
- **Authors:** "RA Contributors"


# mssql Batch Mode Execution

## Description

mssql's batch mode processes data in vectors of approximately
900 rows at a time instead of the traditional row-by-row (row mode)
execution.  Originally available only for queries involving
columnstore indexes, batch mode was extended to rowstore tables in
mssql 2019 via "batch mode on rowstore."  Batch mode operators
use vectorized execution with tight loops, SIMD instructions, and
cache-friendly data access patterns.

**When to apply**: Analytical queries that perform full scans,
aggregations, joins, or sorts over large datasets.  The query must
use operators that have batch mode implementations (hash join, hash
aggregate, sort, filter, project).

**Why it works**: Processing 900 rows per function call amortizes
the per-row overhead of the Volcano iterator model.  Batch mode
operators operate on columnar in-memory formats, enabling CPU cache
line utilization and SIMD vectorization.

**Database version**: mssql 2012+ (columnstore), 2019+
(rowstore)

## Relational Algebra

```algebra
-- Before: row-mode hash aggregate
row_mode_gamma[region; SUM(sales)](scan(orders))

-- After: batch-mode hash aggregate
batch_mode_gamma[region; SUM(sales)](
    batch_scan(orders))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mssql-batch-mode-execution";
    "(row-mode-aggregate ?group ?agg ?input)" =>
    "(batch-mode-aggregate ?group ?agg
        (to-batch ?input))"
    if is_database("mssql")
    if supports_batch_mode("?input")
    if estimated_rows_exceed("?input", 10000)
),
```

## Preconditions

```rust
fn applicable(
    query: &QueryPlan,
    table: &Table,
    version: &Version,
) -> bool {
    (table.has_columnstore_index()
        || version >= Version::new(2019, 0, 0))
    && query.estimated_rows() > 10_000
    && query.operators_support_batch_mode()
}
```

**Restrictions:**
- Not all operators support batch mode (e.g., nested loop join
  does not)
- mssql 2019+ required for batch mode on rowstore
- Memory grant required for batch mode operators
- Can be forced with OPTION (USE HINT('ENABLE_BATCH_MODE'))

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    num_operators: usize,
) -> f64 {
    // Batch mode reduces per-row overhead by ~10x
    let row_mode_cost = rows * num_operators as f64 * 0.001;
    let batch_mode_cost = (rows / 900.0) * num_operators as f64 * 0.1;
    row_mode_cost - batch_mode_cost
}
```

**Typical benefit**: 3-10x speedup for analytical queries,
especially aggregations and hash joins over large tables.

## Test Cases

```sql
-- Positive: large aggregation query
SELECT region, SUM(amount), COUNT(*)
FROM sales
GROUP BY region;
-- Batch mode hash aggregate on 10M+ rows
```

```sql
-- Negative: small OLTP lookup
SELECT * FROM orders WHERE order_id = 12345;
-- Row mode is faster for single-row lookups
```

## References

Microsoft: "Batch Mode Execution" documentation
Microsoft: "Batch Mode on Rowstore" (mssql 2019)
Microsoft: DMV sys.dm_exec_query_profiles (batch vs row mode)
