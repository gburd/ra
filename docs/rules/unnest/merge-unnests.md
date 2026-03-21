# Rule: Merge Adjacent Unnests into Zip

**Category:** logical/unnest
**File:** `rules/unnest/merge-unnests.rra`

## Metadata

- **ID:** `merge-adjacent-unnests`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** unnest, merge, zip, array, multi-column
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: pattern
    must_match: "(join ?cond (unnest ?arr1) (unnest ?arr2))"
  - type: predicate
    condition: "arrays_from_same_source(?arr1, ?arr2)"
    description: "Both arrays originate from the same input relation"
```


# Merge Adjacent Unnests into Zip

## Description

When two unnest operators are joined (cross-joined or equi-joined on
row position) and their array sources come from the same input row,
merge them into a single multi-column unnest (zip). This avoids
producing a Cartesian product that must then be filtered to matching
ordinal positions.

**When to apply**: Two unnest operators are cross-joined or joined
on ordinal position, and both arrays originate from the same row
of an outer relation.

**Why it works**: PostgreSQL's multi-argument UNNEST zips arrays
in lockstep: `unnest(arr1, arr2)` produces rows `(arr1[i], arr2[i])`
for each index `i`. This is equivalent to the join of two separate
unnests filtered to matching ordinality, but without the quadratic
intermediate result.

## Relational Algebra

```algebra
-- Before: cross join of two unnests (O(n*m) intermediate rows)
unnest(arr1) AS u1_alias (a) CROSS JOIN unnest(arr2) AS u2_alias (b)
  WHERE u1.ordinality = u2.ordinality

-- After: single zip unnest (O(max(n,m)) rows)
zip_unnest(arr1, arr2) AS result (a, b)
```

### Lateral variant

```algebra
-- Before: two lateral unnests from same input
R join_lateral unnest(R.arr1) AS u1_alias (a)
  join_lateral unnest(R.arr2) AS u2_alias (b)
  WHERE u1.ord = u2.ord

-- After: single multi-column lateral unnest
R join_lateral zip_unnest(R.arr1, R.arr2) AS result (a, b)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Merge cross-joined unnests with ordinality equality
rw!("merge-unnests-cross-join";
    "(filter (= (ordinality ?u1) (ordinality ?u2))
       (cross-join (unnest ?arr1) (unnest ?arr2)))" =>
    "(zip-unnest ?arr1 ?arr2)"
    if arrays_from_same_source("?arr1", "?arr2")
),

// Merge lateral unnests from same input relation
rw!("merge-lateral-unnests";
    "(lateral-join ?input
       (lateral-join ?input
         (unnest ?arr1))
       (unnest ?arr2))" =>
    "(lateral-join ?input (zip-unnest ?arr1 ?arr2))"
    if arrays_from_same_source("?arr1", "?arr2")
),

// PostgreSQL multi-argument UNNEST syntax
rw!("multi-arg-unnest";
    "(cross-join (unnest ?arr1) (unnest ?arr2))" =>
    "(zip-unnest ?arr1 ?arr2)"
    if arrays_have_equal_length("?arr1", "?arr2")
),
```

## Preconditions

```rust
fn arrays_from_same_source(arr1: &Expr, arr2: &Expr) -> bool {
    // Both arrays must come from the same base relation row.
    // This ensures lockstep iteration is semantically correct.
    match (arr1, arr2) {
        (Expr::Column(c1), Expr::Column(c2)) => {
            c1.table_ref() == c2.table_ref()
        }
        _ => false,
    }
}

fn arrays_have_equal_length(arr1: &Expr, arr2: &Expr) -> bool {
    // For literal arrays, check lengths match.
    // For column references, rely on schema metadata
    // or assume equal length (PostgreSQL pads with NULL).
    match (arr1, arr2) {
        (Expr::Array(a), Expr::Array(b)) => a.len() == b.len(),
        _ => true, // PostgreSQL pads shorter arrays with NULLs
    }
}
```

**Restrictions:**
- Arrays must originate from the same input relation (same correlation scope).
- Join condition must be on ordinality/positional equality, not arbitrary predicates.
- If arrays have different lengths, the zip uses NULL padding for shorter arrays (PostgreSQL semantics).

## Cost Model

```rust
fn estimated_benefit(
    avg_len_arr1: f64,
    avg_len_arr2: f64,
) -> f64 {
    // Before: cross join produces len1 * len2 rows,
    // then filter to matching positions keeps max(len1, len2).
    let cross_product = avg_len_arr1 * avg_len_arr2;

    // After: zip produces max(len1, len2) rows directly.
    let zip_rows = avg_len_arr1.max(avg_len_arr2);

    (cross_product - zip_rows) / cross_product
}
```

**Typical benefit**: 30-70%. For arrays of length N, eliminates
N^2 - N intermediate rows from the cross product.

## Test Cases

### Positive: parallel array unnest

```sql
-- Before
SELECT u1.id, u2.name
FROM unnest(ARRAY[1, 2, 3]) WITH ORDINALITY AS u1_alias (id, ord1),
     unnest(ARRAY['a', 'b', 'c']) WITH ORDINALITY AS u2_alias (name, ord2)
WHERE u1.ord1 = u2.ord2;

-- After (PostgreSQL multi-arg UNNEST)
SELECT id, name
FROM unnest(ARRAY[1, 2, 3], ARRAY['a', 'b', 'c']) AS unnest_result (id, name);
```

### Positive: lateral parallel unnest from same row

```sql
-- Before
SELECT p.name, u1.tag, u2.score
FROM products p,
  LATERAL unnest(p.tags) WITH ORDINALITY AS u1_alias (tag, o1),
  LATERAL unnest(p.scores) WITH ORDINALITY AS u2_alias (score, o2)
WHERE u1.o1 = u2.o2;

-- After
SELECT p.name, t.tag, t.score
FROM products p,
  LATERAL unnest(p.tags, p.scores) AS unnest_result (tag, score);
```

### Negative: arrays from different relations

```sql
-- Cannot merge: arrays come from different base tables
SELECT u1.val, u2.val
FROM table_a a, unnest(a.arr1) AS u1_alias (val),
     table_b b, unnest(b.arr2) AS u2_alias (val);

-- No common source, so zip semantics would be incorrect.
```

### Negative: join condition is not positional

<div v-pre>

```sql
-- Cannot merge: join is on value equality, not position
SELECT u1.id, u2.name
FROM unnest(ARRAY[1, 2, 3]) AS u1_alias (id),
     unnest(ARRAY['a', 'b', 'c']) AS u2_alias (name)
WHERE u1.id = length(u2.name);

-- Arbitrary predicate, not positional alignment.
```

</div>

## References

- PostgreSQL: ExecScanReScan for multi-arg UNNEST handling
- DuckDB: PhysicalUnnest with multiple list columns
- SQL:2016 Section 7.6 <collection derived table>
