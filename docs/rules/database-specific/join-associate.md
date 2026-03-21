# Rule: Calcite JoinAssociateRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/join-associate.rra`

## Metadata

- **ID:** `calcite-join-associate`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, join, associate, reorder
- **Authors:** "RA Contributors"


# Calcite JoinAssociateRule

## Description

Applies join associativity to transform `(A JOIN B) JOIN C`
into `A JOIN (B JOIN C)`. This is distinct from
`JoinPushThroughJoinRule` in that it changes the tree shape
from left-deep to right-deep (or vice versa), enabling the
optimizer to explore bushy join trees.

**When to apply**: Three-way inner join where the join
conditions are compatible with reassociation.

**Why it works**: Different tree shapes lead to different
intermediate result sizes. Bushy trees can be more efficient
than strictly left-deep or right-deep trees.

**Calcite class**: `org.apache.calcite.rel.rules.JoinAssociateRule`

## Relational Algebra

```algebra
-- Before: left-deep
(A join[p1] B) join[p2] C

-- After: right-deep
A join[p1'] (B join[p2'] C)
  where p2' contains predicates between B and C
  where p1' contains predicates between A and (B join C)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-join-associate";
    "(join inner ?c2
        (join inner ?c1 ?a ?b)
        ?c)" =>
    "(join inner ?c1_new
        ?a
        (join inner ?c2_new ?b ?c))"
    if can_reassociate("?c1", "?c2", "?a", "?b", "?c")
),
```

## Preconditions

```rust
fn applicable(
    cond1: &Expr,
    cond2: &Expr,
    b_cols: &[Column],
    c_cols: &[Column],
) -> bool {
    // cond2 must reference only B and C columns
    // (not A columns) for the inner join to form
    let cond2_refs = cond2.referenced_columns();
    cond2_refs.iter().all(|c| {
        b_cols.contains(c) || c_cols.contains(c)
    })
}
```

**Restrictions:**
- Only applies to inner joins
- Join conditions must be decomposable between the new pairs

## Cost Model

```rust
fn estimated_benefit(
    card_a: f64,
    card_b: f64,
    card_c: f64,
    sel_bc: f64,
) -> f64 {
    let left_deep_cost = card_a * card_b + card_a * card_b * card_c;
    let right_deep_cost = card_b * card_c * sel_bc + card_a * card_b * card_c * sel_bc;
    if right_deep_cost < left_deep_cost {
        (left_deep_cost - right_deep_cost) / left_deep_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 10-70% when the new inner join is
significantly more selective.

## Test Cases

```sql
-- Positive: reassociate to join small tables first
-- Before: (orders JOIN customers) JOIN products
-- After: orders JOIN (customers JOIN products)
-- Beneficial when customers-products join is small
SELECT * FROM orders o
JOIN customers c ON o.cust_id = c.id
JOIN products p ON c.pref_product = p.id;
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/JoinAssociateRule.java
Ono & Lohman: "Measuring the Complexity of Join Enumeration in Query Optimization" (VLDB 1990)
