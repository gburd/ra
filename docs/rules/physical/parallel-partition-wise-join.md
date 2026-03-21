# Rule: Parallel Partition-Wise Join

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/parallel-partition-wise-join.rra`

## Metadata

- **ID:** `parallel-partition-wise-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, greenplum
- **Tags:** parallelization, join, partitioned
- **Authors:** "RA Contributors"


# Partition-Wise Join

## Description

Joins co-partitioned tables partition-by-partition in parallel; no repartitioning needed.

**When to apply**: Both tables partitioned on join key.

**Why it works**: Each partition pair joins independently; perfect parallelism.

## Relational Algebra

```algebra
join[R.key = S.key](partitioned(R), partitioned(S))
  -> parallel_union(
       join[R_p1.key = S_p1.key](R_p1, S_p1),
       join[R_p2.key = S_p2.key](R_p2, S_p2),
       ...
     )
  where same_partitioning(R, S, key)
```

## Implementation

```rust
rw!("partition-wise-join";
    "(join (= ?key_r ?key_s) ?r ?s)" =>
    "(parallel-gather
       (map-partitions ?p
         (join (= ?key_r ?key_s)
           (partition ?r ?p)
           (partition ?s ?p))))"
    if co_partitioned("?r", "?s", "?key_r")
),
```

## Cost Model

```rust
fn cost(r_size: u64, s_size: u64, partitions: usize) -> f64 {
    ((r_size + s_size) as f64 / partitions as f64) * 1.2
}
```

**Typical benefit**: 60-95% with matching partitions

## References

- PostgreSQL: Partition-wise join
- Oracle: Partition-wise joins
- Greenplum: Co-located joins
