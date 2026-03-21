# Rule: Bushy Parallelism

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/bushy-parallelism.rra`

## Metadata

- **ID:** `bushy-parallelism`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** parallelization, bushy, multi-way-join
- **Authors:** "RA Contributors"


# Bushy Parallelism

## Description

Executes independent branches of bushy join tree in parallel.

**When to apply**: Bushy join plans with independent sub-trees.

**Why it works**: Independent joins computed concurrently; reduces overall latency.

## Relational Algebra

```algebra
join(join(A, B), join(C, D))
  -> concurrent:
       thread1: join(A, B)
       thread2: join(C, D)
       main: join(result1, result2)
```

## Implementation

```rust
rw!("bushy-parallel";
    "(join (join ?a ?b) (join ?c ?d))" =>
    "(join (parallel (join ?a ?b))
           (parallel (join ?c ?d)))"
    if independent_subtrees()
),
```

## Cost Model

```rust
fn latency(left_cost: f64, right_cost: f64) -> f64 {
    left_cost.max(right_cost) // Parallel execution
}
```

**Typical benefit**: 40-80% latency reduction

## References

- PostgreSQL: Parallel bushy joins
- DuckDB: Multi-way join parallelism
