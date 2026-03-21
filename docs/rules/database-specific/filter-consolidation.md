# Rule: Filter Constraint Consolidation

**Category:** database-specific/cockroachdb
**File:** `rules/database-specific/cockroachdb/filter-consolidation.rra`

## Metadata

- **ID:** `crdb-filter-consolidation`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** distributed, filter, constraint, range, consolidation, optimization
- **Authors:** "RA Contributors"


# Filter Constraint Consolidation

## Description

Consolidates multiple filter predicates on the same variable into a
single Range constraint. When a select has predicates like `x >= 5`
and `x <= 10`, they are combined into the range constraint `[5, 10]`.
This enables better selectivity estimation and more efficient index
constraint generation for distributed scan planning.

**When to apply**: A Select operator has multiple filter conditions
that constrain the same variable and can be combined into a single
range or equality constraint.

**Why it works**: Individual predicates produce separate, potentially
overlapping constraints. The optimizer's selectivity estimator may
over-estimate or under-estimate the combined selectivity because it
treats them independently. A consolidated Range constraint produces a
more accurate selectivity estimate, leading to better join ordering
and scan planning decisions.

## Relational Algebra

```algebra
sigma[x >= 5 AND x <= 10](R)
  -> sigma[x IN [5, 10]](R)
  -- internally: Range(x, 5, 10)

sigma[x > 3 AND x > 7](R)
  -> sigma[x > 7](R)
  -- redundant lower bound eliminated
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("consolidate-select-filters";
    "(filter ?filters (scan ?table))" =>
    "(filter (consolidate_filters ?filters) (scan ?table))"
    if can_consolidate("?filters")
),
```

## Preconditions

```rust
fn applicable(filters: &FiltersExpr) -> bool {
    // Multiple filters constrain the same column
    filters.has_consolidatable_predicates()
    // Filters are on the same variable with compatible operators
    && filters.iter()
        .group_by(|f| f.constrained_column())
        .any(|(_, group)| group.count() > 1)
}
```

**Restrictions:**
- Only works with comparison operators (>, >=, <, <=, =, BETWEEN)
- Cannot consolidate predicates with OR (those remain disjunctive)
- Functions applied to the column prevent consolidation
  (e.g., LOWER(x) >= 'a' and LOWER(x) <= 'z' are separate from
  x >= 'a' and x <= 'z')
- This is a low-priority normalization rule to avoid running before
  other filter simplification rules

## Cost Model

```rust
fn consolidation_benefit(
    original_selectivity_estimate: f64,
    consolidated_selectivity_estimate: f64,
) -> f64 {
    // Better selectivity estimate leads to better plan choices
    (original_selectivity_estimate
        - consolidated_selectivity_estimate).abs()
}
```

## Test Cases

```sql
-- Positive: range consolidation
SELECT * FROM orders
WHERE amount >= 100 AND amount <= 500;

-- Consolidated to: Range(amount, 100, 500)
-- Single index span: [/100 - /500]
```

```sql
-- Positive: redundant bound elimination
SELECT * FROM t WHERE x > 3 AND x >= 5 AND x < 10;
-- Consolidated to: Range(x, 5, 10) with x >= 5 AND x < 10
-- The x > 3 predicate is subsumed by x >= 5
```

```sql
-- Positive: deduplication of identical filters
SELECT * FROM t WHERE x = 5 AND x = 5;
-- Deduplicated to single x = 5
```

```sql
-- Negative: filters on different columns
SELECT * FROM t WHERE x > 5 AND y < 10;
-- Different columns; nothing to consolidate
```

## References

CockroachDB: pkg/sql/opt/norm/rules/select.opt:47 - ConsolidateSelectFilters (commit 51e808c)
CockroachDB: pkg/sql/opt/norm/rules/select.opt:61 - DeduplicateSelectFilters
CockroachDB: pkg/sql/opt/constraint/ - constraint generation from ranges
