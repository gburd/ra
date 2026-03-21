# Rule: Calcite UnnestDecorrelateRule

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/unnest-decorrelate.rra`

## Metadata

- **ID:** `calcite-unnest-decorrelate`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql
- **Tags:** logical, calcite, unnest, decorrelate, array, lateral
- **Authors:** "RA Contributors"


# Calcite UnnestDecorrelateRule

## Description

Converts a correlated Unnest plan (using LogicalCorrelate with
Uncollect) into a simpler non-correlated Unnest representation.
This removes the correlation variable, enabling standard join
optimization techniques.

**When to apply**: A projected Unnest uses a LogicalCorrelate to
reference an array column from the outer query via a correlation
variable.

**Why it works**: The correlated pattern is an artifact of SQL-to-rel
translation. The array column can be directly projected from the
left subquery and unnested without correlation.

**Calcite class**: `org.apache.calcite.rel.rules.UnnestDecorrelateRule`

## Relational Algebra

```algebra
-- Before: correlated unnest
pi[right_cols](
    LeftQuery CORRELATE (
        pi[inner_cols](
            Uncollect(pi[$cor0.array_col](VALUES(0)))
        )
    )
)

-- After: non-correlated unnest
pi[cols](
    Uncollect(pi[array_col](LeftQuery))
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-unnest-decorrelate";
    "(project ?outer_cols
        (correlate inner ?left
            (project ?inner_cols
                (uncollect
                    (project (cor-ref ?array_col)
                        (values single-row))))))" =>
    "(project ?result_cols
        (uncollect
            (project ?array_col ?left)))"
),
```

## Preconditions

```rust
fn applicable(
    correlate: &Correlate,
) -> bool {
    // Right side must be Uncollect referencing a correlation variable
    let right = correlate.right();
    right.contains_uncollect()
        && right.correlation_refs().len() == 1
}
```

**Restrictions:**
- Only applies to the specific correlated Unnest pattern
- The Uncollect must reference exactly one correlation variable
- Inner and outer projections must be reconciled

## Cost Model

```rust
fn estimated_benefit(
    left_rows: f64,
    avg_array_size: f64,
) -> f64 {
    // Removes per-row correlation overhead
    left_rows * 0.001
}
```

**Typical benefit**: 10-50% by removing correlation overhead.

## Test Cases

```sql
-- Positive: UNNEST of correlated array
SELECT e.name, u.skill
FROM employees e,
     UNNEST(e.skills) AS u(skill);
-- Decorrelate to non-correlated unnest
```

```sql
-- Positive: LATERAL UNNEST
SELECT d.name, t.tag
FROM documents d,
     LATERAL UNNEST(d.tags) AS t(tag)
WHERE t.tag LIKE 'important%';
-- Decorrelate and push filter
```

```sql
-- Negative: non-correlated UNNEST
SELECT * FROM UNNEST(ARRAY[1, 2, 3]) AS t(x);
-- Already non-correlated; rule does not apply
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/UnnestDecorrelateRule.java (commit af6367d)
