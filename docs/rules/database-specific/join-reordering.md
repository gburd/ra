# Rule: DataFusion Join Reordering

**Category:** database-specific/datafusion
**File:** `rules/database-specific/datafusion/join-reordering.rra`

## Metadata

- **ID:** `datafusion-join-reordering`
- **Version:** "1.0.0"
- **Databases:** datafusion
- **Tags:** database-specific, datafusion, join, reorder, cost-based
- **Authors:** "RA Contributors"


# DataFusion Join Reordering

## Description

Reorders multi-way joins to minimize intermediate result sizes using
a dynamic programming algorithm.  DataFusion's join reorder pass
evaluates different join orderings and selects the one with the
lowest estimated cost based on table statistics.

**When to apply**: A query joins three or more tables and statistics
are available to estimate join selectivity.

**Why it works**: Join order dramatically affects performance.  Joining
two small-result tables first and then probing a large table can reduce
hash table sizes and intermediate Arrow RecordBatch allocations by
orders of magnitude compared to a naive left-deep plan.

**Database version**: DataFusion 28.0+

## Relational Algebra

```algebra
-- Before: left-deep plan (parse order)
(A join B) join C

-- After: reordered by cost
A join (B join C)
  where |B join C| << |A join B|
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("datafusion-join-commutativity";
    "(join ?type ?cond ?left ?right)" =>
    "(join ?type (swap-cond ?cond) ?right ?left)"
    if is_database("datafusion")
    if is_inner_join("?type")
),

rw!("datafusion-join-associativity";
    "(join inner ?cond1 (join inner ?cond2 ?a ?b) ?c)" =>
    "(join inner ?cond2 ?a (join inner ?cond1 ?b ?c))"
    if is_database("datafusion")
    if join_conditions_compatible("?cond1", "?cond2", "?a", "?b", "?c")
),
```

## Preconditions

```rust
fn applicable(joins: &[JoinNode], stats: &Statistics) -> bool {
    joins.len() >= 3
    && joins.iter().all(|j| j.join_type == JoinType::Inner)
    && stats.has_cardinality_estimates()
}
```

**Restrictions:**
- Only inner joins can be freely reordered
- Outer joins preserve order constraints
- Requires cardinality estimates for cost comparison
- Exponential in number of tables; uses DP with pruning for > 10 tables

## Cost Model

```rust
fn join_order_cost(
    left_rows: f64,
    right_rows: f64,
    selectivity: f64,
) -> f64 {
    let output_rows = left_rows * right_rows * selectivity;
    // Hash join cost: build + probe + output materialization
    let build = right_rows * 1.2;   // hash table construction
    let probe = left_rows * 0.8;    // hash lookups
    let output = output_rows * 1.0; // result batch creation
    build + probe + output
}
```

**Typical benefit**: For a 3-way join, optimal ordering can be 10x-100x
faster than worst-case ordering when table sizes vary significantly.

## Test Cases

```sql
-- Positive: 3-way join benefits from reordering
SELECT *
FROM orders o
JOIN customers c ON o.customer_id = c.id
JOIN order_items i ON o.id = i.order_id
WHERE c.country = 'US';
-- Optimal: filter customers first, join with orders, then items
```

```sql
-- Negative: 2-way join (no reordering needed)
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;
-- Only two tables; commutativity may help but not reordering
```

## References

DataFusion: datafusion/optimizer/src/join_reorder.rs
DataFusion: datafusion/optimizer/src/reorder_join.rs
Research: "Access Path Selection in a Relational DBMS" (Selinger et al.)
