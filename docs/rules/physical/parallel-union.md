# Rule: Parallel Union

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/parallel-union.rra`

## Metadata

- **ID:** `parallel-union`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** parallelization, union, set-operations
- **Authors:** "RA Contributors"


# Parallel Union

## Description

Executes UNION branches in parallel, gathers results.

**When to apply**: UNION of multiple expensive subqueries.

**Why it works**: Independent branches computed concurrently.

## Relational Algebra

```algebra
union(query1, query2, query3)
  -> parallel_gather(
       concurrent(query1),
       concurrent(query2),
       concurrent(query3)
     )
```

## Implementation

```rust
rw!("parallel-union";
    "(union ?q1 ?q2 ?q3)" =>
    "(parallel-gather
       (parallel ?q1)
       (parallel ?q2)
       (parallel ?q3))"
    if expensive_branches("?q1", "?q2", "?q3")
),
```

## Cost Model

```rust
fn cost(costs: Vec<f64>) -> f64 {
    *costs.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap()
}
```

**Typical benefit**: 50-85% for multi-branch unions

## References

- PostgreSQL: Parallel append
- DuckDB: Parallel UNION execution
