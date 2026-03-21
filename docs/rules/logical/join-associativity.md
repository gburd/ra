# Rule: Join Associativity

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/join-associativity.rra`

## Metadata

- **ID:** `join-associativity`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, oracle, mssql
- **Tags:** join, reordering, associativity, core
- **SQL Standard:** "sql:1992"
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner ?c2 (join inner ?c1 ?r ?s) ?t)"
    description: "Nested inner join structure"
  - type: "predicate"
    condition: "is_inner_join(?c1) && is_inner_join(?c2)"
    description: "Both joins must be inner joins"
  - type: "predicate"
    condition: "join_conds_compatible(?c1, ?c2, ?r, ?s, ?t)"
    description: "Join conditions must remain valid after reassociation"
```


# Join Associativity

## Description

Reassociates a chain of inner joins so the optimizer can explore different
join orderings. If `(R join S) join T` is the current plan, this rule
produces `R join (S join T)` and vice-versa. The optimizer's cost model
then picks the cheaper ordering.

**When to apply**: Three or more relations are joined with inner joins.

**Why it works**: Inner join is associative: the final multiset of result
tuples is the same regardless of grouping. Changing the grouping changes
intermediate result sizes, which the cost model exploits.

## Relational Algebra

```algebra
(R join[c1] S) join[c2] T -> R join[c1] (S join[c2] T)
  where c1 references attrs(R) and attrs(S)
  where c2 references attrs(S) and attrs(T)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("join-assoc-l-to-r";
    "(join inner ?c2 (join inner ?c1 ?r ?s) ?t)" =>
    "(join inner ?c1 ?r (join inner ?c2 ?s ?t))"
    if join_conds_compatible("?c1", "?c2", "?r", "?s", "?t")
),

rw!("join-assoc-r-to-l";
    "(join inner ?c1 ?r (join inner ?c2 ?s ?t))" =>
    "(join inner ?c2 (join inner ?c1 ?r ?s) ?t)"
    if join_conds_compatible("?c1", "?c2", "?r", "?s", "?t")
),
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(
    join1: JoinType,
    join2: JoinType,
    c1: &Expr,
    c2: &Expr,
) -> bool {
    // Both joins must be inner
    matches!(join1, JoinType::Inner)
        && matches!(join2, JoinType::Inner)
    // Join conditions must remain valid after reassociation:
    // c1 must reference R and S; c2 must reference S and T.
}
```

**Restrictions:**
- Both joins must be inner joins
- Join conditions must remain satisfiable after reassociation
- Does not apply to outer joins (associativity does not hold)

## Cost Model

```rust
fn estimated_benefit(
    r_card: f64,
    s_card: f64,
    t_card: f64,
    sel_c1: f64,
    sel_c2: f64,
) -> f64 {
    let cost_left_deep =
        r_card * s_card * sel_c1
        + (r_card * s_card * sel_c1) * t_card * sel_c2;
    let cost_right_deep =
        s_card * t_card * sel_c2
        + r_card * (s_card * t_card * sel_c2) * sel_c1;
    (cost_left_deep - cost_right_deep).abs()
        / cost_left_deep.max(cost_right_deep)
}
```

**Typical benefit**: Highly variable. For skewed cardinalities (e.g., a
small dimension table joined to a large fact table), reassociation can
reduce cost by orders of magnitude.

## Test Cases

```sql
-- Positive: reassociate to build smaller intermediate
-- Before (left-deep)
SELECT * FROM orders o
JOIN items i ON o.id = i.order_id
JOIN products p ON i.product_id = p.id
WHERE p.category = 'electronics';

-- After (right-deep, if products is small after filter)
SELECT * FROM orders o
JOIN (
    SELECT * FROM items i
    JOIN products p ON i.product_id = p.id
    WHERE p.category = 'electronics'
) ip ON o.id = ip.order_id;
```

```sql
-- Negative: outer joins are not associative
SELECT * FROM a
LEFT JOIN b ON a.id = b.a_id
LEFT JOIN c ON b.id = c.b_id;
-- Reassociating would change NULL semantics
```

## References

PostgreSQL: src/backend/optimizer/path/joinrels.c - make_join_rel()
DuckDB: src/optimizer/join_order/join_order_optimizer.cpp
MySQL: sql/sql_optimizer.cc - choose_table_order()
Moerkotte & Neumann "Analysis of Two Existing and One New Dynamic Programming Algorithm" (VLDB 2006)
Selinger et al. "Access Path Selection in a Relational DBMS" (SIGMOD 1979)
