# Rule: MonetDB BAT Join Ordering

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/bat-join-ordering.rra`

## Metadata

- **ID:** `monetdb-bat-join-ordering`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, BAT, join-ordering, columnar
- **Authors:** "RA Contributors"


# MonetDB BAT Join Ordering

## Description

MonetDB's optimizer reorders joins in the BAT (Binary Association
Table) algebra layer to minimize intermediate result sizes.  Unlike
row-store optimizers that reason about row cardinalities, MonetDB's
join ordering considers the column-at-a-time execution model where
intermediate BATs consume memory proportional to the number of
qualifying OIDs (object identifiers).

**When to apply**: A multi-way join involves three or more BATs and
the optimizer can estimate the selectivity of each join predicate.

**Why it works**: In MonetDB's columnar model, each join produces a
pair of OID vectors mapping qualifying rows.  Reordering joins to
produce the smallest intermediate OID vectors first reduces memory
pressure and improves cache utilization in subsequent joins.

**Database version**: MonetDB 5 (SQL/MIL) and MonetDB 11+

## Relational Algebra

```algebra
-- Before: left-to-right join order
(A join[A.x = B.y] B) join[B.z = C.w] C

-- After: reordered by estimated intermediate size
(B join[B.z = C.w] C) join[A.x = B.y] A
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-bat-join-reorder";
    "(join ?pred1
        (join ?pred2 ?rel_a ?rel_b)
        ?rel_c)" =>
    "(join ?pred2
        ?rel_a
        (join ?pred1 ?rel_b ?rel_c))"
    if is_database("monetdb")
    if reduces_intermediate_size("?pred1", "?pred2",
        "?rel_a", "?rel_b", "?rel_c")
),
```

## Preconditions

```rust
fn applicable(
    joins: &[JoinPredicate],
    relations: &[Relation],
) -> bool {
    relations.len() >= 3
    && joins.iter().all(|j| j.has_selectivity_estimate())
    && !joins.iter().any(|j| j.is_outer_join())
}
```

**Restrictions:**
- Only applies to inner joins; outer joins have fixed ordering
  constraints
- Requires cardinality estimates from column statistics (imprints,
  histograms)
- MonetDB uses a greedy heuristic for large join graphs (>10 tables)
  rather than exhaustive enumeration

## Cost Model

```rust
fn estimated_benefit(
    original_intermediate_rows: f64,
    reordered_intermediate_rows: f64,
    bat_width: usize,
) -> f64 {
    let original_cost =
        original_intermediate_rows * bat_width as f64;
    let reordered_cost =
        reordered_intermediate_rows * bat_width as f64;
    original_cost - reordered_cost
}
```

**Typical benefit**: 2-10x improvement for star-schema joins where
dimension table joins are highly selective.

## Test Cases

```sql
-- Positive: three-way join reorderable by selectivity
SELECT * FROM lineitem l
JOIN orders o ON l.l_orderkey = o.o_orderkey
JOIN customer c ON o.o_custkey = c.c_custkey
WHERE c.c_nationkey = 5;
-- MonetDB reorders: customer(filtered) -> orders -> lineitem
```

```sql
-- Negative: two-way join (no reordering possible)
SELECT * FROM orders o
JOIN lineitem l ON o.o_orderkey = l.l_orderkey;
-- Only two relations; nothing to reorder
```

## References

MonetDB: "Optimizing Join Enumeration in Transformation-based
Query Optimizers" (CWI technical report)
Source: monetdb5/optimizer/opt_joinorder.c
MonetDB: BAT algebra documentation
