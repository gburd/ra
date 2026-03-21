# Rule: mssql Spool Optimization

**Category:** database-specific/mssql
**File:** `rules/database-specific/mssql/spool-optimization.rra`

## Metadata

- **ID:** `mssql-spool-optimization`
- **Version:** "1.0.0"
- **Databases:** mssql
- **Tags:** database-specific, mssql, spool, eager, lazy, table-spool, index-spool
- **Authors:** "RA Contributors"


# mssql Spool Optimization

## Description

Introduces spool operators to cache intermediate results when the same
subtree is accessed multiple times.  mssql uses eager spools
(materialize all rows first), lazy spools (materialize on demand),
and index spools (build a temporary index) to avoid re-executing
expensive subplans.

**When to apply**: A subplan is referenced multiple times in the
execution plan (e.g., self-joins, subqueries that correlate on
different predicates, or Halloween protection).

**Why it works**: Without spooling, the same subtree is executed
multiple times.  A spool materializes the result into tempdb once and
replays it for subsequent accesses, converting O(N * cost) to
O(cost + N * replay_cost).

**Database version**: mssql 2000+

## Relational Algebra

```algebra
-- Before: subplan executed twice
union-all(
    sigma[p1](expensive_subplan),
    sigma[p2](expensive_subplan))

-- After: spool caches subplan result
let spool = eager-spool(expensive_subplan) in
union-all(sigma[p1](spool), sigma[p2](spool))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mssql-eager-spool-shared-subtree";
    "(union-all (filter ?p1 ?common) (filter ?p2 ?common))" =>
    "(let-spool ?common
        (union-all (filter ?p1 (replay-spool ?common))
                   (filter ?p2 (replay-spool ?common))))"
    if is_database("mssql")
    if subtree_is_expensive("?common")
),

rw!("mssql-index-spool-for-nl-join";
    "(nested-loop-join ?cond ?outer
        (filter ?pred (scan ?table)))" =>
    "(nested-loop-join ?cond ?outer
        (index-spool ?pred (scan ?table)))"
    if is_database("mssql")
    if no_suitable_index("?table", "?pred")
    if outer_row_count_high("?outer")
),
```

## Preconditions

```rust
fn applicable(subtree: &PhysicalPlan, references: usize) -> bool {
    references >= 2
    && subtree.estimated_cost() > spool_overhead()
}
```

**Restrictions:**
- Spools use tempdb; large spools may cause tempdb contention
- Lazy spools are preferred when only a subset of rows is needed
- Index spools add B-tree build overhead; beneficial only for many lookups
- Eager spools block the pipeline (no streaming)

## Cost Model

```rust
fn spool_benefit(
    subtree_cost: f64,
    references: usize,
    spool_overhead: f64,
) -> f64 {
    let without = subtree_cost * references as f64;
    let with_spool = subtree_cost + spool_overhead
        + (references - 1) as f64 * 0.001 * subtree_cost;
    without - with_spool
}
```

**Typical benefit**: For an expensive subplan referenced 10 times,
spool reduces from 10x execution to 1x + 9x replay.

## Test Cases

```sql
-- Positive: CTE referenced twice
WITH expensive AS (
    SELECT customer_id, SUM(amount) AS total
    FROM orders GROUP BY customer_id
)
SELECT * FROM expensive WHERE total > 1000
UNION ALL
SELECT * FROM expensive WHERE total < 100;
-- Spool materializes expensive CTE once
```

```sql
-- Negative: subplan is trivial
SELECT * FROM small_table
UNION ALL
SELECT * FROM small_table;
-- Spool overhead exceeds re-scan cost
```

## References

mssql: Spool Operator (Showplan)
mssql: Understanding Spools in Execution Plans
