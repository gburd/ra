# Rule: Calcite AggregateProjectPullUpConstantsRule

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-project-pull-up-constants.rra`

## Metadata

- **ID:** `calcite-aggregate-project-pull-up-constants`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql, oracle
- **Tags:** logical, calcite, aggregate, constants, pullup, simplification
- **Authors:** "RA Contributors"


# Calcite AggregateProjectPullUpConstantsRule

## Description

Removes constant keys from an aggregate's GROUP BY clause. When a
group-by column is known to be constant (from metadata or inferred
predicates), it can be removed from the grouping set and placed
in a projection above as a literal.

**When to apply**: An aggregate's input has constant columns
(detected via pulled-up predicates) that appear in the GROUP BY.

**Why it works**: Grouping by a constant is a no-op; all rows have
the same value for that column. Removing it reduces the number of
group-by keys, shrinking hash table entries and potentially reducing
the number of groups.

**Calcite class**: `org.apache.calcite.rel.rules.AggregateProjectPullUpConstantsRule`

## Relational Algebra

```algebra
-- Before: constant in GROUP BY
gamma[dept, 'active'; COUNT(*)](
    sigma[status = 'active'](R)
)

-- After: constant removed from GROUP BY, added as literal above
pi[dept, 'active' AS status, cnt](
    gamma[dept; COUNT(*) AS cnt](
        sigma[status = 'active'](R)
    )
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-aggregate-project-pull-up-constants";
    "(aggregate (group ?key ?const_key) ?aggs ?input)" =>
    "(project (add-const ?const_val ?key)
        (aggregate (group ?key) ?aggs ?input))"
    if is_constant_in_input("?const_key", "?input")
),
```

## Preconditions

```rust
fn applicable(agg: &Aggregate) -> bool {
    let mq = agg.metadata_query();
    let preds = mq.pulled_up_predicates(agg.input());
    let constants = preds.constant_columns();
    let group_set = agg.group_set();

    // At least one group-by column is constant
    // But never remove the last column
    let constant_group_keys = group_set
        .intersect(&constants);
    !constant_group_keys.is_empty()
        && constant_group_keys.cardinality() < group_set.cardinality()
}
```

**Restrictions:**
- Never removes the last group-by column (Aggregate([]) returns 1 row)
- Constants are deduced from pulled-up predicates
- A projection above restores the original schema

## Cost Model

```rust
fn estimated_benefit(
    input_rows: f64,
    keys_removed: usize,
    total_keys: usize,
) -> f64 {
    // Fewer group-by keys = smaller hash entries
    let key_reduction = keys_removed as f64 / total_keys as f64;
    input_rows * key_reduction * 0.001
}
```

**Typical benefit**: 5-40% through reduced grouping overhead.

## Test Cases

```sql
-- Positive: constant column in GROUP BY
SELECT status, dept, COUNT(*)
FROM emp
WHERE status = 'ACTIVE'
GROUP BY status, dept;
-- status is constant ('ACTIVE'); remove from GROUP BY
```

```sql
-- Positive: multiple constants
SELECT region, country, city, SUM(sales)
FROM orders
WHERE region = 'NA' AND country = 'US'
GROUP BY region, country, city;
-- region and country are constants; only group by city
```

```sql
-- Negative: no constant columns
SELECT dept, COUNT(*) FROM emp GROUP BY dept;
-- No constants to pull up
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/AggregateProjectPullUpConstantsRule.java (commit af6367d)
