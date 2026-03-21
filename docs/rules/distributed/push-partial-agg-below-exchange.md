# Rule: Push Partial Aggregation Below Exchange

**Category:** distributed/partial-aggregation
**File:** `rules/distributed/partial-aggregation/push-partial-agg-below-exchange.rra`

## Metadata

- **ID:** `push-partial-agg-below-exchange`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum, citus
- **Tags:** distributed, aggregation, partial, pushdown, reduction
- **Authors:** "RA Contributors"


# Push Partial Aggregation Below Exchange

## Description

When a grouped aggregation sits above an exchange, insert a partial
aggregation below the exchange to reduce the data that crosses the
network. This is the transformation step that enables two-phase
aggregation, focusing specifically on the pushdown mechanics.

**When to apply**: An aggregation sits above an exchange operator and
the aggregation function is decomposable. The reduction ratio (distinct
groups / input rows) should be significant for the pushdown to pay off.

**Why it works**: Partial aggregation compresses many input rows into
one output row per group. Even a 10:1 reduction ratio translates to 90%
less network traffic.

## Relational Algebra

```algebra
gamma[g, agg(a)](Exchange[d](R))
  -> gamma_final[g, merge(partial)](
       Exchange[hash(g)](
         gamma_partial[g, agg(a)](R)
       )
     )
  where agg is decomposable
  where ndv(g) / |R| < reduction_threshold
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("push-partial-agg-below-exchange";
    "(aggregate ?groups ?agg (exchange ?type ?child))" =>
    "(aggregate_final ?groups ?agg
        (exchange hash_partition
            (aggregate_partial ?groups ?agg ?child)
            ?groups))"
    if agg_is_decomposable("?agg")
    if reduction_ratio_sufficient("?groups", "?child")
),
```

## Preconditions

```rust
fn applicable(
    agg: &AggFunction,
    groups: &[Column],
    child: &RelNode,
) -> bool {
    // Aggregation must be decomposable
    agg.is_decomposable()
    // Reduction must be worth the overhead
    && {
        let ndv = groups.iter()
            .map(|g| child.column_ndv(g))
            .product::<f64>();
        let rows = child.estimated_cardinality();
        ndv / rows < 0.5 // at least 2:1 reduction
    }
}
```

**Restrictions:**
- If the number of groups approaches the number of input rows (e.g.,
  GROUP BY primary_key), partial aggregation adds overhead with no
  reduction
- The exchange type changes from the original to hash(group_keys)
- Partial aggregation state must fit in memory on each node
- Some systems (Presto) adaptively disable partial aggregation when the
  reduction ratio is poor at runtime

## Cost Model

```rust
fn should_push_partial(
    input_rows: f64,
    distinct_groups: f64,
    row_bytes: f64,
    num_nodes: u32,
    network_bandwidth: f64,
) -> bool {
    let reduction = distinct_groups / input_rows;
    let shuffle_savings =
        (1.0 - reduction) * input_rows * row_bytes
        * (num_nodes - 1) as f64 / num_nodes as f64;
    let partial_agg_cpu = input_rows * 50e-9; // 50ns per row
    shuffle_savings / network_bandwidth > partial_agg_cpu
}
```

**Typical benefit**: Reduces network shuffle by (1 - NDV/rows) fraction.
For 10M rows with 1K groups, that is 99.99% reduction.

## Test Cases

```sql
-- Positive: high reduction ratio
SELECT department, AVG(salary) FROM employees GROUP BY department;
-- 1M employees, 50 departments -> 50 partial rows per node

-- Plan:
-- AggregateFinal(dept, SUM(p_sum)/SUM(p_cnt))
--   Exchange[hash(department)]
--     AggregatePartial(dept, SUM(salary), COUNT(salary))
--       Scan(employees)
```

```sql
-- Negative: group by primary key, no reduction
SELECT id, COUNT(*) FROM events GROUP BY id;
-- NDV = row count -> partial aggregation adds overhead
-- Keep single-phase aggregation
```

```sql
-- Negative: non-decomposable aggregation
SELECT region, PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY amount)
FROM orders GROUP BY region;
-- PERCENTILE is not decomposable -> cannot push partial
```

## References

Presto/Trino: presto-main/src/main/java/com/facebook/presto/operator/aggregation/partial/PartialAggregation.java
Spark SQL: sql/catalyst/src/main/scala/org/apache/spark/sql/execution/aggregate/HashAggregateExec.scala
CockroachDB: pkg/sql/opt/norm/rules/agg.opt - PushPartialAggThroughExchange
Greenplum: src/backend/optimizer/plan/planagg.c
