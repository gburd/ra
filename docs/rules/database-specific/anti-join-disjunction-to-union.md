# Rule: Split Disjunctive Anti-Join into Intersection

**Category:** database-specific/cockroachdb
**File:** `rules/database-specific/cockroachdb/anti-join-disjunction-to-union.rra`

## Metadata

- **ID:** `cockroachdb-anti-join-disjunction-to-union`
- **Version:** 1.0.0
- **Databases:** cockroachdb
- **Tags:** database-specific, cockroachdb, anti-join, disjunction, set-operations
- **Authors:** "RA Contributors"


# Split Disjunctive Anti-Join into Intersection

## Description

Splits disjunctions (OR) in anti-join ON clauses into an intersection of two anti-joins. Unlike inner/semi joins which use union for OR splits, anti-joins use intersection because they return rows that DON'T match ANY condition.

**When to apply**: Anti-join with OR in the ON clause where both disjuncts can benefit from index access.

**Why it works**: For anti-join, (NOT (c1 OR c2)) = (NOT c1 AND NOT c2). Splitting allows each anti-join to use appropriate indexes, then intersecting the results gives rows matching neither condition.

**Database version**: CockroachDB v20.1+

## Relational Algebra

```algebra
AntiJoin[c1 OR c2](L, R)
  -> Intersect(
       AntiJoin[c1](L, R),
       AntiJoin[c2](L, R)
     )
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("cockroachdb-anti-join-disjunction-to-intersection";
    "(anti_join ?left ?right (or ?c1 ?c2) ?private)" =>
    "(intersect
        (anti_join ?left ?right ?c1 ?private)
        (anti_join ?left ?right ?c2 ?private))"
    if is_database("cockroachdb")
    if can_split_anti_join_disjuncts("?c1", "?c2")
),
```

## Preconditions

```rust
fn applicable(
    c1: &Expr,
    c2: &Expr,
    left: &RelNode,
    right: &RelNode,
) -> bool {
    // Both disjuncts should benefit from indexes
    let c1_indexed = left.has_index_for_predicate(c1)
                  || right.has_index_for_predicate(c1);
    let c2_indexed = left.has_index_for_predicate(c2)
                  || right.has_index_for_predicate(c2);

    c1_indexed && c2_indexed
}
```

**Restrictions:**
- Only applies to CockroachDB anti-joins
- Requires indexes on both disjuncts for benefit
- Uses intersection instead of union (unlike inner joins)

## Cost Model

```rust
fn estimated_benefit(
    cross_anti_cost: f64,
    indexed_anti1_cost: f64,
    indexed_anti2_cost: f64,
    intersect_cost: f64,
) -> f64 {
    let split_cost = indexed_anti1_cost + indexed_anti2_cost + intersect_cost;
    (cross_anti_cost - split_cost) / cross_anti_cost
}
```

**Typical benefit**: 20-60% with indexed disjuncts

## References

**Source code:**
- CockroachDB: `pkg/sql/opt/xform/rules/join.opt`
  - Rule: `SplitDisjunctionOfAntiJoinTerms` (lines 198+)
  - Commit: 6e210ba6aa33cea5e27b1a8fae212c27941781f4
