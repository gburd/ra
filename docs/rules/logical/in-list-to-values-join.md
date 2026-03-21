# Rule: IN List to VALUES Join

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/in-list-to-values-join.rra`

## Metadata

- **ID:** `in-list-to-values-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, mssql, oracle
- **Tags:** subquery, in-list, values, join
- **Authors:** "RA Contributors"


# IN List to VALUES Join

## Description

Converts a large IN list to a semi-join with an inline VALUES table. This enables the optimizer to use index lookups or hash joins instead of sequential OR comparisons.

**When to apply**: IN list with more than a threshold number of constant values.

## Relational Algebra

```algebra
Filter[col IN (v1, v2, ..., vN)](Scan[table])
  -> SemiJoin[col = v](Scan[table], Values[(v1), (v2), ..., (vN)])
  where N > in_list_threshold
```

## Implementation

```rust
rw!("in-list-to-values-join";
    "(filter (in ?col ?values) ?input)" =>
    "(join semi (= ?col ?v) ?input (values ?values))"
    if large_in_list("?values")
),
```

## Test Cases

### Positive: Large IN list

```sql
SELECT * FROM orders WHERE customer_id IN (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12);

-- Convert to semi-join with VALUES
```

### Negative: Small IN list

```sql
SELECT * FROM orders WHERE status IN ('open', 'closed');

-- Keep as OR comparison
```

## References

- IN list optimization in PostgreSQL (enable_hashjoin)
- DuckDB IN list to hash join conversion
