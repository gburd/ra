# Rule: Push Filter Predicate Into Both Join Sides

**Category:** distributed/data-movement
**File:** `rules/distributed/data-movement/push-predicate-through-join.rra`

## Metadata

- **ID:** `push-predicate-through-join`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, tidb
- **Tags:** distributed, filter, pushdown, join, equivalence, data-movement
- **Authors:** "RA Contributors"


# Push Filter Predicate Into Both Join Sides

## Description

Pushes a filter predicate from a join's ON clause into both the left
and right inputs of the join, using equality-based column equivalences.
When a join has an equality condition (e.g., a.x = b.x) and a filter
on one side's column (e.g., a.x + a.y < 5), the filter can be mapped
to the other side using the equivalence (b.x + b.y < 5) and pushed
to both inputs.

**When to apply**: An inner join or semi join has a filter that is not
bound to either side alone, but can be mapped to both sides using the
equality conditions in the ON clause.

**Why it works**: In distributed execution, filtering before the join
reduces the number of rows that must be shuffled across the network.
By pushing the filter to both sides, each side independently eliminates
non-matching rows before the join materializes any intermediate result.

## Relational Algebra

```algebra
InnerJoin[a.x = b.x AND a.x + a.y < 5](A, B)
  -> InnerJoin[a.x = b.x](
       sigma[a.x + a.y < 5](A),
       sigma[b.x + b.y < 5](B)
     )
  using equivalence: a.x = b.x, a.y = b.y
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("push-filter-into-join-left-and-right";
    "(join inner
        ?left
        ?right
        [(eq_filter ?lk ?rk) ?filter ?rest]
        ?private)" =>
    "(join inner
        (filter [(map_filter ?filter ?left_cols ?equiv)] ?left)
        (filter [(map_filter ?filter ?right_cols ?equiv)] ?right)
        [(eq_filter ?lk ?rk) ?rest]
        ?private)"
    if no_outer_cols("?left")
    if no_outer_cols("?right")
    if can_map_to_both_sides("?filter", "?left", "?right")
),
```

## Preconditions

```rust
fn applicable(
    filter: &FiltersItem,
    left: &RelExpr,
    right: &RelExpr,
    on: &FiltersExpr,
) -> bool {
    // Neither side has outer columns (no apply correlation)
    !left.has_outer_cols() && !right.has_outer_cols()
    // Filter is not a simple equality (those stay in ON)
    && !filter.is_equality()
    // Filter can be mapped to left side columns
    && can_map_filter(filter, &left.output_cols(), &equiv_groups(on))
    // Filter can be mapped to right side columns
    && can_map_filter(filter, &right.output_cols(), &equiv_groups(on))
}
```

**Restrictions:**
- Only applies to InnerJoin and SemiJoin (not outer joins, which
  would change NULL extension semantics)
- Does not apply to InnerJoinApply or SemiJoinApply (correlated)
- The filter must not be a simple column equality (those are part
  of the join condition)
- Column equivalences must be derivable from equality predicates
  in the ON clause
- This rule must run before other filter-push-down rules to avoid
  conflicts

## Cost Model

```rust
fn push_both_sides_benefit(
    left_rows: f64,
    right_rows: f64,
    filter_selectivity: f64,
    network_cost_per_row: f64,
) -> f64 {
    let left_filtered = left_rows * filter_selectivity;
    let right_filtered = right_rows * filter_selectivity;
    let saved_left = (left_rows - left_filtered) * network_cost_per_row;
    let saved_right =
        (right_rows - right_filtered) * network_cost_per_row;
    saved_left + saved_right
}
```

## Test Cases

```sql
-- Positive: filter mapped to both sides
SELECT * FROM a
JOIN b ON a.x = b.x AND a.y = b.y
WHERE a.x + b.y < 5;

-- Maps to:
-- a.x + a.y < 5 (using a.y = b.y equivalence on left)
-- b.x + b.y < 5 (using a.x = b.x equivalence on right)
-- Both sides filter independently before join
```

```sql
-- Positive: range filter with equivalence
SELECT * FROM orders o
JOIN shipments s ON o.id = s.order_id
WHERE o.created_at > '2024-01-01';

-- If o.id = s.order_id implies ordering relationship:
-- Push date filter to orders side directly
-- Cannot map to shipments (no date equivalence)
-- Falls through to PushFilterIntoJoinLeft instead
```

```sql
-- Negative: left outer join
SELECT * FROM a
LEFT JOIN b ON a.x = b.x
WHERE a.x + a.y < 5;
-- Cannot push to right side of left join (changes NULL behavior)
```

## References

CockroachDB: pkg/sql/opt/norm/rules/join.opt:72 - PushFilterIntoJoinLeftAndRight (commit 51e808c)
CockroachDB: pkg/sql/opt/norm/rules/join.opt:131 - MapFilterIntoJoinLeft
TiDB: pkg/planner/core/rule_predicate_push_down.go:31 - PPDSolver (commit e2184a2)
