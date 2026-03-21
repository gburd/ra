# Rule: Oracle Group By Placement

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/group-by-placement.rra`

## Metadata

- **ID:** `oracle-group-by-placement`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, group-by, placement, eager, aggregation
- **Authors:** "RA Contributors"


# Oracle Group By Placement

## Description

Moves GROUP BY aggregation below joins (eager aggregation) to reduce
the number of rows entering the join.  Oracle's optimizer can push
aggregation to one side of a join when the aggregation is compatible
with the join condition, dramatically reducing the join's build or
probe side.

**When to apply**: An aggregate query joins a detail table with a
dimension table, and the aggregation can be partially computed before
the join.

**Why it works**: Aggregating before the join reduces the number of
rows from potentially millions to the number of groups (often orders
of magnitude smaller).  The smaller intermediate result makes the join
faster and uses less memory for hash tables.

**Database version**: Oracle 11g+

## Relational Algebra

```algebra
-- Before: join then aggregate
gamma[d.name; total=SUM(f.amount)](
    f join[f.dim_id = d.id] d)

-- After: aggregate then join (eager aggregation)
gamma[d.name; total=SUM(partial_total)](
    (gamma[f.dim_id; partial_total=SUM(f.amount)](f))
    join[f.dim_id = d.id] d)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-group-by-placement";
    "(aggregate ?aggs ?groups
        (join ?type ?cond ?detail ?dimension))" =>
    "(aggregate (finalize-aggs ?aggs) ?groups
        (join ?type ?cond
            (aggregate (partial-aggs ?aggs)
                (detail-groups ?groups ?cond) ?detail)
            ?dimension))"
    if is_database("oracle")
    if can_push_agg_below_join("?aggs", "?cond", "?groups")
),
```

## Preconditions

```rust
fn applicable(
    agg_funcs: &[AggregateFunction],
    join_type: JoinType,
    detail_rows: f64,
    detail_groups: f64,
) -> bool {
    join_type == JoinType::Inner
    && agg_funcs.iter().all(|f| f.is_decomposable())
    && detail_groups < detail_rows * 0.5 // significant reduction
}
```

**Restrictions:**
- Only decomposable aggregates (SUM, COUNT, MIN, MAX) can be pushed
- AVG must be decomposed into SUM + COUNT
- Outer joins require careful handling (NULL groups)
- PLACE_GROUP_BY / NO_PLACE_GROUP_BY hints control this

## Cost Model

```rust
fn estimated_benefit(
    detail_rows: f64,
    groups_after_agg: f64,
    join_cost_per_row: f64,
) -> f64 {
    let rows_eliminated = detail_rows - groups_after_agg;
    rows_eliminated * join_cost_per_row
}
```

**Typical benefit**: For 100M detail rows aggregated to 10K groups
before joining with a dimension table, the join processes 10K rows
instead of 100M -- 10000x reduction.

## Test Cases

```sql
-- Positive: aggregate pushable below join
SELECT d.region, SUM(s.amount)
FROM sales s JOIN regions d ON s.region_id = d.id
GROUP BY d.region;
-- SUM pushed below join: aggregate sales by region_id first
```

```sql
-- Negative: aggregate references both sides
SELECT d.region, COUNT(DISTINCT s.product_id || d.name)
FROM sales s JOIN regions d ON s.region_id = d.id
GROUP BY d.region;
-- DISTINCT on cross-table expression cannot be pushed
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Group By Placement"
Oracle: PLACE_GROUP_BY / NO_PLACE_GROUP_BY hints
