# Rule: Window to Aggregate Conversion

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-to-aggregate.rra`

## Metadata

- **ID:** `window-to-aggregate`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** window, aggregate, conversion, simplification
- **Authors:** "RA Contributors"


# Window to Aggregate Conversion

## Description

Converts a window aggregate function without PARTITION BY and with no ORDER BY (or with a frame covering all rows) into a simple aggregate. This avoids the overhead of window function infrastructure.

**When to apply**: Window function has no PARTITION BY, no ORDER BY, and computes a simple aggregate over all rows.

**Why it works**: A window aggregate over the entire result set is equivalent to a scalar aggregate cross-joined back.

## Relational Algebra

```algebra
window[AGG() OVER ()](R)
  -> cross_join(R, aggregate[AGG()](R))
  where no_partition_by AND (no_order_by OR frame = UNBOUNDED)
```

## Implementation

```rust
rw!("window-to-aggregate";
    "(window (agg ?func ?arg () () ()) ?input)" =>
    "(cross-join ?input (aggregate (list (agg-expr ?func ?arg)) ?input))"
),
```

## Cost Model

```rust
fn benefit(rows: u64) -> f64 {
    // Window infrastructure has overhead vs simple aggregate
    0.2
}
```

**Typical benefit**: 10-40% for simple full-table window aggregates

## Test Cases

### Positive: Unpartitioned COUNT

```sql
SELECT *, COUNT(*) OVER () as total FROM orders;

-- Convert to cross join with scalar aggregate
```

### Negative: Partitioned window

```sql
SELECT *, COUNT(*) OVER (PARTITION BY dept_id) as dept_count FROM employees;

-- Has PARTITION BY; keep as window function
```

## References

- PostgreSQL: Window function simplification
- MySQL: Window function to aggregate optimization
