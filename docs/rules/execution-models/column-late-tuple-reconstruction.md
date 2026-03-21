# Rule: Column-at-a-Time Late Tuple Reconstruction

**Category:** execution-models/column-at-a-time
**File:** `rules/execution-models/column-at-a-time/column-late-tuple-reconstruction.rra`

## Metadata

- **ID:** `column-late-tuple-reconstruction`
- **Version:** "1.0.0"
- **Databases:** monetdb, clickhouse, duckdb
- **Tags:** execution, columnar, late-materialization, tuple-reconstruction


# Column-at-a-Time Late Tuple Reconstruction

## Description

Delays reconstruction of full tuples (rows) until the final output stage,
operating on individual columns and position lists throughout the query pipeline.
After all filtering, joining, and aggregation operate on columnar data, only the
surviving rows are reconstructed into tuples for the result set.

**When to apply**: Queries that filter, sort, or aggregate before producing
output rows. Late materialization avoids constructing tuples for rows that will
be filtered out, and avoids reading columns that are only needed in the final
projection.

**Why it works**: Early materialization (constructing tuples before filtering)
reads all columns and builds tuples for every row, including those that will be
discarded. Late materialization processes only the columns needed for filtering,
then uses the surviving position list to fetch the output columns. If a filter
discards 99% of rows, late materialization reads output columns for only 1% of
rows instead of 100%.

## Implementation

```rust
/// Late materialization: filter first, fetch output columns only for survivors
pub fn late_materialize(
    filter_cols: &[Column],
    output_cols: &[Column],
    predicate: &Predicate,
) -> Vec<Row> {
    // Phase 1: filter on filter columns, produce position list
    let positions = predicate.evaluate_bitmap(filter_cols);

    // Phase 2: fetch output columns only for surviving positions
    let mut results = Vec::with_capacity(positions.count_ones());
    for &col in output_cols {
        results.push(col.gather(&positions));
    }

    reconstruct_tuples(results)
}
```

## Cost Model

- Early materialization: reads ALL columns for ALL rows, cost = n * C
- Late materialization: reads filter columns for all rows + output columns for k rows
  cost = n * c_filter + k * c_output, where k = n * selectivity
- Breakeven: when selectivity < c_output / (C - c_filter + c_output)
- For 10% selectivity and 10 output columns vs 2 filter columns: ~5x savings

## Test Cases

```sql
-- Highly selective query on wide table
SELECT name, address, phone, email  -- 4 output columns
FROM customers                       -- 50 columns total
WHERE region = 'US'                  -- 1 filter column
  AND status = 'active';             -- 1 filter column

-- Late materialization:
-- 1. Read region and status columns (2 of 50)
-- 2. Filter: 5% of rows survive
-- 3. Read name, address, phone, email for surviving 5%
-- Total I/O: 2/50 * 100% + 4/50 * 5% = 4.4% of full table scan
```

## References

1. Abadi et al., "Materialization Strategies in a Column-Oriented DBMS",
   ICDE 2007
2. Idreos et al., "MonetDB: Two Decades of Research", IEEE Data Eng. Bull. 2012
3. Boncz et al., "MonetDB/X100: Hyper-Pipelining Query Execution", CIDR 2005
