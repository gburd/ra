# Rule: Convert Lateral Unnest to Semi-Join

**Category:** logical/unnest
**File:** `rules/unnest/lateral-to-semi-join.rra`

## Metadata

- **ID:** `lateral-unnest-to-semi-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, oracle, mssql
- **Tags:** unnest, lateral, semi-join, exists, decorrelation
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: pattern
    must_match: "(lateral-join ?input (unnest ?arr))"
  - type: predicate
    condition: "only_checks_existence(?outer_context)"
    description: "Outer query only tests whether unnest produces any rows"
```


# Convert Lateral Unnest to Semi-Join

## Description

When a lateral unnest is used only to check whether an array has
matching elements (via EXISTS, IN, or a count > 0 check), replace
the lateral join + unnest with a semi-join on an array containment
predicate. This avoids expanding the array into rows entirely.

**When to apply**: The lateral unnest result is consumed only by
an existence check (EXISTS wrapper, IS NOT NULL on the unnested
column, or aggregation that tests non-emptiness).

**Why it works**: `EXISTS(SELECT 1 FROM unnest(arr) u WHERE u = x)`
is equivalent to `x = ANY(arr)` or `arr @> ARRAY[x]`. The semi-join
avoids materializing the unnested rows and can leverage array indexes
(e.g., PostgreSQL GIN indexes).

## Relational Algebra

```algebra
-- Before: lateral unnest with existence check
sigma[EXISTS](R join_lateral unnest(R.arr) AS u(val) WHERE u.val = ?)

-- After: semi-join with array containment
R semi_join (R.arr IS NOT NULL AND cardinality(R.arr) > 0)

-- Or with specific value match:
R semi_join (? = ANY(R.arr))
```

### Specific value variant

```algebra
-- Before
sigma[EXISTS(SELECT 1 FROM unnest(R.tags) u WHERE u = 'urgent')](R)

-- After
sigma['urgent' = ANY(R.tags)](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// EXISTS + lateral unnest with equality -> ANY containment
rw!("lateral-unnest-exists-to-any";
    "(filter (exists
       (filter (= ?unnest_col ?val)
         (lateral-join ?input (unnest ?arr))))
      ?outer)" =>
    "(filter (any-eq ?val ?arr) ?outer)"
    if is_unnest_alias("?unnest_col")
),

// Lateral unnest used only for non-empty check
rw!("lateral-unnest-nonempty-to-predicate";
    "(filter (exists (lateral-join ?input (unnest ?arr)))
      ?outer)" =>
    "(filter (and (is-not-null ?arr) (gt (cardinality ?arr) 0))
      ?outer)"
),

// Left lateral unnest where NULL fill indicates semi-join pattern
rw!("left-lateral-unnest-to-semi-join";
    "(filter (is-not-null ?unnest_col)
       (left-lateral-join ?input (unnest ?arr) ?unnest_col))" =>
    "(semi-join (gt (cardinality ?arr) 0) ?input)"
    if is_unnest_alias("?unnest_col")
),
```

## Preconditions

```rust
fn only_checks_existence(outer: &RelExpr) -> bool {
    // The outer query must consume the lateral unnest result
    // only through an existence test. Patterns:
    // 1. EXISTS(SELECT ... FROM unnest(...) WHERE ...)
    // 2. LEFT JOIN unnest(...) ... WHERE unnested_col IS NOT NULL
    // 3. COUNT(*) > 0 on the unnest result
    // 4. ANY/SOME quantified comparison
    match outer {
        RelExpr::Filter { pred, .. } => {
            pred.is_exists_subquery()
                || pred.is_not_null_on_unnest_col()
        }
        RelExpr::Aggregate { aggs, .. } => {
            aggs.iter().all(|a| a.is_count_star_gt_zero())
        }
        _ => false,
    }
}

fn is_unnest_alias(col: &Expr) -> bool {
    // Column must be the alias produced by the unnest operator,
    // not a column from the base relation.
    matches!(col, Expr::Column(c) if c.is_from_unnest())
}
```

**Restrictions:**
- The unnest result must be consumed only for existence testing, not for actual row values.
- If the query selects columns from the unnested rows (e.g., `SELECT u.val`), this rule does not apply.
- Array must not be NULL-producing in a way that changes semantics (LEFT JOIN with NULL array should preserve the outer row).

## Cost Model

```rust
fn estimated_benefit(
    outer_card: f64,
    avg_array_length: f64,
) -> f64 {
    // Before: lateral unnest produces outer_card * avg_array_length
    // rows, then existence filter reduces back to outer_card.
    let lateral_cost = outer_card * avg_array_length;

    // After: array containment check is O(avg_array_length) per
    // outer row but avoids row materialization.
    let semi_join_cost = outer_card * avg_array_length.ln().max(1.0);

    (lateral_cost - semi_join_cost) / lateral_cost
}
```

**Typical benefit**: 50-95%. For large arrays, avoiding row
materialization provides significant memory and CPU savings.
With a GIN index on the array column, the containment check
is O(log N) instead of O(N).

## Test Cases

### Positive: EXISTS with unnest equality

```sql
-- Before
SELECT c.name FROM customers c
WHERE EXISTS (
  SELECT 1 FROM unnest(c.tags) AS u(tag)
  WHERE u.tag = 'premium'
);

-- After
SELECT c.name FROM customers c
WHERE 'premium' = ANY(c.tags);
```

### Positive: non-empty array check via lateral

```sql
-- Before
SELECT o.id FROM orders o
WHERE EXISTS (
  SELECT 1 FROM unnest(o.line_items) AS u(item)
);

-- After
SELECT o.id FROM orders o
WHERE o.line_items IS NOT NULL
  AND cardinality(o.line_items) > 0;
```

### Positive: left join with IS NOT NULL filter

```sql
-- Before
SELECT p.name
FROM products p
LEFT JOIN LATERAL unnest(p.categories) AS u(cat) ON true
WHERE u.cat IS NOT NULL;

-- After (semi-join eliminates left join + filter)
SELECT p.name
FROM products p
WHERE cardinality(p.categories) > 0;
```

### Negative: selecting unnested values

```sql
-- Cannot convert: query uses the actual unnested values
SELECT c.name, u.tag
FROM customers c, LATERAL unnest(c.tags) AS u(tag);

-- u.tag is projected, not just existence-checked.
```

### Negative: aggregation on unnested values

```sql
-- Cannot convert: aggregation uses unnested values
SELECT c.name, COUNT(DISTINCT u.tag)
FROM customers c, LATERAL unnest(c.tags) AS u(tag)
GROUP BY c.name;

-- COUNT(DISTINCT u.tag) requires actual tag values.
```

## References

- Kim, W. "On Optimizing an SQL-like Nested Query" TODS (1982)
- PostgreSQL: convert_ANY_sublink_to_join in optimizer
- Oracle: Unnest hint and semi-join transformation
- Neumann, T. "Unnesting Arbitrary Queries" BTW (2015)
