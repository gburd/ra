# Rule: Convert Semi-Join to Inner (Non-Equality)

**Category:** database-specific/cockroachdb
**File:** `rules/database-specific/cockroachdb/convert-semi-to-inner-non-equality.rra`

## Metadata

- **ID:** `cockroachdb-convert-semi-to-inner-non-equality`
- **Version:** 1.0.0
- **Databases:** cockroachdb
- **Tags:** database-specific, cockroachdb, semi-join, inner-join, distinct
- **Authors:** "RA Contributors"


# Convert Semi-Join to Inner (Non-Equality)

## Description

Converts a semi-join with non-equality ON conditions to an inner join with DistinctOn. Unlike the equality case (which does distinct before join), non-equality requires distinct after the join to ensure each left row appears at most once.

**When to apply**: Semi-join where ON condition is not a simple equality, enabling lookup joins for non-covering indexes.

**Why it works**: Allows exploring inner join alternatives including lookup joins, at the cost of a post-join distinct operation.

**Database version**: CockroachDB v20.1+

## Relational Algebra

```algebra
SemiJoin[c](L, R)
  -> Project[L_cols](DistinctOn[L.key](InnerJoin[c](EnsureKey(L), R)))
  where ¬is_simple_equality(c)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("cockroachdb-convert-semi-to-inner-non-equality";
    "(semi_join ?left ?right ?on ?private)" =>
    "(project
        (distinct_on
            (inner_join
                (ensure_key ?left)
                ?right
                ?on
                ?private)
            (key_cols ?left))
        (output_cols ?left))"
    if is_database("cockroachdb")
    if not(is_simple_equality("?on"))
    if no_join_hints("?private")
),
```

## Preconditions

```rust
fn applicable(
    on_condition: &Expr,
    private: &JoinPrivate,
) -> bool {
    // Must NOT be simple equality (other rule handles that)
    !is_simple_equality(on_condition)
        // No join hints
        && !private.has_join_hints()
}
```

**Restrictions:**
- Only applies to CockroachDB
- ON condition must not be simple equality
- Requires ensuring left side has a key
- DistinctOn after join adds overhead

## Cost Model

```rust
fn estimated_benefit(
    semi_cost: f64,
    inner_cost: f64,
    distinct_overhead: f64,
) -> f64 {
    let transformed_cost = inner_cost + distinct_overhead;
    if transformed_cost < semi_cost {
        (semi_cost - transformed_cost) / semi_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 20-60% when enabling efficient join methods

## Test Cases

```sql
SELECT * FROM orders
WHERE EXISTS (
  SELECT 1 FROM customers
  WHERE orders.amount < customers.credit_limit
);

-- Non-equality condition: transform to inner join + distinct
```

## References

**Source code:**
- CockroachDB: `pkg/sql/opt/xform/rules/join.opt`
  - Rule: `ConvertSemiToInnerJoin` (lines 94-125)
  - Commit: 6e210ba6aa33cea5e27b1a8fae212c27941781f4
