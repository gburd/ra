# Rule: Inter-Operator Parallelism

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/inter-operator-parallelism.rra`

## Metadata

- **ID:** `inter-operator-parallelism`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** parallelization, inter-operator, pipeline
- **Authors:** "RA Contributors"


# Inter-Operator Parallelism

## Description

Executes different operators in parallel in pipeline fashion; producer-consumer parallelism.

**When to apply**: Long operator pipelines with available threads.

**Why it works**: Operators run concurrently; reduces latency; overlaps computation.

## Relational Algebra

```algebra
operator3(operator2(operator1(data)))
  -> concurrent_pipeline:
       operator1 produces → operator2 processes → operator3 consumes
```

## Implementation

```rust
rw!("inter-op-parallel";
    "(pipeline ?op1 ?op2 ?op3)" =>
    "(parallel-pipeline
       (thread ?op1)
       (thread ?op2)
       (thread ?op3))"
    if can_pipeline("?op1", "?op2", "?op3")
),
```

## Cost Model

```rust
fn latency_reduction(ops: usize) -> f64 {
    1.0 / ops as f64 // Pipelined latency
}
```

**Typical benefit**: 30-60% latency reduction

## References

- PostgreSQL: Parallel query execution
- DuckDB: Pipeline parallelism
