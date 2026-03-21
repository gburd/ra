# Rule: Parallel Scan

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/parallel-scan.rra`

## Metadata

- **ID:** `parallel-scan`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, clickhouse
- **Tags:** parallelization, scan, partition
- **Authors:** "RA Contributors"


# Parallel Scan

## Description

Divides table into partitions, scans each partition in parallel using multiple worker threads.

**When to apply**: Large table scans with available CPU cores.

**Why it works**: Linear speedup with number of cores; each worker scans independent partition.

## Relational Algebra

```algebra
scan[T]
  -> parallel_union(
       scan[T_partition_1],
       scan[T_partition_2],
       ...
       scan[T_partition_p]
     )
  where p = num_workers
```

## Implementation

```rust
rw!("parallelize-scan";
    "(scan ?table)" =>
    "(parallel-gather
       (map-workers ?worker_id
         (scan-partition ?table ?worker_id)))"
    if is_large("?table") && has_workers()
),
```

## Cost Model

```rust
fn cost(table_size: u64, num_workers: usize, parallelism_overhead: f64) -> f64 {
    let sequential = table_size as f64;
    let parallel = (table_size as f64 / num_workers as f64) + parallelism_overhead;
    parallel
}

fn speedup(workers: usize) -> f64 {
    workers as f64 * 0.8 // 80% efficiency
}
```

**Typical benefit**: 50-90% with 4-8 cores

## Test Cases

### Positive: Large table scan

```sql
SELECT * FROM large_table WHERE condition;

-- Table: 100GB, 8 workers
-- Each scans 12.5GB in parallel
```

### Negative: Small table

```sql
SELECT * FROM small_table;

-- Table: 10MB, parallelization overhead > benefit
```

## References

- PostgreSQL: Parallel sequential scan
- MySQL: Parallel query execution
- ClickHouse: Distributed parallel scans
