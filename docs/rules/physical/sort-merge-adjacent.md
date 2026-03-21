# Rule: Adjacent Sort Merge

**Category:** physical/sort
**File:** `rules/physical/sort/sort-merge-adjacent.rra`

## Metadata

- **ID:** `sort-merge-adjacent`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, mysql, oracle
- **Tags:** physical, sort, merge, redundant, elimination
- **Authors:** "Simmen, Shekita & O'Keefe"


# Adjacent Sort Merge

## Description

When two Sort operators are adjacent (one on top of the other), the
outer sort subsumes the inner sort and the inner one can be removed.
If the outer sort specifies a superset or different order, the inner
sort work is wasted.

**When to apply**: Two adjacent Sort operators in the plan.

## Relational Algebra

```algebra
-- Before
Sort[a, b](Sort[a](R))

-- After
Sort[a, b](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw\!("sort-merge-adjacent";
    "(sort ?order1 (sort ?order2 ?input))" =>
    "(sort ?order1 ?input)"
),
```

## Preconditions

```rust
fn applicable(_outer: &Sort, _inner: &Sort) -> bool {
    true // Outer sort always overrides inner
}
```

## Cost Model

```rust
fn estimated_benefit(rows: f64) -> f64 {
    rows * (rows as f64).log2() * 0.001
}
```

## Test Cases

```sql
-- Positive: redundant sort from subquery
SELECT * FROM (SELECT * FROM t ORDER BY a) sub ORDER BY a, b;
-- Inner ORDER BY a removed, outer ORDER BY a, b kept

-- Positive: conflicting sorts
SELECT * FROM (SELECT * FROM t ORDER BY a) sub ORDER BY b;
-- Inner sort wasted, only outer sort needed
```

## References

- Simmen, D., Shekita, E. & O'Keefe, T., "Fundamental Techniques for Order Optimization", ACM SIGMOD 1996, DOI: 10.1145/233269.233320
