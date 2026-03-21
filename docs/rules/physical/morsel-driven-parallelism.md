# Rule: Morsel-Driven Parallelism

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/morsel-driven-parallelism.rra`

## Metadata

- **ID:** `morsel-driven-parallelism`
- **Version:** "1.0.0"
- **Databases:** duckdb, hyper, umbra
- **Tags:** parallelization, morsel, work-stealing
- **Authors:** "RA Contributors"


# Morsel-Driven Parallelism

## Description

Processes data in small batches (morsels); dynamic work distribution with stealing; optimal CPU utilization.

**When to apply**: Modern vectorized execution engines with push-based model.

**Why it works**: Fine-grained parallelism; automatic load balancing; cache-friendly.

## Relational Algebra

```algebra
operator_pipeline(data)
  -> morsel_dispatcher:
       while data:
         morsel = next_batch(MORSEL_SIZE)  // e.g., 100K rows
         assign to available worker
       workers can steal morsels if idle
```

## Implementation

```rust
rw!("morsel-driven";
    "?pipeline" =>
    "(morsel-execution ?pipeline
       :morsel-size 100000
       :work-stealing true)"
    if is_push_based("?pipeline")
),
```

## Cost Model

```rust
fn cost(data_size: u64, morsel_size: u64, workers: usize, efficiency: f64) -> f64 {
    let morsels = data_size / morsel_size;
    let perfect_parallel = (data_size as f64 / workers as f64);
    perfect_parallel / efficiency // 90-95% typical efficiency
}
```

**Typical benefit**: 60-95% with optimal load balancing

## Test Cases

### Positive: Variable workload per tuple

```sql
SELECT expensive_udf(col) FROM large_table;

-- Some rows expensive, some cheap
-- Work stealing balances load dynamically
```

### Positive: Complex pipeline

```sql
SELECT SUM(a) FROM (
    SELECT complex_calc(x) as a FROM data WHERE pred
) GROUP BY a;

-- Morsel-driven execution through entire pipeline
```

## References

- DuckDB: Morsel-driven parallelism
- HyPer: "Morsel-Driven Parallelism" (Leis et al., 2014)
- Umbra: Push-based morsel execution
