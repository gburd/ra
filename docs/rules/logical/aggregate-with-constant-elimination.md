# Rule: Aggregate with Constant Elimination

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-with-constant-elimination.rra`

## Metadata

- **ID:** `aggregate-with-constant-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** aggregation, constant, simplification
- **Authors:** "RA Contributors"


# Aggregate with Constant Elimination

## Description

Simplifies aggregates over constants or expressions that don't require grouping.

**When to apply**: Aggregate functions over constant expressions.

**Why it works**: Constants don't need aggregation; compute directly.

## Relational Algebra

```algebra
aggregate[group, SUM(constant)](R)
  -> project[group, constant * COUNT(*)](aggregate[group, COUNT(*))](R))

aggregate[AVG(constant)](R)
  -> constant
```

## Implementation

```rust
rw!("sum-of-constant";
    "(aggregate ?group (sum ?const) ?input)" =>
    "(project (list ?group (* ?const (count-star)))
       (aggregate ?group (count-star) ?input))"
    if is_constant("?const")
),

rw!("avg-of-constant";
    "(aggregate (avg ?const) ?input)" =>
    "?const"
    if is_constant("?const")
),
```

## Cost Model

```rust
fn benefit() -> f64 {
    0.2 // Minor: eliminates redundant aggregation logic
}
```

**Typical benefit**: 10-30% (mainly simplification)

## Test Cases

### Positive: SUM of constant

```sql
SELECT dept_id, SUM(100) FROM employees GROUP BY dept_id;

-- Rewrite to: dept_id, 100 * COUNT(*)
```

### Positive: AVG of constant

```sql
SELECT AVG(42) FROM users;

-- Result is always 42
```

### Negative: Non-constant expression

```sql
SELECT SUM(salary * 1.1) FROM employees;

-- Must compute per row
```

## References

- PostgreSQL: eval_const_expressions for constant folding
- Calcite: Constant reduction rules
