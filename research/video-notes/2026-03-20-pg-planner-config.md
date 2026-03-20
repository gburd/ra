# PostgreSQL Planner Configuration Deep Dive

**Source:** https://www.postgresql.org/docs/current/runtime-config-query.html
**Date:** Reference documentation (current)
**Speaker:** PostgreSQL documentation

## Key Points
- 24+ boolean switches to enable/disable plan node types
- Cost constants calibrate the cost model to hardware
- Several newer features (incremental sort, memoize, self-join elimination)
- Configuration exposes optimization techniques RA could implement

## Notable Optimization Features

### Self-Join Elimination (enable_self_join_elimination)
- Replace self joins with single scans when provably equivalent
- Detects when a table is joined to itself unnecessarily
- New in PostgreSQL 17

### Incremental Sort (enable_incremental_sort)
- When data partially sorted, sort only remaining columns
- Example: data sorted on (a), need (a, b) - only sort within each a group
- Reduces sort cost from O(n log n) to O(n log m) where m = max group size

### Memoize (enable_memoize)
- Cache results of parameterized nested loop inner scans
- When outer side has repeated values, reuse cached inner results
- Reduces redundant inner scan executions

### Partition Pruning (enable_partition_pruning)
- Eliminate partitions at planning AND execution time
- Runtime pruning: use parameter values not known at planning
- Critical for partitioned tables

### Partitionwise Join (enable_partitionwise_join)
- Join matching partitions separately instead of full table join
- Reduces memory usage and enables partition-local optimization
- Off by default (planning overhead)

### Partitionwise Aggregation (enable_partitionwise_aggregate)
- Aggregate per partition, then combine
- Two-phase aggregation across partitions
- Off by default

### Presorted Aggregate (enable_presorted_aggregate)
- Provide presorted input for ORDER BY/DISTINCT aggregates
- Avoids redundant sorting within aggregate functions

### DISTINCT Key Reordering (enable_distinct_reordering)
- Reorder DISTINCT keys to match existing pathkeys
- Avoids unnecessary re-sort for DISTINCT elimination

### GROUP BY Reordering (enable_group_by_reordering)
- Reorder GROUP BY keys to match child node's sort order
- Avoids unnecessary sort when partial ordering available

## Applicable to RA
- Gap: No self-join elimination rules
- Gap: No incremental sort rules (partial sort optimization)
- Gap: No memoize/caching rules for parameterized scans
- Gap: No runtime partition pruning rules
- Gap: No partitionwise join optimization rules
- Gap: No partitionwise aggregation rules
- Gap: No DISTINCT key reordering optimization
- Gap: No GROUP BY key reordering optimization
- Gap: No presorted aggregate optimization

## References
- PostgreSQL documentation: Chapter 19 - Server Configuration
- PostgreSQL release notes for features by version
