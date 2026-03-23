# Rule: Split Conjunctive Filter Predicates

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/split-filter.rra`

## Metadata

- **ID:** `clickhouse-split-filter`
- **Version:** 1.0.0
- **Databases:** clickhouse
- **Tags:** database-specific, clickhouse, filter, split, and
- **Authors:** "RA Contributors"


# Split Conjunctive Filter Predicates

## Description

Splits a single Filter with conjunctive (AND) predicates into multiple consecutive filters. This enables individual filters to be pushed down or reordered independently.

**When to apply**: Filter with multiple AND-ed predicates.

**Why it works**: Separate filters can be pushed to different places in the plan or reordered by selectivity. Enables more flexible optimization.

**Database version**: ClickHouse v19.16+

## Relational Algebra

```algebra
Filter[c1 $\land$ c2](R) -> Filter[c2](Filter[c1](R))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("clickhouse-split-filter";
    "(filter (and ?c1 ?c2) ?input)" =>
    "(filter ?c2 (filter ?c1 ?input))"
    if is_database("clickhouse")
),
```

**Typical benefit**: 10-30% (enables other optimizations)

## References

**Source code:**
- ClickHouse: `src/Processors/QueryPlan/Optimizations/splitFilter.cpp`
  - Commit: 35f2d31186cca2f8c50f7ba4bd93817da490da85
