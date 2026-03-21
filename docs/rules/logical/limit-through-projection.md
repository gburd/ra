# Rule: LIMIT Through Projection

**Category:** logical/limit-pushdown
**File:** `rules/logical/limit-pushdown/limit-through-projection.rra`

## Metadata

- **ID:** `limit-through-projection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** limit, projection, pushdown
- **Authors:** "RA Contributors"


# LIMIT Through Projection

## Description

Pushes LIMIT below projection to reduce rows before computing projections.

**When to apply**: LIMIT over projection with expensive expressions.

**Why it works**: Computing fewer expensive projections saves CPU.

## Relational Algebra

```algebra
limit[K](project[exprs](R))
  -> project[exprs](limit[K](R))
  where no_side_effects(exprs)
```

## Implementation

```rust
rw!("limit-through-project";
    "(limit ?k (project ?exprs ?input))" =>
    "(project ?exprs (limit ?k ?input))"
    if no_side_effects("?exprs")
),
```

## Cost Model

```rust
fn benefit(input_size: u64, k: u64, expr_cost: f64) -> f64 {
    let without = input_size as f64 * expr_cost; // Compute all
    let with = k as f64 * expr_cost; // Compute K only
    (without - with) / without
}
```

**Typical benefit**: 20-50% for expensive expressions

## Test Cases

### Positive: Expensive UDF

```sql
SELECT expensive_udf(col) FROM logs LIMIT 100;

-- Push limit: call UDF on 100 rows, not all
```

### Positive: Complex calculation

```sql
SELECT id, complex_calc(a, b, c) FROM data LIMIT 10;

-- Compute complex_calc for 10 rows only
```

### Negative: Side effects

```sql
SELECT id, nextval('seq') FROM generate_series(1, 1000) LIMIT 10;

-- Cannot push: nextval has side effects
```

## References

- PostgreSQL: Projection pushdown in planner
- DuckDB: Expression evaluation cost model
