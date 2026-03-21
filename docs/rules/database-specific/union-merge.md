# Rule: Calcite UnionMergeRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/union-merge.rra`

## Metadata

- **ID:** `calcite-union-merge`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, union, merge, flatten
- **Authors:** "RA Contributors"


# Calcite UnionMergeRule

## Description

Flattens nested UNION ALL operations into a single multi-input
union. A UNION ALL of a UNION ALL becomes a single UNION ALL
with three or more inputs, eliminating intermediate nodes.

**When to apply**: A `LogicalUnion` has another `LogicalUnion`
as a child with the same ALL/DISTINCT setting.

**Why it works**: Reduces plan tree depth and enables more
efficient multi-input union execution.

**Calcite class**: `org.apache.calcite.rel.rules.UnionMergeRule`

## Relational Algebra

```algebra
-- Before: nested unions
(A UNION ALL B) UNION ALL C

-- After: flattened union
A UNION ALL B UNION ALL C
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-union-merge";
    "(union-all (union-all ?a ?b) ?c)" =>
    "(union-all-3 ?a ?b ?c)"
),
```

## Preconditions

```rust
fn applicable(
    outer_all: bool,
    inner_all: bool,
) -> bool {
    outer_all == inner_all
}
```

**Restrictions:**
- UNION (distinct) and UNION ALL cannot be merged together
- Both unions must have the same ALL/DISTINCT modifier

## Cost Model

```rust
fn estimated_benefit(total_rows: f64) -> f64 {
    total_rows * 0.001
}
```

**Typical benefit**: 5-20% from eliminating intermediate buffering.

## Test Cases

```sql
-- Positive: nested UNION ALL flattened
SELECT * FROM a UNION ALL SELECT * FROM b
UNION ALL SELECT * FROM c;
-- Becomes: single 3-input UNION ALL
```

```sql
-- Negative: mixed UNION and UNION ALL
SELECT * FROM a UNION SELECT * FROM b
UNION ALL SELECT * FROM c;
-- Cannot flatten: inner is DISTINCT, outer is ALL
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/UnionMergeRule.java
