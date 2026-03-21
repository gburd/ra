# Rule: Oracle Hash Group By

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/hash-group-by.rra`

## Metadata

- **ID:** `oracle-hash-group-by`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, aggregate, hash, group-by, sort
- **Authors:** "RA Contributors"


# Oracle Hash Group By

## Description

Uses hash-based aggregation instead of sort-based aggregation for
GROUP BY operations.  Oracle's optimizer chooses HASH GROUP BY when
the estimated number of groups is small relative to input rows and
no ORDER BY requires sorted output.

**When to apply**: An aggregate query uses GROUP BY without ORDER BY,
and the estimated group count is small enough for a hash table to
fit in PGA memory.

**Why it works**: Sort-based GROUP BY requires O(n log n) sorting of
all input rows.  Hash-based GROUP BY builds a hash table with one entry
per group, processing each input row in O(1) amortized time for a
total O(n) cost.

**Database version**: Oracle 10gR2+

## Relational Algebra

```algebra
-- Before: sort-based group by
sort-group-by[dept; sum=SUM(sal)](R)

-- After: hash group by
hash-group-by[dept; sum=SUM(sal)](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-hash-group-by";
    "(sort-aggregate ?groups ?aggs ?input)" =>
    "(hash-aggregate ?groups ?aggs ?input)"
    if is_database("oracle")
    if no_order_by_required("?groups")
    if estimated_groups_fit_memory("?groups", "?input")
),
```

## Preconditions

```rust
fn applicable(
    estimated_groups: f64,
    pga_target: usize,
) -> bool {
    let hash_table_bytes = estimated_groups * 100.0; // avg entry
    hash_table_bytes < pga_target as f64
}
```

**Restrictions:**
- If ORDER BY matches GROUP BY columns, sort-based is preferred
  (avoids separate sort)
- Very high group counts may cause PGA spill to temp
- _GBY_HASH_AGGREGATION_ENABLED controls this feature

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    groups: f64,
) -> f64 {
    let sort_cost = rows * rows.log2() * 0.01;
    let hash_cost = rows * 0.005 + groups * 100.0; // build + probe
    sort_cost - hash_cost
}
```

**Typical benefit**: For 10M rows with 1000 groups, hash is ~10x
faster than sort-based aggregation.

## Test Cases

```sql
-- Positive: GROUP BY without ORDER BY
SELECT department_id, SUM(salary) FROM employees GROUP BY department_id;
-- HASH GROUP BY: 1000 groups, hash table fits in memory
```

```sql
-- Negative: ORDER BY same as GROUP BY
SELECT department_id, SUM(salary) FROM employees
GROUP BY department_id ORDER BY department_id;
-- SORT GROUP BY preferred: provides sorted output for ORDER BY
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Hash Group By"
Oracle: EXPLAIN PLAN HASH GROUP BY operation
