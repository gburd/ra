# Rule: Oracle Bloom Filter Join Optimization

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/bloom-filter-join.rra`

## Metadata

- **ID:** `oracle-bloom-filter-join`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, bloom-filter, join, parallel, partition
- **Authors:** "RA Contributors"


# Oracle Bloom Filter Join Optimization

## Description

Inserts Bloom filters during parallel hash joins to pre-filter rows
on the probe side before they are sent across the parallel execution
interconnect.  Oracle's optimizer creates Bloom filters from the build
side of a hash join and applies them to the probe side's table scan,
eliminating non-matching rows early.

**When to apply**: A parallel hash join where the build side is
significantly smaller than the probe side, and the join is selective.

**Why it works**: In Oracle's parallel query infrastructure, rows from
the probe side must be redistributed across parallel server processes
via the Table Queue (TQ).  A Bloom filter eliminates non-matching rows
before redistribution, reducing inter-process communication by the
join's selectivity factor.

**Database version**: Oracle 10gR2+

## Relational Algebra

```algebra
-- Before: parallel hash join
hash-join[R.k = S.k](
    parallel-scan(R),
    parallel-scan(S))

-- After: Bloom filter applied to probe side
hash-join[R.k = S.k](
    parallel-scan(R),
    bloom-filter[bf(R.k)](parallel-scan(S)))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-bloom-filter-join";
    "(hash-join ?type (eq ?bk ?pk) ?build ?probe)" =>
    "(hash-join ?type (eq ?bk ?pk)
        ?build
        (bloom-filter-apply ?bk ?probe))"
    if is_database("oracle")
    if is_parallel_execution("?build", "?probe")
    if join_selectivity_lt("?type", 0.5)
),
```

## Preconditions

```rust
fn applicable(
    build_rows: f64,
    probe_rows: f64,
    parallelism: usize,
) -> bool {
    parallelism > 1
    && build_rows < probe_rows * 0.5
}
```

**Restrictions:**
- Only applies in parallel query execution
- Bloom filter size must fit in PGA memory
- False positive rate increases with build side cardinality
- Not beneficial for nearly 1:1 join ratios

## Cost Model

```rust
fn estimated_benefit(
    probe_rows: f64,
    selectivity: f64,
    parallelism: usize,
    row_bytes: f64,
) -> f64 {
    let rows_eliminated = probe_rows * (1.0 - selectivity);
    let tq_savings = rows_eliminated * row_bytes / parallelism as f64;
    tq_savings * 0.001 // network/IPC cost per byte
}
```

**Typical benefit**: For a 10% selective join with 100M probe rows
across 16 parallel servers, eliminates 90M rows from redistribution.

## Test Cases

```sql
-- Positive: parallel join with selective build side
SELECT /*+ PARALLEL(8) */ s.*, p.name
FROM sales s JOIN products p ON s.product_id = p.id
WHERE p.category = 'electronics';
-- Bloom filter from products(category='electronics') applied to sales scan
```

```sql
-- Negative: serial execution
SELECT s.*, p.name
FROM sales s JOIN products p ON s.product_id = p.id;
-- No parallel execution; Bloom filter not beneficial
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Bloom Filters"
Oracle: V$SQL_PLAN OPERATION = 'JOIN FILTER CREATE/USE'
