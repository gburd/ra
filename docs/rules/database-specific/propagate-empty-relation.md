# Rule: DataFusion Propagate Empty Relation

**Category:** database-specific/datafusion
**File:** `rules/database-specific/datafusion/propagate-empty-relation.rra`

## Metadata

- **ID:** `datafusion-propagate-empty-relation`
- **Version:** "1.0.0"
- **Databases:** datafusion
- **Tags:** database-specific, datafusion, empty, propagation, pruning
- **Authors:** "RA Contributors"


# DataFusion Propagate Empty Relation

## Description

Detects that a subtree produces zero rows and propagates this
emptiness upward, replacing entire plan branches with empty relations.
DataFusion's `PropagateEmptyRelation` pass eliminates unnecessary
computation when the optimizer proves a branch cannot produce output.

**When to apply**: A plan node's input is known to produce zero rows,
either from contradictory filters, empty tables, or LIMIT 0.

**Why it works**: If a child produces no rows, many parent operators
also produce no rows (filter, project, sort, inner join).  Replacing
the subtree with an empty relation avoids all computation, I/O, and
memory allocation for that branch.

**Database version**: DataFusion 25.0+

## Relational Algebra

```algebra
-- Filter contradiction
sigma[false](R) -> empty(schema(R))

-- Project over empty
pi[cols](empty) -> empty(schema(cols))

-- Inner join with empty side
R inner-join empty -> empty(schema(R ++ S))

-- LIMIT 0
limit[0](R) -> empty(schema(R))

-- Union with empty branch
R union-all empty -> R
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("datafusion-empty-filter";
    "(filter (const-bool false) ?input)" =>
    "(empty-relation (schema-of ?input))"
    if is_database("datafusion")
),

rw!("datafusion-empty-inner-join-left";
    "(join inner ?cond (empty-relation ?schema) ?right)" =>
    "(empty-relation (merge-schemas ?schema (schema-of ?right)))"
    if is_database("datafusion")
),

rw!("datafusion-empty-inner-join-right";
    "(join inner ?cond ?left (empty-relation ?schema))" =>
    "(empty-relation (merge-schemas (schema-of ?left) ?schema))"
    if is_database("datafusion")
),

rw!("datafusion-empty-limit-zero";
    "(limit (const-int 0) ?input)" =>
    "(empty-relation (schema-of ?input))"
    if is_database("datafusion")
),

rw!("datafusion-empty-union-left";
    "(union-all (empty-relation ?schema) ?right)" =>
    "?right"
    if is_database("datafusion")
),

rw!("datafusion-empty-union-right";
    "(union-all ?left (empty-relation ?schema))" =>
    "?left"
    if is_database("datafusion")
),
```

## Preconditions

```rust
fn applicable(plan: &LogicalPlan) -> bool {
    plan.inputs().iter().any(|input| {
        matches!(input, LogicalPlan::EmptyRelation { .. })
    })
    || is_always_false_filter(plan)
    || is_limit_zero(plan)
}
```

**Restrictions:**
- Outer joins preserve the non-empty side (LEFT JOIN with empty right
  still produces rows with NULLs)
- Aggregates without GROUP BY produce one row even with empty input
  (e.g., COUNT(*) returns 0)

## Cost Model

```rust
fn estimated_benefit(plan: &LogicalPlan) -> f64 {
    // Eliminates entire subtree cost
    plan.estimated_total_cost()
}
```

**Typical benefit**: Eliminates entire branches of complex queries
when contradictions are detected, from seconds to microseconds.

## Test Cases

```sql
-- Positive: contradictory filter
SELECT * FROM events WHERE 1 = 0;
-- Entire scan eliminated, returns empty result
```

```sql
-- Positive: LIMIT 0
SELECT * FROM events LIMIT 0;
-- No scan performed
```

```sql
-- Negative: aggregate on empty (returns one row)
SELECT COUNT(*) FROM events WHERE 1 = 0;
-- Returns single row: count = 0
```

## References

DataFusion: datafusion/optimizer/src/propagate_empty_relation.rs
DataFusion: datafusion/optimizer/src/eliminate_limit.rs
