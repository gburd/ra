# Rule: Calcite UnionToValuesRule

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/union-to-values.rra`

## Metadata

- **ID:** `calcite-union-to-values`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql
- **Tags:** logical, calcite, union, values, constant-folding
- **Authors:** "RA Contributors"


# Calcite UnionToValuesRule

## Description

Converts a UNION whose inputs are all VALUES clauses into a single
VALUES clause. This eliminates the UNION operator entirely when all
inputs are constant.

**When to apply**: A UNION ALL or UNION DISTINCT has inputs that
are all VALUES (constant tuples).

**Why it works**: UNION of constant values can be computed at plan
time, replacing the entire subtree with a single VALUES node.

**Calcite class**: `org.apache.calcite.rel.rules.UnionToValuesRule`

## Relational Algebra

```algebra
-- Before: UNION of constants
VALUES(3, NULL) UNION ALL VALUES(7369, NULL) UNION ALL VALUES(1, 2)

-- After: single VALUES
VALUES((3, NULL), (7369, NULL), (1, 2))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-union-to-values";
    "(union-all (values ?v1) (values ?v2))" =>
    "(values (concat ?v1 ?v2))"
),

rw!("calcite-union-distinct-to-values";
    "(union-distinct (values ?v1) (values ?v2))" =>
    "(values (distinct-concat ?v1 ?v2))"
),
```

## Preconditions

```rust
fn applicable(union: &Union) -> bool {
    union.inputs().iter().all(|input| input.is_values())
}
```

**Restrictions:**
- All inputs must be VALUES nodes (constant tuples)
- UNION DISTINCT requires deduplication of the merged values
- Large VALUE sets may not benefit from this transformation

## Cost Model

```rust
fn estimated_benefit(num_union_inputs: usize) -> f64 {
    // Eliminates UNION operator entirely
    let ops_saved = num_union_inputs as f64 - 1.0;
    ops_saved / num_union_inputs as f64
}
```

**Typical benefit**: 50-99% by eliminating runtime UNION operations.

## Test Cases

```sql
-- Positive: UNION ALL of VALUES
SELECT * FROM (VALUES (1, 'a'), (2, 'b'))
UNION ALL
SELECT * FROM (VALUES (3, 'c'), (4, 'd'));
-- Merged into single VALUES ((1,'a'),(2,'b'),(3,'c'),(4,'d'))
```

```sql
-- Positive: UNION DISTINCT of VALUES
SELECT * FROM (VALUES (1), (2)) UNION SELECT * FROM (VALUES (2), (3));
-- Merged and deduplicated: VALUES (1), (2), (3)
```

```sql
-- Negative: non-VALUES input
SELECT 1, 'a' UNION ALL SELECT id, name FROM emp;
-- Second input is not VALUES; cannot merge
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/UnionToValuesRule.java (commit af6367d)
