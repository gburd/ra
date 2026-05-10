# Planner Comparison Benchmark Report

**Generated**: 2026-05-09T21:50:16.603503+00:00
**Git Commit**: 3b008181

## Overall Summary

- Total queries: 25
- Parsed successfully: 25 (100.0%)
- Optimized successfully: 25 (100.0%)
- Median plan time: 221.35ms
- P95 plan time: 1991.54ms
- Total plan time: 12832.11ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| basic_joins | 15 | 15 | 15 | 926.39ms | 2227.34ms | 0 | 0 |
| simple | 10 | 10 | 10 | 1.75ms | 2.00ms | 0 | 0 |

## Detailed Query Results

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 1233.04 | 0 | 0 | 0 | OK |
| basic_joins_02_left_join | 1.80 | 0 | 0 | 0 | OK |
| basic_joins_03_right_join | 1.73 | 0 | 0 | 0 | OK |
| basic_joins_04_equi_join_filter | 926.39 | 0 | 0 | 0 | OK |
| basic_joins_05_three_table_join | 221.35 | 0 | 0 | 0 | OK |
| basic_joins_06_foreign_key | 1991.54 | 0 | 0 | 0 | OK |
| basic_joins_07_multi_predicate_join | 945.50 | 0 | 0 | 0 | OK |
| basic_joins_08_cross_product | 590.22 | 0 | 0 | 0 | OK |
| basic_joins_09_join_aggregate | 974.98 | 0 | 0 | 0 | OK |
| basic_joins_10_self_join | 662.14 | 0 | 0 | 0 | OK |
| basic_joins_11_dimension_table | 532.86 | 0 | 0 | 0 | OK |
| basic_joins_12_join_with_in | 1035.82 | 0 | 0 | 0 | OK |
| basic_joins_13_non_equi_join | 2227.34 | 0 | 0 | 0 | OK |
| basic_joins_14_join_distinct | 536.57 | 0 | 0 | 0 | OK |
| basic_joins_15_join_computed | 933.65 | 0 | 0 | 0 | OK |

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 1.76 | 0 | 0 | 0 | OK |
| simple_02_simple_aggregate | 1.76 | 0 | 0 | 0 | OK |
| simple_03_group_by | 1.75 | 0 | 0 | 0 | OK |
| simple_04_filter_aggregate | 1.83 | 0 | 0 | 0 | OK |
| simple_05_selective_filter | 2.00 | 0 | 0 | 0 | OK |
| simple_06_order_limit | 1.56 | 0 | 0 | 0 | OK |
| simple_07_distinct_count | 1.58 | 0 | 0 | 0 | OK |
| simple_08_having_clause | 1.67 | 0 | 0 | 0 | OK |
| simple_09_multiple_filters | 1.74 | 0 | 0 | 0 | OK |
| simple_10_offset | 1.54 | 0 | 0 | 0 | OK |

## Feature Coverage

- Parser success rate: 100.0%
- Optimizer success rate: 100.0%

## Failed Queries

No failures.

