# Rule: Commute Left Join to Right Join

**Category:** database-specific/cockroachdb
**File:** `rules/database-specific/cockroachdb/commute-left-to-right-join.rra`

## Metadata

- **ID:** `cockroachdb-commute-left-to-right-join`
- **Version:** 1.0.0
- **Databases:** cockroachdb
- **Tags:** database-specific, cockroachdb, join, commutativity, outer-join
- **Authors:** "RA Contributors"


# Commute Left Join to Right Join

## Description

Creates a Right Join with swapped left and right inputs from a Left Join. This exploration rule allows the optimizer to consider symmetric join orders for outer joins, potentially enabling better join methods or access paths on the swapped inputs.

**When to apply**: Any Left Join can be commuted to Right Join during exploration phase.

**Why it works**: Left and Right joins are semantically equivalent with swapped inputs. Trying both forms allows the optimizer to find better physical plans, such as using an index on the originally-right input when it becomes the left input of a Right Join.

**Database version**: CockroachDB v19.1+

## Relational Algebra

```algebra
LeftJoin[c](R, S) -> RightJoin[c](S, R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("cockroachdb-commute-left-to-right-join";
    "(left_join ?left ?right ?on ?private)" =>
    "(right_join ?right ?left ?on (commute_join_flags ?private))"
    if is_database("cockroachdb")
),
```

## Preconditions

```rust
fn applicable(join: &LeftJoin) -> bool {
    // Always applicable during exploration
    true
}
```

**Restrictions:**
- Only applies to CockroachDB
- Join flags must be commuted appropriately
- Symmetric to the CommuteRightJoin normalization rule

## Cost Model

```rust
fn estimated_benefit(
    left_card: f64,
    right_card: f64,
    has_left_index: bool,
    has_right_index: bool,
) -> f64 {
    // Benefit when swapping enables better index usage
    if !has_left_index && has_right_index {
        return 0.4;
    }
    // Exploration benefit: allows optimizer to consider more plans
    0.1
}
```

**Typical benefit**: 10-50% when enabling better index access after swap

## Test Cases

### Positive Case 1: Index on Right Side

```sql
SELECT * FROM small_table s
LEFT JOIN indexed_table i ON s.key = i.key;

-- Commuted to RightJoin, potentially using index on i
-- RightJoin(indexed_table, small_table) with index on i.key
```

## References

**Source code:**
- CockroachDB: `pkg/sql/opt/xform/rules/join.opt`
  - Rule: `CommuteLeftJoin` (lines 18-23)
  - Git: https://github.com/cockroachdb/cockroach
  - Commit: 6e210ba6aa33cea5e27b1a8fae212c27941781f4 (2026-03-17)
