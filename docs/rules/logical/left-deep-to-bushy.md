# Rule: Left-Deep to Bushy Join Tree

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/left-deep-to-bushy.rra`

## Metadata

- **ID:** `left-deep-to-bushy`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, oracle, mssql
- **Tags:** join, reordering, bushy, parallel, core
- **SQL Standard:** "sql:1992"
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner ?c3 (join inner ?c2 (join inner ?c1 ?r ?s) ?t) ?u)"
    description: "Left-deep chain of 4+ inner joins"
  - type: "predicate"
    condition: "all_inner_joins(?c1, ?c2, ?c3)"
    description: "All joins must be inner joins"
  - type: "predicate"
    condition: "has_independent_subtrees(?r, ?s, ?t, ?u)"
    description: "Independent sub-trees must exist for bushy form"
```


# Left-Deep to Bushy Join Tree

## Description

Transforms a left-deep join tree into a bushy tree by grouping independent
sub-joins. In a left-deep tree, each join takes the result of the previous
join as its left input, forming a chain. A bushy tree can join independent
sub-trees in parallel, reducing total latency.

**When to apply**: Four or more tables in a left-deep chain where some
pairs can be joined independently.

**Why it works**: If `(((R join S) join T) join U)` contains independent
sub-joins `R join S` and `T join U`, the bushy form
`(R join S) join (T join U)` can execute both sub-joins in parallel.

## Relational Algebra

```algebra
((R join[c1] S) join[c2] T) join[c3] U
  -> (R join[c1] S) join[c4] (T join[c3] U)
  where c2 can be decomposed into c4 connecting the two sub-trees
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("left-deep-to-bushy";
    "(join inner ?c3
        (join inner ?c2
            (join inner ?c1 ?r ?s)
            ?t)
        ?u)" =>
    "(join inner ?c_new
        (join inner ?c1 ?r ?s)
        (join inner ?c3_local ?t ?u))"
    if independent_joins("?c1", "?c3_local", "?r", "?s", "?t", "?u")
),
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(
    joins: &[JoinNode],
) -> bool {
    // All joins must be inner
    joins.iter().all(|j| matches!(j.kind, JoinType::Inner))
    // Must have at least 4 tables
    && joins.len() >= 3
    // Must have independent sub-trees
    && has_independent_subtrees(joins)
}
```

**Restrictions:**
- All joins must be inner joins
- Independent sub-trees must exist (join conditions reference disjoint sets)
- Not all left-deep trees can become bushy (dependencies may force ordering)

## Cost Model

```rust
fn estimated_benefit(
    sub_tree_costs: &[f64],
) -> f64 {
    // Left-deep: sequential execution, total = sum of costs
    let sequential: f64 = sub_tree_costs.iter().sum();
    // Bushy: parallel execution, total = max of parallel branches
    let parallel: f64 = sub_tree_costs
        .iter()
        .copied()
        .fold(0.0_f64, f64::max);
    (sequential - parallel) / sequential
}
```

**Typical benefit**: 0.3-0.7 with parallel execution. Without parallelism,
bushy trees may still win by reducing intermediate result sizes.

## Test Cases

```sql
-- Positive: independent pairs can be joined in parallel
-- Before (left-deep)
SELECT * FROM orders o
JOIN items i ON o.id = i.order_id
JOIN customers c ON o.cust_id = c.id
JOIN regions r ON c.region_id = r.id;

-- After (bushy: orders-items and customers-regions in parallel)
SELECT * FROM (
    SELECT * FROM orders o JOIN items i ON o.id = i.order_id
) oi JOIN (
    SELECT * FROM customers c JOIN regions r ON c.region_id = r.id
) cr ON oi.cust_id = cr.id;
```

```sql
-- Negative: all joins depend on previous result
SELECT * FROM a
JOIN b ON a.x = b.x
JOIN c ON b.y = c.y
JOIN d ON c.z = d.z;
-- Fully dependent chain, cannot form bushy tree
```

## References

PostgreSQL: src/backend/optimizer/geqo/ (genetic optimizer explores bushy trees)
DuckDB: src/optimizer/join_order/join_order_optimizer.cpp
Moerkotte & Neumann "Analysis of Two Existing and One New Dynamic Programming Algorithm" (VLDB 2006)
Ioannidis & Kang "Left-Deep vs. Bushy Trees" (VLDB 1991)
