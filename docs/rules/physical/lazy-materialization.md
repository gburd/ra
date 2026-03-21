# Rule: Lazy Materialization

**Category:** physical/materialization
**File:** `rules/physical/materialization/lazy-materialization.rra`

## Metadata

- **ID:** `lazy-materialization`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** materialization, lazy, pipeline
- **Authors:** "RA Contributors"


# Lazy Materialization

## Description

Delays materialization until needed; keeps pipeline flowing; avoids temp table overhead.

**When to apply**: Single-use subqueries or when pipelining possible.

**Why it works**: No temp table I/O; data flows through pipeline; lower memory footprint.

## Relational Algebra

```algebra
WITH cte AS (query)
SELECT ... FROM cte  // single reference

-> pipeline: inline cte directly (no materialization)
```

## Implementation

```rust
rw!("lazy-materialize-cte";
    "(with ?name ?subquery ?query)" =>
    "(substitute ?name ?subquery ?query)"  // Inline
    if single_reference("?name", "?query") && can_pipeline("?subquery")
),
```

## Cost Model

```rust
fn cost(subquery_cost: f64) -> f64 {
    subquery_cost // No materialization overhead
}
```

**Typical benefit**: 30-60% vs eager when single-use

## Test Cases

### Positive: Single-use CTE

```sql
WITH filtered AS (SELECT * FROM orders WHERE status = 'pending')
SELECT COUNT(*) FROM filtered;

-- Pipeline: no temp table
```

### Negative: Multiple references

```sql
WITH data AS (SELECT * FROM expensive_query)
SELECT * FROM data UNION ALL SELECT * FROM data;

-- Must materialize: referenced twice
```

## References

- PostgreSQL: NOT MATERIALIZED hint
- DuckDB: Automatic pipeline optimization
