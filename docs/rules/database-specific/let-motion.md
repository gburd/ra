# Rule: Materialize Let Motion (Common Subexpression Hoisting)

**Category:** database-specific/materialize
**File:** `rules/database-specific/materialize/let-motion.rra`

## Metadata

- **ID:** `materialize-let-motion`
- **Version:** "1.0.0"
- **Databases:** materialize
- **Tags:** database-specific, materialize, let, cse, hoisting, sharing
- **Authors:** "RA Contributors"


# Materialize Let Motion (Common Subexpression Hoisting)

## Description

Identifies common subexpressions in the MIR (Mid-level Intermediate
Representation) plan and hoists them into Let bindings.  This ensures
that shared subgraphs are computed once and their differential dataflow
arrangement is reused across all references.

**When to apply**: The same subexpression (identical MIR subtree)
appears multiple times in the plan.

**Why it works**: Without Let bindings, each occurrence of a
subexpression becomes a separate differential dataflow subgraph with
its own operators and arrangements.  Hoisting into a Let binding
creates a single subgraph whose output is shared, eliminating
duplicate computation and memory.

**Database version**: Materialize 0.20+

## Relational Algebra

```algebra
-- Before: duplicated subexpression
union-all(
    sigma[p1](join(R, S)),
    sigma[p2](join(R, S)))

-- After: hoisted into Let
let common = join(R, S) in
union-all(sigma[p1](common), sigma[p2](common))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("materialize-let-hoisting";
    "(union-all (filter ?p1 ?common) (filter ?p2 ?common))" =>
    "(let-binding ?common
        (union-all (filter ?p1 (get ?common))
                   (filter ?p2 (get ?common))))"
    if is_database("materialize")
),
```

## Preconditions

```rust
fn applicable(plan: &MirRelationExpr) -> bool {
    let subtrees = collect_subtrees(plan);
    subtrees.iter().any(|(_, count)| *count >= 2)
}
```

**Restrictions:**
- Subtrees must be structurally identical (not just semantically)
- Let bindings increase arrangement sharing but may prevent other
  optimizations that require operator locality
- Recursive Let bindings (WITH RECURSIVE) follow different rules

## Cost Model

```rust
fn estimated_benefit(
    subtree_cost: f64,
    occurrences: usize,
) -> f64 {
    // Eliminate (occurrences - 1) copies of the subtree
    (occurrences - 1) as f64 * subtree_cost
}
```

**Typical benefit**: For a complex join used in 3 branches of a UNION,
eliminates 2 duplicate dataflow subgraphs and their arrangements.

## Test Cases

```sql
-- Positive: CTE used multiple times
WITH active_users AS (
    SELECT * FROM users WHERE active = true
)
SELECT * FROM active_users WHERE age > 30
UNION ALL
SELECT * FROM active_users WHERE department = 'eng';
-- active_users computed once, shared via Let binding
```

```sql
-- Negative: unique subexpressions
SELECT * FROM users WHERE age > 30
UNION ALL
SELECT * FROM orders WHERE total > 100;
-- No common subexpression to hoist
```

## References

Materialize: src/transform/src/cse/let_motion.rs
Materialize: src/transform/src/cse/map.rs
