# Rule: DuckDB Zone Map (Min/Max Index) Pruning

**Category:** database-specific/duckdb
**File:** `rules/database-specific/duckdb/zonemap-pruning.rra`

## Metadata

- **ID:** `duckdb-zonemap-pruning`
- **Version:** "1.0.0"
- **Databases:** duckdb
- **Tags:** database-specific, duckdb, zonemap, min-max, pruning, columnar
- **Authors:** "RA Contributors"


# DuckDB Zone Map (Min/Max Index) Pruning

## Description

DuckDB maintains lightweight min/max statistics (zone maps) for
each row group in its columnar storage.  When a filter predicate
is applied, DuckDB checks the zone map for each row group and
skips entire groups whose min/max range does not overlap with the
filter range.  This is effectively a form of segment elimination.

**When to apply**: A filter on a column with range conditions
(equality, less than, greater than, BETWEEN) and the data has
some degree of clustering within row groups.

**Why it works**: If a row group's maximum value for column X is
50 and the filter is X > 100, the entire group can be skipped
without reading any data.  For sorted or partially sorted data,
this eliminates the majority of row groups.

**Database version**: DuckDB 0.3.0+

## Relational Algebra

```algebra
-- Before: full column scan with filter
sigma[price > 1000](column_scan(orders, price))

-- After: zone-map pruned scan
sigma[price > 1000](
    zonemap_pruned_scan(orders, price, price > 1000))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("duckdb-zonemap-pruning";
    "(filter ?pred (column-scan ?table ?cols))" =>
    "(filter ?pred
        (zonemap-pruned-scan ?table ?cols ?pred))"
    if is_database("duckdb")
    if is_range_predicate("?pred")
    if column_has_zonemaps("?table", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    predicate: &Predicate,
    column_stats: &ColumnStats,
) -> bool {
    predicate.is_range_comparison()
    && column_stats.has_zone_maps()
}
```

**Restrictions:**
- Only works with range comparisons (=, <, >, <=, >=, BETWEEN)
- Not effective for LIKE or complex expressions
- Effectiveness depends on data clustering within row groups
- Zone maps are always maintained, no configuration needed

## Cost Model

```rust
fn estimated_benefit(
    total_row_groups: usize,
    prunable_groups: usize,
    rows_per_group: usize,
) -> f64 {
    let rows_skipped = prunable_groups * rows_per_group;
    rows_skipped as f64 * 0.001 // cost per row avoided
}
```

**Typical benefit**: 50-99% of row groups skipped for selective
range queries on sorted or semi-sorted data.

## Test Cases

```sql
-- Positive: range filter on semi-sorted column
SELECT * FROM events
WHERE event_date BETWEEN '2025-01-01' AND '2025-01-31';
-- Zone maps skip row groups outside the date range
```

```sql
-- Negative: non-range predicate
SELECT * FROM events
WHERE event_name LIKE '%error%';
-- Zone maps cannot prune on LIKE patterns
```

## References

DuckDB: "Storage" documentation (duckdb.org)
DuckDB: Zone maps in row group metadata
Source: src/storage/table/row_group.cpp
