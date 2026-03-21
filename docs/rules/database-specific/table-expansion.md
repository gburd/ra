# Rule: Oracle Table Expansion

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/table-expansion.rra`

## Metadata

- **ID:** `oracle-table-expansion`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, table-expansion, partition, union-all
- **Authors:** "RA Contributors"


# Oracle Table Expansion

## Description

Expands a query on a partitioned table into a UNION ALL of per-partition
queries, each with a potentially different access path.  This allows
Oracle to use index access for some partitions and full scan for others
based on per-partition statistics.

**When to apply**: A query accesses a partitioned table where different
partitions have different optimal access paths (e.g., some partitions
have selective predicates, others do not).

**Why it works**: Global query plans use a single access path for all
partitions.  Table expansion allows Oracle to use an index scan on
partitions where the predicate is selective and a full partition scan
where it is not, achieving the best of both approaches.

**Database version**: Oracle 12c+

## Relational Algebra

```algebra
-- Before: single plan for all partitions
sigma[status = 'active'](scan(orders_partitioned))

-- After: expanded per partition
sigma[status = 'active'](scan(orders_p1, index=idx_status))
  union-all
sigma[status = 'active'](scan(orders_p2, full))
  union-all
sigma[status = 'active'](scan(orders_p3, index=idx_status))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-table-expansion";
    "(filter ?pred (scan ?partitioned_table))" =>
    "(union-all-partitions
        (expand-per-partition ?partitioned_table ?pred))"
    if is_database("oracle")
    if is_partitioned("?partitioned_table")
    if benefits_from_per_partition_plans("?pred", "?partitioned_table")
),
```

## Preconditions

```rust
fn applicable(
    table: &PartitionedTable,
    pred: &Expr,
) -> bool {
    table.partitions().len() >= 2
    && table.partitions().iter().any(|p|
        optimal_access_path(p, pred) != optimal_access_path(
            &table.partitions()[0], pred)
    )
}
```

**Restrictions:**
- Only applies when different partitions benefit from different plans
- Not beneficial when all partitions have similar data distribution
- EXPAND_TABLE / NO_EXPAND_TABLE hints control this
- Increases plan complexity (one branch per partition)

## Cost Model

```rust
fn estimated_benefit(
    partitions: &[PartitionStats],
    pred: &Expr,
) -> f64 {
    let unified_cost: f64 = partitions.iter()
        .map(|p| unified_access_cost(p, pred))
        .sum();
    let expanded_cost: f64 = partitions.iter()
        .map(|p| optimal_access_cost(p, pred))
        .sum();
    unified_cost - expanded_cost
}
```

**Typical benefit**: For a table with 12 monthly partitions where
the predicate is selective in 2 and not in 10, index access on the
2 selective partitions avoids full scans.

## Test Cases

```sql
-- Positive: varying selectivity across partitions
SELECT * FROM sales WHERE product_category = 'luxury';
-- luxury is 1% of Q1 data but 50% of Q4 data
-- Q1: index scan, Q4: full partition scan
```

```sql
-- Negative: uniform distribution across partitions
SELECT * FROM sales WHERE sale_date > SYSDATE - 30;
-- Recent date filter prunes partitions; no expansion needed
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Table Expansion"
Oracle: EXPAND_TABLE / NO_EXPAND_TABLE hints
