# Rule: Distinct on Unique Key Elimination

**Category:** logical/distinct-elimination
**File:** `rules/logical/distinct-elimination/distinct-on-unique-key.rra`

## Metadata

- **ID:** `distinct-on-unique-key`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, duckdb, sqlite
- **Tags:** distinct, unique, primary-key, elimination
- **Authors:** "RA Contributors"


# Distinct on Unique Key Elimination

## Description

Removes DISTINCT when the selected columns include a unique key or primary key, since all rows are already unique.

**When to apply**: Projection includes a column (or set of columns) that form a unique constraint.

**Why it works**: If the output includes a unique key, no two rows can be identical, making DISTINCT a no-op.

## Relational Algebra

```algebra
distinct(project[cols](R))
  -> project[cols](R)
  where unique_key(R) ⊆ cols
```

## Implementation

```rust
rw!("distinct-on-unique-key";
    "(distinct ?input)" =>
    "?input"
    if output_has_unique_key("?input")
),
```

## Cost Model

```rust
fn benefit(rows: u64) -> f64 {
    let distinct_cost = rows as f64 * (rows as f64).log2();
    distinct_cost / (distinct_cost + rows as f64)
}
```

**Typical benefit**: 50-90% (eliminates sort or hash entirely)

## Test Cases

### Positive: Primary key in projection

```sql
SELECT DISTINCT id, name FROM users;

-- id is primary key; DISTINCT is redundant
```

### Positive: Unique constraint

```sql
SELECT DISTINCT email FROM users;

-- email has UNIQUE constraint
```

### Negative: Non-unique columns

```sql
SELECT DISTINCT department FROM employees;

-- department is not unique
```

## References

- PostgreSQL: Unique key detection for DISTINCT elimination
- Oracle: DISTINCT elimination with constraints
- MySQL: Index-based DISTINCT optimization
