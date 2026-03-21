# Rule: Parallel Sort

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/parallel-sort.rra`

## Metadata

- **ID:** `parallel-sort`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** parallelization, sort
- **Authors:** "RA Contributors"


# Parallel Sort

## Description

Sorts data partitions in parallel, then merges sorted runs.

**When to apply**: Large sorts with multiple cores.

**Why it works**: Sort parallelized across partitions; final merge efficient.

## Relational Algebra

```algebra
sort[key](R)
  -> merge_sorted(
       parallel_gather(
         sort[key](R_partition_i)
       ))
```

## Implementation

```rust
rw!("parallelize-sort";
    "(sort ?key ?input)" =>
    "(merge-sorted ?key
       (parallel-gather
         (map-workers ?w
           (sort ?key (partition-data ?input ?w)))))"
    if is_large("?input")
),
```

## Cost Model

```rust
fn cost(size: u64, workers: usize) -> f64 {
    let partition_sort = (size as f64 / workers as f64) * (size as f64).log2();
    let merge = size as f64 * (workers as f64).log2();
    partition_sort + merge
}
```

**Typical benefit**: 40-80% for large sorts

## References

- PostgreSQL: Parallel sort
- DuckDB: Parallel merge sort
