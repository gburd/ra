# ClickHouse Transformation Rules

This directory contains 22 database-specific transformation rules extracted from ClickHouse v26.3.17.

## Source Information
- **Repository**: https://github.com/ClickHouse/ClickHouse
- **Commit**: 35f2d31186cca2f8c50f7ba4bd93817da490da85
- **Date**: 2026-03-17
- **Primary Source**: `src/Processors/QueryPlan/Optimizations/*.cpp`

## Rules by Category

### MergeTree Storage Optimizations (6 rules)
1. **prewhere-pushdown.rra** - Moves filters to PREWHERE for columnar I/O reduction
2. **lazy-materialization.rra** - Defers column reading until after filters
3. **read-in-order.rra** - Eliminates sorting using MergeTree sort key
4. **sparse-index-skip.rra** - Granule skipping with sparse primary index
5. **primary-key-condition-limit.rra** - Combines PK predicates with limits
6. **direct-text-index-read.rra** - Direct reads from full-text indexes

### Projection Optimizations (2 rules)
7. **aggregate-projection.rra** - Uses pre-aggregated materialized views
8. **normal-projection-usage.rra** - Uses column-subset projections

### Join Optimizations (5 rules)
9. **optimize-join-to-semi.rra** - Converts ANY JOIN to semi-join
10. **filter-pushdown-through-join.rra** - Pushes filters below joins
11. **outer-join-to-inner.rra** - Converts outer joins when filters reject NULLs
12. **join-to-in-conversion.rra** - Converts joins to IN subqueries
13. **runtime-filter-join.rra** - Runtime bloom filters for joins

### Query Plan Optimizations (5 rules)
14. **remove-redundant-distinct.rra** - Eliminates unnecessary DISTINCT
15. **remove-redundant-sorting.rra** - Eliminates unnecessary sorting
16. **topk-optimization.rra** - Priority queue for ORDER BY...LIMIT
17. **merge-expressions.rra** - Merges consecutive expression steps
18. **split-filter.rra** - Splits conjunctive filters for flexibility

### Distributed Query Optimizations (1 rule)
19. **limit-pushdown-distributed.rra** - Pushes limits to remote nodes

### Advanced Transformations (3 rules)
20. **lift-up-functions.rra** - Moves functions above aggregations
21. **lift-up-array-join.rra** - Reorders ARRAY JOIN with filters
22. **use-data-parallel-aggregation.rra** - Parallelizes aggregations

## Key Features

### Columnar Storage Optimizations
ClickHouse's columnar MergeTree engine enables unique optimizations:
- PREWHERE for early filtering with minimal I/O
- Lazy materialization of non-essential columns
- Sparse primary index for granule skipping
- Read-in-order from sorted storage

### Projection Support
- Aggregate projections (pre-aggregated data)
- Normal projections (column subsets with different sort orders)
- Automatic projection selection by optimizer

### Query Plan Optimization
- Filter and expression manipulation
- Redundant operation elimination
- Top-K optimization with priority queues
- Data-parallel execution

### Distributed Queries
- Limit pushdown to reduce network traffic
- Runtime filters for distributed joins
- Support for sharded tables

## MergeTree Storage Model

ClickHouse stores data in parts, each sorted by the primary key. Within each part:
- Data is divided into granules (default: 8192 rows)
- Sparse primary index stores one entry per granule
- Columns are stored separately (columnar format)

This enables:
- Granule skipping via sparse index
- Reading only needed columns
- Order-preserving reads from sort key

## References

1. ClickHouse Documentation: https://clickhouse.com/docs/en/guides/developer/optimizing-performance
2. MergeTree Engine: https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/mergetree
3. Projections: https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/mergetree#projections
4. PREWHERE: https://clickhouse.com/docs/en/sql-reference/statements/select/prewhere
