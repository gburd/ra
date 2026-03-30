# Planner Comparison Benchmark Report

**Generated**: 2026-03-30T14:23:32.597471162+00:00
**Git Commit**: 2f521637

## Overall Summary

- Total queries: 25
- Parsed successfully: 25 (100.0%)
- Optimized successfully: 25 (100.0%)
- Median plan time: 1089.19ms
- P95 plan time: 2808.10ms
- Total plan time: 30988.77ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| simple | 10 | 10 | 10 | 3.21ms | 4.52ms | 0 | 0 |
| basic_joins | 15 | 15 | 15 | 2115.00ms | 5246.06ms | 0 | 0 |

## Detailed Query Results

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 4.52 | 0 | 0 | 0 | OK |
| simple_02_simple_aggregate | 2.96 | 0 | 0 | 0 | OK |
| simple_03_group_by | 2.99 | 0 | 0 | 0 | OK |
| simple_04_filter_aggregate | 3.27 | 0 | 0 | 0 | OK |
| simple_05_selective_filter | 3.75 | 0 | 0 | 0 | OK |
| simple_06_order_limit | 3.32 | 0 | 0 | 0 | OK |
| simple_07_distinct_count | 3.15 | 0 | 0 | 0 | OK |
| simple_08_having_clause | 3.21 | 0 | 0 | 0 | OK |
| simple_09_multiple_filters | 3.13 | 0 | 0 | 0 | OK |
| simple_10_offset | 3.07 | 0 | 0 | 0 | OK |

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 2808.10 | 0 | 0 | 0 | OK |
| basic_joins_02_left_join | 3.23 | 0 | 0 | 0 | OK |
| basic_joins_03_right_join | 3.46 | 0 | 0 | 0 | OK |
| basic_joins_04_equi_join_filter | 2115.00 | 0 | 0 | 0 | OK |
| basic_joins_05_three_table_join | 2804.50 | 0 | 0 | 0 | OK |
| basic_joins_06_foreign_key | 5246.06 | 0 | 0 | 0 | OK |
| basic_joins_07_multi_predicate_join | 2030.96 | 0 | 0 | 0 | OK |
| basic_joins_08_cross_product | 1260.38 | 0 | 0 | 0 | OK |
| basic_joins_09_join_aggregate | 2562.32 | 0 | 0 | 0 | OK |
| basic_joins_10_self_join | 2540.73 | 0 | 0 | 0 | OK |
| basic_joins_11_dimension_table | 1094.60 | 0 | 0 | 0 | OK |
| basic_joins_12_join_with_in | 2649.46 | 0 | 0 | 0 | OK |
| basic_joins_13_non_equi_join | 2762.17 | 0 | 0 | 0 | OK |
| basic_joins_14_join_distinct | 1089.19 | 0 | 0 | 0 | OK |
| basic_joins_15_join_computed | 1985.25 | 0 | 0 | 0 | OK |

## Feature Coverage

- Parser success rate: 100.0%
- Optimizer success rate: 100.0%

## Failed Queries

No failures.

