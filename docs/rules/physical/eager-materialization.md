# Rule: Eager Materialization

**Category:** physical/materialization
**File:** `rules/physical/materialization/eager-materialization.rra`

## Metadata

- **ID:** `eager-materialization`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** materialization, eager, cte
- **Authors:** "RA Contributors"


# Eager Materialization

## Description

Materializes subquery/CTE results immediately into temp table; reuses materialized result for multiple references.

**When to apply**: Subquery referenced multiple times or expensive to recompute.

**Why it works**: Compute once, reuse many times; avoids redundant evaluation.

## Relational Algebra

```algebra
WITH cte AS (expensive_query)
SELECT ... FROM cte, cte AS cte2 ...

-> materialize(cte) = temp_table
   use temp_table twice
```

## Implementation

```rust
rw!("eager-materialize-cte";
    "(with ?name ?subquery (references-multiple ?name ?query))" =>
    "(let ?temp (materialize ?subquery)
      (substitute ?name ?temp ?query))"
    if expensive("?subquery") || multiple_refs("?name", "?query")
),
```

## Cost Model

```rust
fn cost(subquery_cost: f64, num_refs: usize, subquery_size: u64) -> f64 {
    let materialize = subquery_cost + (subquery_size as f64 * 0.1); // Write temp
    let reuse = (num_refs - 1) as f64 * (subquery_size as f64 * 0.05); // Read temp
    materialize + reuse
}

fn benefit(sq_cost: f64, refs: usize) -> f64 {
    let without = sq_cost * refs as f64;
    let with = sq_cost + (refs as f64 * 10.0);
    (without - with) / without
}
```

**Typical benefit**: 40-80% when subquery referenced 3+ times

## Test Cases

### Positive: CTE referenced multiple times

```sql
WITH active_users AS (
    SELECT * FROM users WHERE last_login > NOW() - INTERVAL '30 days'
)
SELECT
    (SELECT COUNT(*) FROM active_users),
    (SELECT AVG(age) FROM active_users),
    (SELECT MAX(score) FROM active_users);

-- Materialize active_users once, scan 3 times
```

### Positive: Expensive subquery

```sql
WITH complex_calc AS (
    SELECT user_id, expensive_function(data) as result
    FROM large_table
)
SELECT * FROM complex_calc WHERE result > 100;

-- Materialize expensive_function results
```

### Negative: Simple subquery, single reference

```sql
WITH recent AS (SELECT * FROM logs WHERE date = CURRENT_DATE)
SELECT COUNT(*) FROM recent;

-- Not worth materializing: single scan, simple filter
```

## References

- PostgreSQL: CTE materialization (WITH clause)
- MySQL: Derived table materialization
- DuckDB: Automatic CTE materialization
