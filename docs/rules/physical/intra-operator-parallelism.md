# Rule: Intra-Operator Parallelism

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/intra-operator-parallelism.rra`

## Metadata

- **ID:** `intra-operator-parallelism`
- **Version:** "1.0.0"
- **Databases:** duckdb, clickhouse, umbra
- **Tags:** parallelization, intra-operator, vectorized
- **Authors:** "RA Contributors"


# Intra-Operator Parallelism

## Description

Parallelizes single operator across multiple threads processing different data chunks.

**When to apply**: CPU-intensive operators with partitionable data.

**Why it works**: Data-parallel execution; near-linear scaling within operator.

## Relational Algebra

```algebra
operator(data)
  -> parallel_apply(operator, chunks(data, num_threads))
```

## Implementation

```rust
rw!("intra-op-parallel";
    "?operator" =>
    "(parallel-execute ?operator ?num_threads)"
    if is_parallelizable("?operator") && has_threads()
),
```

## Cost Model

```rust
fn cost(op_cost: f64, threads: usize, efficiency: f64) -> f64 {
    (op_cost / threads as f64) / efficiency
}
```

**Typical benefit**: 50-90% for CPU-bound operators

## References

- DuckDB: Intra-query parallelism
- ClickHouse: Thread-per-core execution
