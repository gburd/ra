# Rule: MonetDB Candidate List Intersection

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/cand-list-intersection.rra`

## Metadata

- **ID:** `monetdb-cand-list-intersection`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, candidate-list, intersection, OID, AND
- **Authors:** "RA Contributors"


# MonetDB Candidate List Intersection

## Description

When multiple selection predicates apply to the same table, MonetDB
evaluates each predicate independently to produce candidate OID lists,
then intersects them.  This is MonetDB's equivalent of AND-combining
predicates without requiring a composite index.

**When to apply**: Multiple independent filter predicates apply to
different columns of the same table.

**Why it works**: Each selection produces a sorted OID list.
Intersecting sorted lists is O(n+m) using merge-intersection.  This
avoids the need for composite indexes and allows each predicate to
use the most efficient column-specific method (imprints, range
select, hash).

**Database version**: MonetDB 5+

## Relational Algebra

```algebra
-- Before: combined filter
sigma[price > 100 AND qty < 5](scan(products))

-- After: separate selections + intersection
intersect(
    thetaselect(products.price, >, 100),
    thetaselect(products.qty, <, 5)
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-cand-intersection";
    "(filter (and ?pred1 ?pred2) (scan ?table))" =>
    "(fetch ?table
        (intersect-cands
            (theta-select-col ?table ?pred1)
            (theta-select-col ?table ?pred2)))"
    if is_database("monetdb")
    if predicates_on_different_columns("?pred1", "?pred2")
),
```

## Preconditions

```rust
fn applicable(
    pred1: &Predicate,
    pred2: &Predicate,
) -> bool {
    pred1.columns() != pred2.columns()
    && pred1.is_simple_comparison()
    && pred2.is_simple_comparison()
}
```

**Restrictions:**
- Intersection of very large candidate lists may be slower than
  evaluating predicates sequentially with early termination
- OR predicates use union instead of intersection
- Candidate lists must be sorted for merge-intersection

## Cost Model

```rust
fn estimated_benefit(
    cand1_size: f64,
    cand2_size: f64,
) -> f64 {
    let combined_scan =
        (cand1_size + cand2_size) * 0.0005;
    combined_scan
}
```

**Typical benefit**: Enables multi-column filtering without
composite indexes.

## Test Cases

```sql
-- Positive: AND on separate columns
SELECT * FROM products
WHERE price > 100 AND category_id = 5;
-- Separate OID lists intersected
```

```sql
-- Negative: predicates on same column
SELECT * FROM products WHERE price > 100 AND price < 200;
-- Single range select, not intersection
```

## References

MonetDB: BAT algebra candidate list operations
Source: gdk/gdk_select.c, `BATintersect()`
