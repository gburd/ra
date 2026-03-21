# Rule: Calcite JoinToCorrelateRule

**Category:** logical/semantic-rewriting
**File:** `rules/logical/semantic-rewriting/join-to-correlate.rra`

## Metadata

- **ID:** `calcite-join-to-correlate`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql
- **Tags:** logical, calcite, join, correlate, nested-loop, lateral
- **Authors:** "RA Contributors"


# Calcite JoinToCorrelateRule

## Description

Converts a join into a correlated subquery (Correlate operator),
enabling nested-loop execution. For each row from the left input,
the right input is evaluated with the correlation variable bound.

**When to apply**: A join should be executed as a nested loop,
particularly when the right side can use an index lookup correlated
to the left side.

**Why it works**: Correlated execution is optimal when the right
side has an index on the join key and the left side is small. The
cost is O(n * log m) with an index vs O(n * m) without.

**Calcite class**: `org.apache.calcite.rel.rules.JoinToCorrelateRule`

## Relational Algebra

```algebra
-- Before: standard join
R JOIN[R.k = S.k] S

-- After: correlated execution
R CORRELATE (sigma[S.k = $cor0.k](S))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-join-to-correlate";
    "(join inner (= ?lkey ?rkey) ?left ?right)" =>
    "(correlate inner
        ?left
        (filter (= ?rkey (cor-var ?lkey)) ?right))"
    if is_inner_or_left_join("inner")
),
```

## Preconditions

```rust
fn applicable(join: &Join) -> bool {
    matches!(
        join.join_type(),
        JoinType::Inner | JoinType::Left | JoinType::Semi
    )
}
```

**Restrictions:**
- RIGHT and FULL outer joins cannot be directly correlated
- Best when right side has an index on join keys
- Without an index, this is typically slower than hash join

## Cost Model

```rust
fn estimated_benefit(
    left_rows: f64,
    right_rows: f64,
    right_has_index: bool,
) -> f64 {
    if right_has_index {
        let hash_cost = left_rows + right_rows;
        let nested_cost = left_rows * right_rows.log2();
        if hash_cost > 0.0 {
            (hash_cost - nested_cost) / hash_cost
        } else {
            0.0
        }
    } else {
        -0.5 // Typically worse without index
    }
}
```

**Typical benefit**: 0-50% when index is available on correlated side.

## Test Cases

```sql
-- Positive: join to correlated nested loop
SELECT * FROM emp e
JOIN dept d ON e.deptno = d.deptno;
-- With index on dept.deptno: correlated lookup per emp row
```

```sql
-- Negative: RIGHT JOIN
SELECT * FROM emp e
RIGHT JOIN dept d ON e.deptno = d.deptno;
-- Cannot convert RIGHT JOIN to correlate
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/JoinToCorrelateRule.java (commit af6367d)
