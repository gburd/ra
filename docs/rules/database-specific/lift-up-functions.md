# Rule: Lift Functions Above Aggregation

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/lift-up-functions.rra`

## Metadata

- **ID:** `clickhouse-lift-up-functions`
- **Version:** 1.0.0
- **Databases:** clickhouse
- **Tags:** database-specific, clickhouse, aggregation, expression, lift
- **Authors:** "RA Contributors"


# Lift Functions Above Aggregation

## Description

Moves deterministic scalar functions from inside aggregation expressions to after the aggregation when possible. This can reduce computation by applying functions to aggregated results rather than raw rows.

**When to apply**: Scalar functions applied to aggregate results.

**Why it works**: Applying functions to N input rows costs O(N). Applying to M aggregate results (where M << N) costs O(M).

**Database version**: ClickHouse v20.6+

## Relational Algebra

```algebra
GroupBy[keys, f(agg(x))](R) -> Project[keys, f(y)](GroupBy[keys, agg(x) as y](R))
  where f is deterministic scalar function
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("clickhouse-lift-up-functions";
    "(group_by ?keys
        [(agg_item (func ?f (agg ?agg_func ?col)) ?priv)])" =>
    "(project
        (group_by ?keys [(agg_item ?agg_func ?col ?priv)])
        (apply_function ?f))"
    if is_database("clickhouse")
    if is_deterministic_scalar("?f")
),
```

**Typical benefit**: 20-40% when N >> M

## References

**Source code:**
- ClickHouse: `src/Processors/QueryPlan/Optimizations/liftUpFunctions.cpp`
  - Commit: 35f2d31186cca2f8c50f7ba4bd93817da490da85
