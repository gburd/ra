# Planner Comparison Benchmark Report

**Generated**: 2026-05-11T17:29:42.243238+00:00
**Git Commit**: 8ceff1b4

## Overall Summary

- Total queries: 25
- Parsed successfully: 25 (100.0%)
- Optimized successfully: 25 (100.0%)
- Median plan time: 0.00ms
- P95 plan time: 26.92ms
- Total plan time: 230.21ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| simple | 10 | 10 | 10 | 0.00ms | 0.05ms | 0 | 0 |
| basic_joins | 15 | 15 | 15 | 0.01ms | 201.68ms | 0 | 0 |

## Detailed Query Results

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 0.05 | 0 | 0 | 0 | OK |
| simple_02_simple_aggregate | 0.00 | 0 | 0 | 0 | OK |
| simple_03_group_by | 0.00 | 0 | 0 | 0 | OK |
| simple_04_filter_aggregate | 0.00 | 0 | 0 | 0 | OK |
| simple_05_selective_filter | 0.00 | 0 | 0 | 0 | OK |
| simple_06_order_limit | 0.00 | 0 | 0 | 0 | OK |
| simple_07_distinct_count | 0.00 | 0 | 0 | 0 | OK |
| simple_08_having_clause | 0.00 | 0 | 0 | 0 | OK |
| simple_09_multiple_filters | 0.00 | 0 | 0 | 0 | OK |
| simple_10_offset | 0.00 | 0 | 0 | 0 | OK |

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 26.92 | 0 | 0 | 0 | OK |
| basic_joins_02_left_join | 0.03 | 0 | 0 | 0 | OK |
| basic_joins_03_right_join | 0.01 | 0 | 0 | 0 | OK |
| basic_joins_04_equi_join_filter | 0.00 | 0 | 0 | 0 | OK |
| basic_joins_05_three_table_join | 0.02 | 0 | 0 | 0 | OK |
| basic_joins_06_foreign_key | 0.01 | 0 | 0 | 0 | OK |
| basic_joins_07_multi_predicate_join | 0.00 | 0 | 0 | 0 | OK |
| basic_joins_08_cross_product | 201.68 | 0 | 0 | 0 | OK |
| basic_joins_09_join_aggregate | 0.01 | 0 | 0 | 0 | OK |
| basic_joins_10_self_join | 0.00 | 0 | 0 | 0 | OK |
| basic_joins_11_dimension_table | 0.01 | 0 | 0 | 0 | OK |
| basic_joins_12_join_with_in | 0.00 | 0 | 0 | 0 | OK |
| basic_joins_13_non_equi_join | 1.46 | 0 | 0 | 0 | OK |
| basic_joins_14_join_distinct | 0.00 | 0 | 0 | 0 | OK |
| basic_joins_15_join_computed | 0.00 | 0 | 0 | 0 | OK |

## Feature Coverage

- Parser success rate: 100.0%
- Optimizer success rate: 100.0%

## Failed Queries

No failures.

