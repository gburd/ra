# Rule: Aggregation Push Down Through Join

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/aggregation-push-down-join.rra`

## Metadata

- **ID:** `tidb-aggregation-push-down-join`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** distributed, aggregation, pushdown, join, decomposable, tidb
- **Authors:** "RA Contributors"


# Aggregation Push Down Through Join

## Description

Pushes aggregation functions below a join by decomposing them into
partial aggregations on each side of the join. For a decomposable
aggregation function F, F(S1 UNION ALL S2) = F2(F1(S1), F1(S2)).
The partial aggregations on each join child reduce the number of rows
before the join, which can dramatically reduce join cost.

**When to apply**: An aggregation sits above a join, and the
aggregation functions are decomposable (can be split into
partial/merge phases). The aggregation's input columns must come from
only one side of the join, or the function must support cross-side
decomposition.

**Why it works**: In distributed joins, data from both sides must be
shuffled across the network. Aggregating before the join reduces the
data volume for the shuffle. For star-schema queries, this can reduce
fact table volume by orders of magnitude.

## Relational Algebra

```algebra
gamma[g, SUM(R.a)](Join[cond](L, R))
  -> gamma[g, SUM(partial_sum)](
       Join[cond](L,
         gamma[join_keys, SUM(a) AS partial_sum](R)))
  where a comes entirely from R
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("agg-push-down-through-join";
    "(aggregate ?group_keys ?agg_fns
        (join ?type ?left ?right ?cond))" =>
    "(aggregate_final ?group_keys ?agg_fns_final
        (join ?type
            ?left_with_partial_agg
            ?right_with_partial_agg
            ?cond))"
    if agg_fns_are_decomposable_with_join("?agg_fns")
    if agg_inputs_from_single_side("?agg_fns", "?left", "?right")
),
```

## Preconditions

```rust
fn is_decomposable_with_join(agg: &AggFuncDesc) -> bool {
    // No ORDER BY in aggregate
    agg.order_by_items.is_empty()
    && match agg.name {
        // MAX, MIN, FirstRow: always decomposable
        Max | Min | FirstRow => true,
        // SUM, COUNT: decomposable unless DISTINCT
        Sum | Count => !agg.has_distinct,
        // Not decomposable through joins
        Avg | GroupConcat | VarPop | StddevPop
            | JsonArrayAgg | ApproxPercentile => false,
        _ => false,
    }
}

fn get_agg_child_idx(
    agg: &AggFuncDesc,
    left_schema: &Schema,
    right_schema: &Schema,
) -> AggSide {
    let cols = extract_columns(agg.args);
    let from_left = cols.iter().any(|c| left_schema.contains(c));
    let from_right = cols.iter().any(|c| right_schema.contains(c));
    match (from_left, from_right) {
        (true, true) => Both,   // Cannot decompose
        (true, false) => Left,
        (false, true) => Right,
        (false, false) => Neither, // e.g., COUNT(*), SUM(1)
    }
}
```

**Restrictions:**
- AVG cannot be pushed down through joins (unlike through exchanges
  where it decomposes into SUM/COUNT)
- COUNT(DISTINCT x) is not decomposable through joins
- GROUP_CONCAT and statistical functions (VAR_POP, STDDEV) are not
  decomposable
- If an aggregation references columns from both sides, it cannot
  be pushed down
- COUNT(*) and SUM(1) reference neither side and need special
  handling

## Cost Model

```rust
fn push_down_benefit(
    left_rows: f64,
    right_rows: f64,
    join_result_rows: f64,
    right_groups: f64,
) -> f64 {
    // Without pushdown: join produces join_result_rows, then aggregate
    let without = join_result_rows;
    // With pushdown: aggregate right to right_groups, then join
    let with_pushdown = left_rows + right_groups;
    without - with_pushdown
}
```

## Test Cases

```sql
-- Positive: SUM pushed to right side of join
SELECT d.region, SUM(f.amount)
FROM dim_store d
JOIN fact_sales f ON d.id = f.store_id
GROUP BY d.region;

-- Before: GroupBy(region, SUM(amount))
--           HashJoin(d.id = f.store_id)
--             Scan(dim_store)
--             Scan(fact_sales)  -- 1B rows

-- After: GroupBy(region, SUM(partial_sum))
--           HashJoin(d.id = f.store_id)
--             Scan(dim_store)
--             GroupBy(store_id, SUM(amount) AS partial_sum)
--               Scan(fact_sales)  -- reduces to #stores rows
```

```sql
-- Negative: AVG is not decomposable through join
SELECT d.name, AVG(f.price)
FROM dim d JOIN fact f ON d.id = f.dim_id
GROUP BY d.name;
-- Cannot push AVG below join; must aggregate after join
```

## References

TiDB: pkg/planner/core/rule_aggregation_push_down.go:33 - AggregationPushDownSolver (commit e2184a2)
TiDB: pkg/planner/core/rule_aggregation_push_down.go:44 - isDecomposableWithJoin
TiDB: pkg/planner/core/rule_aggregation_push_down.go:78 - getAggFuncChildIdx
Yan & Larson, "Eager Aggregation and Lazy Aggregation" (VLDB 1995)
