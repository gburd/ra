# Rule: DataFusion Single Distinct Aggregation to Group By

**Category:** database-specific/datafusion
**File:** `rules/database-specific/datafusion/single-distinct-to-groupby.rra`

## Metadata

- **ID:** `datafusion-single-distinct-to-groupby`
- **Version:** "1.0.0"
- **Databases:** datafusion
- **Tags:** database-specific, datafusion, distinct, aggregate, group-by
- **Authors:** "RA Contributors"


# DataFusion Single Distinct Aggregation to Group By

## Description

Converts a single COUNT(DISTINCT col) into a two-phase aggregation
using GROUP BY.  The inner phase deduplicates using GROUP BY, and the
outer phase counts the groups.  This avoids maintaining a per-group
distinct set in memory.

**When to apply**: An aggregate contains exactly one DISTINCT
aggregate function (typically COUNT(DISTINCT ...)).

**Why it works**: Maintaining hash sets for DISTINCT inside an
aggregate operator requires O(distinct_values) memory per group.
Converting to GROUP BY + COUNT uses DataFusion's standard hash
aggregate, which is more memory-efficient and better optimized
for Arrow's columnar format.

**Database version**: DataFusion 25.0+

## Relational Algebra

```algebra
-- Before: distinct aggregate
gamma[g; cnt=COUNT(DISTINCT a)](R)

-- After: two-phase with GROUP BY
gamma[g; cnt=COUNT(a)](
    gamma[g, a](R)
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("datafusion-single-distinct-to-groupby";
    "(aggregate (count-distinct ?col) ?groups ?input)" =>
    "(aggregate (count ?col) ?groups
        (aggregate (list) (extend-groups ?groups ?col) ?input))"
    if is_database("datafusion")
    if single_distinct_aggregate("?col")
),
```

## Preconditions

```rust
fn applicable(aggregates: &[AggregateExpr]) -> bool {
    let distinct_count = aggregates
        .iter()
        .filter(|a| a.is_distinct())
        .count();
    distinct_count == 1
    && aggregates.iter().filter(|a| !a.is_distinct()).all(|a| {
        // Non-distinct aggregates must be compatible
        matches!(a.func(), AggregateFunction::Count | AggregateFunction::Sum)
    })
}
```

**Restrictions:**
- Only applies when there is exactly one DISTINCT aggregate
- Multiple DISTINCT aggregates on different columns cannot use this
- Non-distinct aggregates in the same query must be handled separately

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    distinct_values: f64,
    groups: f64,
) -> f64 {
    // Memory saved: avoid per-group hash sets
    let hashset_memory = groups * distinct_values * 16.0; // bytes
    // CPU saved: GROUP BY dedup is faster than hash set maintenance
    let cpu_saved = rows * 0.0001;
    hashset_memory * 0.001 + cpu_saved
}
```

**Typical benefit**: For high-cardinality DISTINCT on grouped data,
reduces memory usage by 2-5x by leveraging hash aggregate instead of
per-group hash sets.

## Test Cases

```sql
-- Positive: COUNT(DISTINCT) converted to GROUP BY + COUNT
SELECT department, COUNT(DISTINCT employee_id) FROM payroll
GROUP BY department;
-- Inner: GROUP BY department, employee_id
-- Outer: GROUP BY department, COUNT(employee_id)
```

```sql
-- Negative: multiple DISTINCT aggregates
SELECT COUNT(DISTINCT a), COUNT(DISTINCT b) FROM t;
-- Cannot convert: two different DISTINCT columns
```

## References

DataFusion: datafusion/optimizer/src/single_distinct_to_groupby.rs
