# Rule: TiDB Decomposable Aggregation Pushdown

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/aggregation-pushdown-decomposable.rra`

## Metadata

- **ID:** `tidb-aggregation-pushdown-decomposable`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** aggregation, pushdown, distributed, decomposable
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Decomposable Aggregation Pushdown

## Description

Pushes decomposable aggregate functions (MAX, MIN, SUM, COUNT) below joins
by splitting them into partial aggregations on each side. An aggregation
function F is decomposable if F(S1 $\cup$ S2) = F2(F1(S1), F1(S2)), allowing
distributed computation.

**When to apply**: Aggregation over join results where aggregate functions
are decomposable and arguments come from one side of the join.

**Why it works**: By computing partial aggregations before the join, TiDB
reduces the amount of data transferred across TiKV nodes. For example,
SUM(a) over (R $\bowtie$ S) becomes SUM(partial_sum) where partial sums are computed
on each TiKV node before joining.

## Relational Algebra

```algebra
Agg[F(col)](R join[pred] S)
  -> Agg[F2(partial)](
       Agg[F1(col) as partial](R) join[pred] S
     )
  where is_decomposable(F)
    AND col from R schema

Examples:
- MAX(S1 $\cup$ S2) = MAX(MAX(S1), MAX(S2))
- SUM(S1 $\cup$ S2) = SUM(SUM(S1), SUM(S2))
- COUNT(S1 $\cup$ S2) = SUM(COUNT(S1), COUNT(S2))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("tidb-agg-pushdown-decomposable";
    "(aggregate ?aggs
       (join ?pred ?left ?right))" =>
    "(aggregate (final_aggs ?aggs)
       (join ?pred
         (aggregate (partial_aggs_left ?aggs) ?left)
         (aggregate (partial_aggs_right ?aggs) ?right)))"
    if all_decomposable("?aggs")
    if can_split_by_schema("?aggs", "?left", "?right")
),

// TiDB decomposability check (from rule_aggregation_push_down.go:44-59)
fn is_decomposable_with_join(agg_func: &AggFunction) -> bool {
    if !agg_func.order_by.is_empty() {
        return false;
    }

    match agg_func.name {
        // Decomposable without restrictions
        AggFunc::Max | AggFunc::Min | AggFunc::FirstRow => true,

        // Decomposable only without DISTINCT
        AggFunc::Sum | AggFunc::Count => !agg_func.has_distinct,

        // Not decomposable
        AggFunc::Avg
        | AggFunc::GroupConcat
        | AggFunc::VarPop
        | AggFunc::StddevPop
        | AggFunc::JsonArrayAgg
        | AggFunc::JsonObjectAgg
        | AggFunc::ApproxPercentile => false,

        _ => false,
    }
}

// Split aggregates by schema (from rule_aggregation_push_down.go:79-98)
fn get_agg_func_child_idx(
    agg_func: &AggFunction,
    left_schema: &Schema,
    right_schema: &Schema,
) -> ChildIdx {
    let mut from_left = false;
    let mut from_right = false;

    for col in agg_func.extract_columns() {
        if left_schema.contains(col) {
            from_left = true;
        }
        if right_schema.contains(col) {
            from_right = true;
        }
    }

    match (from_left, from_right) {
        (true, true) => ChildIdx::Both,      // Cannot push down
        (true, false) => ChildIdx::Left,     // Push to left
        (false, true) => ChildIdx::Right,    // Push to right
        (false, false) => ChildIdx::Neither, // COUNT(*), SUM(1)
    }
}

enum ChildIdx {
    Left = 0,
    Right = 1,
    Both = -1,
    Neither = 2,
}
```

**Restrictions:**
- Aggregate function must be decomposable (MAX, MIN, SUM, COUNT without DISTINCT)
- No ORDER BY clauses in aggregate (would require global ordering)
- Aggregate arguments must reference only one side of join
- DISTINCT aggregates not supported (require global deduplication)

## Cost Model

```rust
fn estimated_benefit(
    join_card: f64,
    left_card: f64,
    right_card: f64,
    agg_selectivity: f64,
) -> f64 {
    // Without pushdown: join then aggregate
    let join_cost = left_card * right_card;
    let agg_after_join_cost = join_card * 10.0; // Aggregation cost
    let total_without = join_cost + agg_after_join_cost;

    // With pushdown: partial agg, then join smaller results
    let partial_agg_left = left_card * 10.0;
    let partial_agg_right = right_card * 10.0;
    let reduced_left = left_card * agg_selectivity; // Fewer rows after agg
    let reduced_right = right_card * agg_selectivity;
    let reduced_join_cost = reduced_left * reduced_right;
    let final_agg_cost = reduced_join_cost * 5.0;
    let total_with =
        partial_agg_left + partial_agg_right + reduced_join_cost + final_agg_cost;

    if total_without > total_with {
        (total_without - total_with) / total_without
    } else {
        0.0
    }
}
```

**Assumptions:**
- Partial aggregations reduce cardinality significantly (high agg_selectivity)
- Network transfer cost dominates in distributed setting
- TiKV coprocessor can execute partial aggregations efficiently
- Final aggregation over reduced data is cheaper than aggregating join result

**Typical benefit**: 40-90% for aggregations over large joins where partial
aggregation reduces data transfer significantly.

## Test Cases

### Positive: SUM decomposable across join

```sql
-- Original: Join then aggregate
SELECT customer.region, SUM(orders.total_amount)
FROM orders
JOIN customer ON orders.customer_id = customer.id
GROUP BY customer.region;

-- TiDB pushdown: Partial aggregations before join
-- Pseudo-plan:
-- Aggregate[region, SUM(partial_sum)]
--   Join[customer_id]
--     Aggregate[customer_id, SUM(total_amount) as partial_sum](orders)
--     Scan(customer)

-- Benefit: Reduce orders cardinality before join (e.g., 1M -> 10K customers)
```

### Positive: COUNT without DISTINCT

```sql
SELECT product_id, COUNT(*)
FROM order_items
JOIN products ON order_items.product_id = products.id
WHERE products.category = 'electronics'
GROUP BY product_id;

-- Pushdown: COUNT(*) per product before join
-- Reduces network transfer in distributed TiDB cluster
```

### Negative: COUNT(DISTINCT) not decomposable

```sql
SELECT region, COUNT(DISTINCT customer_id)
FROM orders
JOIN regions ON orders.region_id = regions.id
GROUP BY region;

-- Cannot push down: COUNT(DISTINCT) requires global deduplication
-- TiDB keeps aggregation after join
```

### Negative: AVG not supported (yet)

```sql
SELECT department, AVG(salary)
FROM employees
JOIN departments ON employees.dept_id = departments.id
GROUP BY department;

-- AVG is not decomposable in TiDB pushdown rule
-- (Could be split as SUM/COUNT but not implemented)
```

## References

**Source code:**
- File: `pkg/planner/core/rule_aggregation_push_down.go`
- Function: `isDecomposableWithJoin()` (lines 44-59)
- Function: `getAggFuncChildIdx()` (lines 79-98)
- Repository: https://github.com/pingcap/tidb

**Documentation:**
- TiDB Operator Pushdown: https://docs.pingcap.com/tidb/stable/tidb-operator-pushdown
- TiDB Query Execution Plan: https://docs.pingcap.com/tidb/stable/query-execution-plan

**Related concepts:**
- Decomposable aggregation theory (distributed databases)
- TiKV coprocessor framework for pushdown execution
- Two-phase aggregation in distributed query processing
