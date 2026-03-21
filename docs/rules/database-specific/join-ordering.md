# Rule: Apache Derby Cost-Based Join Ordering

**Category:** database-specific/derby
**File:** `rules/database-specific/derby/join-ordering.rra`

## Metadata

- **ID:** `derby-join-ordering`
- **Version:** "1.0.0"
- **Databases:** derby
- **Tags:** database-specific, derby, join-ordering, cost-based, optimizer
- **Authors:** "RA Contributors"


# Apache Derby Cost-Based Join Ordering

## Description

Derby uses a cost-based optimizer that enumerates join orderings to
find the cheapest plan.  For small numbers of tables (<= 6), Derby
explores all permutations.  For larger join graphs, it uses a greedy
heuristic.  The cost model considers row counts, index availability,
join selectivity, and I/O costs.

**When to apply**: A query joins three or more tables and the
optimizer can evaluate multiple orderings.

**Why it works**: The optimal join order can be orders of magnitude
faster than the worst order.  Placing selective joins first reduces
intermediate result sizes, decreasing overall I/O and CPU cost.

**Database version**: Apache Derby 10.1+

## Relational Algebra

```algebra
-- Before: left-to-right order
(A join B) join C

-- After: reordered by cost
(B join C) join A
-- If B join C is more selective
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derby-join-reorder";
    "(join ?pred1
        (join ?pred2 ?a ?b) ?c)" =>
    "(join ?pred2
        ?a (join ?pred1 ?b ?c))"
    if is_database("derby")
    if reorder_reduces_cost("?pred1", "?pred2",
        "?a", "?b", "?c")
),
```

## Preconditions

```rust
fn applicable(tables: &[Table]) -> bool {
    tables.len() >= 3
}
```

**Restrictions:**
- Exhaustive enumeration limited to 6 tables (by default)
- `derby.language.maxJoinOrderOptimization` controls the threshold
- Outer joins restrict reordering (preserved/null-supplying sides)
- Cross joins are placed last

## Cost Model

```rust
fn estimated_benefit(
    original_cost: f64,
    reordered_cost: f64,
) -> f64 {
    original_cost - reordered_cost
}
```

**Typical benefit**: 2-100x for star-schema queries where placing
dimension filters first drastically reduces fact table lookups.

## Test Cases

```sql
-- Positive: star schema join reordering
SELECT * FROM sales s
JOIN products p ON s.prod_id = p.id
JOIN stores st ON s.store_id = st.id
WHERE st.region = 'East';
-- Derby reorders: stores(filtered) -> sales -> products
```

```sql
-- Negative: two-table join (no reordering)
SELECT * FROM a JOIN b ON a.x = b.y;
-- Only one possible order
```

## References

Apache Derby: Derby Technical Architecture, "Query Optimization"
Source: org.apache.derby.impl.sql.compile.OptimizerImpl
