# Planner Comparison Benchmark Report

**Generated**: 2026-05-12T01:27:03.835436+00:00
**Git Commit**: d4d71389

## Overall Summary

- Total queries: 25
- Parsed successfully: 25 (100.0%)
- Optimized successfully: 25 (100.0%)
- Median plan time: 1.19ms
- P95 plan time: 1.52ms
- Total plan time: 263.90ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| simple | 10 | 10 | 10 | 1.20ms | 234.86ms | 30 | 2 |
| basic_joins | 15 | 15 | 15 | 1.19ms | 1.52ms | 115 | 2 |

## Detailed Query Results

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 234.86 | 830 | 17 | 2 | OK |
| simple_02_simple_aggregate | 1.49 | 1834 | 30 | 1 | OK |
| simple_03_group_by | 1.32 | 1833 | 28 | 1 | OK |
| simple_04_filter_aggregate | 1.31 | 1936 | 40 | 2 | OK |
| simple_05_selective_filter | 1.20 | 831 | 40 | 2 | OK |
| simple_06_order_limit | 1.12 | 1533 | 21 | 1 | OK |
| simple_07_distinct_count | 1.10 | 1832 | 16 | 1 | OK |
| simple_08_having_clause | 1.18 | 2235 | 30 | 2 | OK |
| simple_09_multiple_filters | 1.15 | 831 | 41 | 2 | OK |
| simple_10_offset | 1.06 | 1533 | 23 | 1 | OK |

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 1.22 | 3862 | 115 | 2 | OK |
| basic_joins_02_left_join | 1.10 | 4262 | 28 | 2 | OK |
| basic_joins_03_right_join | 1.04 | 3861 | 22 | 2 | OK |
| basic_joins_04_equi_join_filter | 1.32 | 3863 | 154 | 2 | OK |
| basic_joins_05_three_table_join | 1.52 | 6292 | 280 | 2 | OK |
| basic_joins_06_foreign_key | 1.19 | 3862 | 116 | 2 | OK |
| basic_joins_07_multi_predicate_join | 1.33 | 3862 | 151 | 2 | OK |
| basic_joins_08_cross_product | 1.09 | 3861 | 42 | 2 | OK |
| basic_joins_09_join_aggregate | 1.31 | 4262 | 119 | 2 | OK |
| basic_joins_10_self_join | 1.16 | 3863 | 99 | 2 | OK |
| basic_joins_11_dimension_table | 1.16 | 4362 | 81 | 2 | OK |
| basic_joins_12_join_with_in | 1.19 | 3862 | 116 | 2 | OK |
| basic_joins_13_non_equi_join | 1.12 | 3864 | 69 | 2 | OK |
| basic_joins_14_join_distinct | 1.12 | 4161 | 70 | 2 | OK |
| basic_joins_15_join_computed | 1.23 | 3863 | 156 | 2 | OK |

## Feature Coverage

- Parser success rate: 100.0%
- Optimizer success rate: 100.0%

## Failed Queries

No failures.

