# Rule: Use Normal Projection for Column Subset

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/normal-projection-usage.rra`

## Metadata

- **ID:** `clickhouse-normal-projection-usage`
- **Version:** 1.0.0
- **Databases:** clickhouse
- **Tags:** database-specific, clickhouse, projection, column-subset, mergetree
- **Authors:** "RA Contributors"


# Use Normal Projection for Column Subset

## Description

Uses a normal (non-aggregate) projection that stores a subset of columns in a different sort order. When a query needs only projected columns and benefits from the projection's sort order, reading the projection is more efficient than reading the base table.

**When to apply**: Query references only columns in a projection and can benefit from projection's sort order.

**Why it works**: Projections store fewer columns and can have different sort orders optimized for specific queries. Reading less data with better ordering improves performance.

**Database version**: ClickHouse v21.12+

## Relational Algebra

```algebra
Scan[MergeTree, cols](T)
  -> Scan[Projection[cols'], sort_order'](T)
  where cols $\subseteq$ cols'
  where sort_order' benefits_query
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("clickhouse-normal-projection-usage";
    "(scan ?table ?props)" =>
    "(scan (use_projection ?table (find_normal_projection ?props)) ?props)"
    if is_database("clickhouse")
    if has_matching_normal_projection("?table", "?props")
),
```

## Preconditions

```rust
fn applicable(
    query_cols: &[Column],
    query_order: &Ordering,
    table: &TableRef,
) -> bool {
    for proj in table.normal_projections() {
        // All query columns must be in projection
        if !query_cols.iter().all(|c| proj.columns().contains(c)) {
            continue;
        }

        // Projection should provide ordering benefit
        if proj.sort_order().benefits(query_order) {
            return true;
        }
    }
    false
}
```

**Restrictions:**
- Only applies to ClickHouse MergeTree
- Query columns must be subset of projection columns
- Most beneficial when projection has advantageous sort order

## Cost Model

```rust
fn estimated_benefit(
    base_cols_size: f64,
    proj_cols_size: f64,
    has_ordering_benefit: bool,
) -> f64 {
    let io_benefit = (base_cols_size - proj_cols_size) / base_cols_size;
    if has_ordering_benefit {
        io_benefit + 0.3 // Additional benefit from better ordering
    } else {
        io_benefit
    }
}
```

**Typical benefit**: 30-80% when projection is much smaller

## Test Cases

```sql
CREATE TABLE events (
  ts DateTime,
  user_id UInt64,
  event String,
  payload String  -- large column
) ENGINE = MergeTree()
ORDER BY ts;

-- Projection for user queries
ALTER TABLE events ADD PROJECTION user_events (
  SELECT ts, user_id, event
  ORDER BY (user_id, ts)
);

SELECT ts, event FROM events
WHERE user_id = 12345
ORDER BY ts;

-- Uses user_events projection:
-- - Reads only ts, user_id, event (not payload)
-- - Sorted by (user_id, ts) - efficient for this query
```

## References

**Source code:**
- ClickHouse: `src/Processors/QueryPlan/Optimizations/optimizeUseNormalProjection.cpp`
  - Commit: 35f2d31186cca2f8c50f7ba4bd93817da490da85
