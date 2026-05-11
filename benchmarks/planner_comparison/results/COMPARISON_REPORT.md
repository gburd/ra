# Planner Comparison Benchmark Report

**Generated**: 2026-05-11T00:30:52.059229+00:00
**Git Commit**: 8947e9b2

## Overall Summary

- Total queries: 25
- Parsed successfully: 25 (100.0%)
- Optimized successfully: 25 (100.0%)
- Median plan time: 228.68ms
- P95 plan time: 1807.94ms
- Total plan time: 15811.97ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| simple | 10 | 10 | 10 | 2.32ms | 228.14ms | 0 | 0 |
| basic_joins | 15 | 15 | 15 | 918.79ms | 1912.18ms | 0 | 0 |

## Detailed Query Results

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 228.14 | 0 | 0 | 0 | OK |
| simple_02_simple_aggregate | 2.36 | 0 | 0 | 0 | OK |
| simple_03_group_by | 2.17 | 0 | 0 | 0 | OK |
| simple_04_filter_aggregate | 2.37 | 0 | 0 | 0 | OK |
| simple_05_selective_filter | 2.47 | 0 | 0 | 0 | OK |
| simple_06_order_limit | 2.09 | 0 | 0 | 0 | OK |
| simple_07_distinct_count | 2.00 | 0 | 0 | 0 | OK |
| simple_08_having_clause | 2.12 | 0 | 0 | 0 | OK |
| simple_09_multiple_filters | 2.32 | 0 | 0 | 0 | OK |
| simple_10_offset | 2.12 | 0 | 0 | 0 | OK |

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 1508.63 | 0 | 0 | 0 | OK |
| basic_joins_02_left_join | 2.18 | 0 | 0 | 0 | OK |
| basic_joins_03_right_join | 2.05 | 0 | 0 | 0 | OK |
| basic_joins_04_equi_join_filter | 1807.94 | 0 | 0 | 0 | OK |
| basic_joins_05_three_table_join | 228.68 | 0 | 0 | 0 | OK |
| basic_joins_06_foreign_key | 918.79 | 0 | 0 | 0 | OK |
| basic_joins_07_multi_predicate_join | 1713.69 | 0 | 0 | 0 | OK |
| basic_joins_08_cross_product | 615.78 | 0 | 0 | 0 | OK |
| basic_joins_09_join_aggregate | 1583.81 | 0 | 0 | 0 | OK |
| basic_joins_10_self_join | 796.63 | 0 | 0 | 0 | OK |
| basic_joins_11_dimension_table | 557.03 | 0 | 0 | 0 | OK |
| basic_joins_12_join_with_in | 1560.63 | 0 | 0 | 0 | OK |
| basic_joins_13_non_equi_join | 1912.18 | 0 | 0 | 0 | OK |
| basic_joins_14_join_distinct | 561.84 | 0 | 0 | 0 | OK |
| basic_joins_15_join_computed | 1793.94 | 0 | 0 | 0 | OK |

## Feature Coverage

- Parser success rate: 100.0%
- Optimizer success rate: 100.0%

## Failed Queries

No failures.

