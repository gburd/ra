# Rule: CTE Materialization

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/cte-materialization.rra`

## Metadata

- **ID:** `cte-materialization`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, mssql
- **Tags:** cte, materialization, temp-table, with
- **Authors:** "RA Contributors"


# CTE Materialization

## Description

Materializes a CTE into a temporary result when it is referenced multiple times. Avoids recomputing an expensive subquery.

**When to apply**: CTE is referenced more than once and the definition is expensive.

**Why it works**: Compute once, read many times. Trades memory for CPU.

## Relational Algebra

```algebra
CTE[name, def](body)
  -> materialize(name, def); body
  where ref_count(name, body) > 1
```

## Implementation

```rust
rw!("cte-materialize";
    "(cte ?name ?def ?body)" =>
    "(materialize ?name ?def ?body)"
    if multi_reference("?name", "?body")
),
```

## Cost Model

```rust
fn benefit(def_cost: f64, ref_count: u64) -> f64 {
    let without = def_cost * ref_count as f64;
    let with_mat = def_cost + ref_count as f64 * 0.1 * def_cost;
    (without - with_mat) / without
}
```

**Typical benefit**: 30-80% for multi-referenced expensive CTEs

## Test Cases

### Positive: CTE referenced twice

```sql
WITH dept_stats AS (
  SELECT dept_id, AVG(salary) as avg_sal
  FROM employees GROUP BY dept_id
)
SELECT a.dept_id, b.dept_id
FROM dept_stats a, dept_stats b
WHERE a.avg_sal > b.avg_sal;

-- Materialize dept_stats once
```

### Negative: Single reference

```sql
WITH t AS (SELECT * FROM users WHERE active)
SELECT * FROM t LIMIT 10;

-- Single reference: inline instead
```

## References

- PostgreSQL: MATERIALIZED / NOT MATERIALIZED hints (PG 12+)
- Oracle: MATERIALIZE hint for CTEs
- mssql: Automatic CTE spooling
