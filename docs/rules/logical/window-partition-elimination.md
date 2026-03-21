# Rule: Window Partition Elimination

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-partition-elimination.rra`

## Metadata

- **ID:** `window-partition-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, oracle
- **Tags:** window, partition, elimination, optimization
- **Authors:** "RA Contributors"


# Window Partition Elimination

## Description

Removes redundant PARTITION BY columns from window functions when the input is already filtered to a single partition value.

**When to apply**: Filter guarantees a single partition value for a PARTITION BY column.

**Why it works**: If all rows belong to the same partition, the partition step is unnecessary.

## Relational Algebra

```algebra
window[W PARTITION BY c](filter[c = const](R))
  -> window[W](filter[c = const](R))
```

## Implementation

```rust
rw!("window-partition-elim";
    "(window (partition ?cols ?order ?frame ?func)
       (filter (= ?col ?val) ?input))" =>
    "(window (partition (remove ?col ?cols) ?order ?frame ?func)
       (filter (= ?col ?val) ?input))"
    if col_in_partition("?col", "?cols")
),
```

## Cost Model

```rust
fn benefit(num_partitions: u64) -> f64 {
    if num_partitions == 1 { 0.3 } else { 0.0 }
}
```

**Typical benefit**: 10-40% when partitions are eliminated

## Test Cases

### Positive: Single-value partition

```sql
SELECT ROW_NUMBER() OVER (PARTITION BY dept_id ORDER BY salary)
FROM employees WHERE dept_id = 5;

-- Remove PARTITION BY dept_id (always 5)
```

### Negative: Multiple partition values

```sql
SELECT ROW_NUMBER() OVER (PARTITION BY dept_id ORDER BY salary)
FROM employees WHERE salary > 50000;

-- Multiple dept_id values remain
```

## References

- PostgreSQL: Window function simplification
- Oracle: Partition elimination in analytics
