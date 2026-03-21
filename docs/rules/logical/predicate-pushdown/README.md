# Predicate Pushdown Rules

Total rules: 27

## Overview

Predicate pushdown is a fundamental optimization technique that moves filter conditions closer to the data source. By applying filters early in the query plan, we reduce the amount of data flowing through subsequent operators, improving both CPU and I/O efficiency.

## Key Concepts

### Benefits
- **Reduced I/O**: Filters at scan level can skip reading unnecessary data
- **Smaller intermediate results**: Less memory usage and faster processing
- **Index utilization**: Pushed predicates can leverage indexes
- **Partition pruning**: Filters can eliminate entire partitions

### Pushdown Targets
1. **Through Joins**: Move filters from above joins to individual join inputs
2. **Through Aggregates**: Push filters that don't depend on aggregate results
3. **Through Unions**: Distribute filters to all union branches
4. **Into Subqueries**: Push outer query filters into subqueries
5. **To Storage**: Push filters to storage engines and file formats

## Rules in this Category

- ["Expand Disjunction For Join"](./expand-disjunction-for-join.md) - `expand-disjunction-for-join`
- ["FilterAggregateTranspose"](./filter-aggregate-transpose.md) - `filter-aggregate-transpose`
- [Calcite FilterCorrelateRule](./filter-correlate.md) - `calcite-filter-correlate`
- ["Filter Transpose Aggregate"](./filter-into-aggregate.md) - `filter-into-aggregate`
- [Filter Absorption Into Join Condition](./filter-into-join-condition.md) - `filter-into-join-condition`
- ["Filter Into Join"](./filter-into-join.md) - `filter-into-join`
- [Calcite FilterJoinRule](./filter-join-push.md) - `calcite-filter-join`
- ["FilterJoin"](./filter-join.md) - `filter-join`
- [Filter Merge (Cascading Selections)](./filter-merge.md) - `filter-merge`
- ["FilterProjectTranspose"](./filter-project-transpose.md) - `filter-project-transpose`
- [Calcite FilterSortTransposeRule](./filter-sort-transpose.md) - `calcite-filter-sort-transpose`
- [Calcite FilterTableFunctionTransposeRule](./filter-table-function-transpose.md) - `calcite-filter-table-function-transpose`
- [Calcite FilterTableScanRule](./filter-table-scan.md) - `calcite-filter-table-scan`
- [Filter Pushdown Through Join](./filter-through-join.md) - `filter-through-join`
- [Filter Pushdown Through Projection](./filter-through-project.md) - `filter-through-project`
- [Filter Pushdown Through Union](./filter-through-union.md) - `filter-through-union`
- ["Function Push-Down"](./function-push-down.md) - `function-push-down`
- ["JoinDeriveIsNotNullFilter"](./join-derive-is-not-null-filter.md) - `join-derive-is-not-null-filter`
- [Calcite JoinExtractFilterRule](./join-extract-filter.md) - `calcite-join-extract-filter`
- [Calcite JoinPushExpressionsRule](./join-push-expressions.md) - `calcite-join-push-expressions`
- [Calcite JoinPushTransitivePredicatesRule](./join-push-transitive-predicates.md) - `calcite-join-push-transitive-predicates`
- ["Partition Pushdown"](./partition-pushdown.md) - `partition-pushdown`
- ["Predicate Transitive Closure"](./predicate-transitive-closure.md) - `predicate-transitive-closure`
- ["Starburst Constraint-Based Predicate Propagation"](./starburst-constraint-propagation.md) - `starburst-constraint-propagation`
- ["Starburst Referential Integrity Rewrite"](./starburst-referential-integrity-rewrite.md) - `starburst-referential-integrity-rewrite`
- [Starburst Semantic Query Optimization](./starburst-semantic-optimization.md) - `starburst-semantic-optimization`
- ["Storage Push-Down Aware"](./storage-push-down-aware.md) - `storage-push-down-aware`

## Common Patterns

### Filter Through Join
```sql
-- Before
SELECT * FROM (
  SELECT * FROM orders o
  JOIN customers c ON o.customer_id = c.id
) WHERE o.status = 'SHIPPED'

-- After (filter pushed to orders)
SELECT * FROM (
  SELECT * FROM orders o WHERE o.status = 'SHIPPED'
  JOIN customers c ON o.customer_id = c.id
)
```

### Filter Into Join Condition
```sql
-- Before
SELECT * FROM orders o
JOIN products p ON o.product_id = p.id
WHERE o.region = p.region

-- After (absorbed into join)
SELECT * FROM orders o
JOIN products p ON o.product_id = p.id
  AND o.region = p.region
```

### Filter Through Aggregate
```sql
-- Before
SELECT customer_id, SUM(amount)
FROM (
  SELECT * FROM orders
  GROUP BY customer_id
) WHERE customer_id > 1000

-- After (pushed below aggregate)
SELECT customer_id, SUM(amount)
FROM orders
WHERE customer_id > 1000
GROUP BY customer_id
```

## Implementation Considerations

1. **Null Handling**: Be careful with three-valued logic when pushing predicates through outer joins
2. **Function Determinism**: Only push deterministic functions
3. **Cost Estimation**: Consider selectivity when deciding pushdown order
4. **Semantic Preservation**: Ensure transformations maintain query semantics

## Related Optimizations

- [Partition Pruning](../partition-pruning/)
- [Index Selection](../../physical/index-selection/)
- [Join Reordering](../join-reordering/)
- [Subquery Unnesting](../subquery-unnesting/)
