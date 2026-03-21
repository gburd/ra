# Rule: Calcite SortRemoveConstantKeysRule

**Category:** logical/limit-pushdown
**File:** `rules/logical/limit-pushdown/sort-remove-constant-keys.rra`

## Metadata

- **ID:** `calcite-sort-remove-constant-keys`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql, oracle
- **Tags:** logical, calcite, sort, constant, keys, simplification
- **Authors:** "RA Contributors"


# Calcite SortRemoveConstantKeysRule

## Description

Removes sort keys that are known to be constant from an ORDER BY
clause. If all sort keys are constant, the entire Sort operator
is removed. Constant keys are detected using pulled-up predicates.

**When to apply**: A Sort has one or more keys that are known to
be constant based on input predicates.

**Why it works**: Sorting by a constant column produces the same
ordering as not sorting by it. Removing constant sort keys reduces
the sort's comparison cost and may eliminate the sort entirely.

**Calcite class**: `org.apache.calcite.rel.rules.SortRemoveConstantKeysRule`

## Relational Algebra

```algebra
-- Before: sort with constant key
tau[const_col, var_col](sigma[const_col = 5](R))

-- After: constant key removed
tau[var_col](sigma[const_col = 5](R))

-- If all keys are constant:
tau[const_col](sigma[const_col = 5](R))
-- Becomes just: sigma[const_col = 5](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-sort-remove-constant-keys";
    "(sort ?keys (filter (= ?const_key ?val) ?input))" =>
    "(sort ?remaining_keys (filter (= ?const_key ?val) ?input))"
    if key_is_constant("?const_key", "?keys")
),
```

## Preconditions

```rust
fn applicable(sort: &Sort) -> bool {
    let mq = sort.metadata_query();
    let preds = mq.pulled_up_predicates(sort.input());
    if preds.is_empty() { return false; }

    let constants = preds.constant_columns();
    sort.collation().field_collations().iter()
        .any(|fc| constants.contains(&fc.field_index()))
}
```

**Restrictions:**
- Requires pulled-up predicate metadata
- Only removes sort keys, not the sort operator itself (unless all removed)
- OFFSET/LIMIT semantics are preserved

## Cost Model

```rust
fn estimated_benefit(
    input_rows: f64,
    keys_removed: usize,
    total_keys: usize,
) -> f64 {
    // Fewer comparison keys in sort
    let comparison_savings = keys_removed as f64 / total_keys as f64;
    input_rows * input_rows.log2() * comparison_savings * 0.0001
}
```

**Typical benefit**: 5-30% by simplifying sort comparisons.

## Test Cases

```sql
-- Positive: constant sort key removed
SELECT * FROM emp
WHERE deptno = 10
ORDER BY deptno, salary DESC;
-- deptno is constant; ORDER BY simplifies to salary DESC
```

```sql
-- Positive: all keys constant
SELECT * FROM emp
WHERE deptno = 10 AND job = 'CLERK'
ORDER BY deptno, job;
-- Both keys constant; ORDER BY removed entirely
```

```sql
-- Negative: no constant keys
SELECT * FROM emp ORDER BY salary DESC;
-- salary is not constant; no change
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/SortRemoveConstantKeysRule.java (commit af6367d)
