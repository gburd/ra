# Planner Comparison Benchmark Report

**Generated**: 2026-05-11T21:06:33.892735+00:00
**Git Commit**: 1c4e6c53

## Overall Summary

- Total queries: 25
- Parsed successfully: 25 (100.0%)
- Optimized successfully: 25 (100.0%)
- Median plan time: 1.23ms
- P95 plan time: 1.59ms
- Total plan time: 263.24ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| basic_joins | 15 | 15 | 15 | 1.29ms | 234.40ms | 115 | 2 |
| simple | 10 | 10 | 10 | 1.09ms | 1.21ms | 30 | 2 |

## Detailed Query Results

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 234.40 | 3862 | 115 | 2 | OK |
| basic_joins_02_left_join | 1.27 | 4262 | 28 | 2 | OK |
| basic_joins_03_right_join | 1.23 | 3861 | 22 | 2 | OK |
| basic_joins_04_equi_join_filter | 1.38 | 3863 | 154 | 2 | OK |
| basic_joins_05_three_table_join | 1.59 | 6292 | 280 | 2 | OK |
| basic_joins_06_foreign_key | 1.32 | 3862 | 116 | 2 | OK |
| basic_joins_07_multi_predicate_join | 1.33 | 3862 | 151 | 2 | OK |
| basic_joins_08_cross_product | 1.12 | 3861 | 42 | 2 | OK |
| basic_joins_09_join_aggregate | 1.30 | 4262 | 119 | 2 | OK |
| basic_joins_10_self_join | 1.29 | 3863 | 99 | 2 | OK |
| basic_joins_11_dimension_table | 1.24 | 4562 | 81 | 2 | OK |
| basic_joins_12_join_with_in | 1.28 | 3862 | 116 | 2 | OK |
| basic_joins_13_non_equi_join | 1.25 | 3864 | 69 | 2 | OK |
| basic_joins_14_join_distinct | 1.14 | 4161 | 70 | 2 | OK |
| basic_joins_15_join_computed | 1.32 | 3863 | 156 | 2 | OK |

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 1.04 | 830 | 17 | 2 | OK |
| simple_02_simple_aggregate | 1.04 | 1834 | 30 | 1 | OK |
| simple_03_group_by | 1.00 | 1833 | 28 | 1 | OK |
| simple_04_filter_aggregate | 1.13 | 2136 | 40 | 2 | OK |
| simple_05_selective_filter | 1.13 | 831 | 40 | 2 | OK |
| simple_06_order_limit | 1.21 | 1733 | 21 | 1 | OK |
| simple_07_distinct_count | 1.00 | 1832 | 16 | 1 | OK |
| simple_08_having_clause | 1.14 | 2235 | 30 | 2 | OK |
| simple_09_multiple_filters | 1.09 | 831 | 41 | 2 | OK |
| simple_10_offset | 1.00 | 1733 | 23 | 1 | OK |

## Feature Coverage

- Parser success rate: 100.0%
- Optimizer success rate: 100.0%

## Failed Queries

No failures.

