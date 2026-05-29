# Planner Comparison Benchmark Report

**Generated**: 2026-05-29T20:25:21.242674+00:00
**Git Commit**: 632162be

## Overall Summary

- Total queries: 120
- Parsed successfully: 116 (96.7%)
- Optimized successfully: 116 (100.0%)
- Median plan time: 1.88ms
- P95 plan time: 56.18ms
- Total plan time: 852.51ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| set_operations | 11 | 11 | 11 | 2.10ms | 251.19ms | 49 | 2 |
| subqueries | 20 | 20 | 20 | 2.03ms | 52.56ms | 59 | 2 |
| advanced | 9 | 9 | 9 | 1.90ms | 2.15ms | 48 | 2 |
| complex_joins | 20 | 20 | 20 | 2.07ms | 72.50ms | 203 | 2 |
| aggregations | 15 | 15 | 15 | 1.87ms | 19.04ms | 50 | 2 |
| unsupported | 8 | 4 | 4 | 1.80ms | 2.04ms | 34 | 2 |
| ctes | 12 | 12 | 12 | 1.33ms | 2.10ms | 146 | 2 |
| basic_joins | 15 | 15 | 15 | 1.85ms | 1.95ms | 100 | 2 |
| simple | 10 | 10 | 10 | 1.70ms | 1.80ms | 30 | 2 |

## Detailed Query Results

### set_operations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| set_operations_set_operations_01_union | 251.19 | 1710 | 44 | 2 | OK |
| set_operations_set_operations_02_union_all | 2.45 | 1712 | 66 | 2 | OK |
| set_operations_set_operations_03_intersect | 2.32 | 2912 | 17 | 2 | OK |
| set_operations_set_operations_04_intersect_all | 2.15 | 1709 | 36 | 2 | OK |
| set_operations_set_operations_05_except | 2.10 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_06_except_all | 2.02 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_07_nested | 1.32 | 2589 | 54 | 2 | OK |
| set_operations_set_operations_08_order_limit | 2.19 | 1812 | 52 | 2 | OK |
| set_operations_set_operations_09_union_join | 1.64 | 7775 | 214 | 2 | OK |
| set_operations_set_operations_10_three_way_union | 1.33 | 3794 | 49 | 2 | OK |
| set_operations_set_operations_11_except_with_join | 2.00 | 10803 | 277 | 2 | OK |

### subqueries

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| subqueries_subqueries_01_scalar_select | 2.25 | 4268 | 43 | 2 | OK |
| subqueries_subqueries_02_scalar_where | 2.17 | 4264 | 59 | 2 | OK |
| subqueries_subqueries_03_exists | 2.13 | 3862 | 41 | 2 | OK |
| subqueries_subqueries_04_not_exists | 1.67 | 6292 | 112 | 2 | OK |
| subqueries_subqueries_05_in_simple | 2.18 | 3867 | 33 | 2 | OK |
| subqueries_subqueries_06_not_in | 2.02 | 3864 | 23 | 2 | OK |
| subqueries_subqueries_07_derived_table | 1.99 | 6795 | 222 | 2 | OK |
| subqueries_subqueries_08_multi_derived | 2.43 | 7605 | 178 | 2 | OK |
| subqueries_subqueries_09_correlated_agg | 2.13 | 4264 | 59 | 2 | OK |
| subqueries_subqueries_10_nested_in | 1.31 | 8730 | 58 | 2 | OK |
| subqueries_subqueries_11_gt_all | 1.83 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_12_gt_any | 1.85 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_13_lateral | 2.03 | 3971 | 54 | 2 | OK |
| subqueries_subqueries_14_lateral_agg | 2.14 | 4271 | 61 | 2 | OK |
| subqueries_subqueries_15_exists_multi | 1.98 | 3863 | 76 | 2 | OK |
| subqueries_subqueries_16_scalar_multi | 2.08 | 9935 | 67 | 2 | OK |
| subqueries_subqueries_17_in_having | 2.01 | 5068 | 46 | 2 | OK |
| subqueries_subqueries_18_correlated_exists_join | 1.42 | 6292 | 139 | 2 | OK |
| subqueries_subqueries_19_scalar_case | 2.00 | 6301 | 66 | 2 | OK |
| subqueries_subqueries_20_anti_join_complex | 52.56 | 11152 | 5023 | 6 | OK |

### advanced

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| advanced_advanced_01_row_number | 2.15 | 966 | 41 | 2 | OK |
| advanced_advanced_02_rank_dense_rank | 2.00 | 1571 | 40 | 1 | OK |
| advanced_advanced_03_lag_lead | 1.84 | 970 | 48 | 2 | OK |
| advanced_advanced_04_ntile | 1.90 | 966 | 39 | 2 | OK |
| advanced_advanced_05_window_frame | 1.85 | 970 | 48 | 2 | OK |
| advanced_advanced_06_multi_window | 2.02 | 973 | 78 | 2 | OK |
| advanced_advanced_07_filter_clause | 1.82 | 1937 | 57 | 2 | OK |
| advanced_advanced_08_grouping_sets | 1.58 | 6695 | 358 | 2 | OK |
| advanced_advanced_09_window_with_join | 2.00 | 4004 | 138 | 2 | OK |

### complex_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| complex_joins_complex_joins_01_star_schema | 56.18 | 11153 | 4131 | 7 | OK |
| complex_joins_complex_joins_02_snowflake | 61.42 | 11153 | 4556 | 7 | OK |
| complex_joins_complex_joins_03_six_table | 72.50 | 13583 | 4878 | 7 | OK |
| complex_joins_complex_joins_04_self_join_alias | 2.43 | 3864 | 149 | 2 | OK |
| complex_joins_complex_joins_05_anti_join | 2.04 | 3862 | 23 | 2 | OK |
| complex_joins_complex_joins_06_semi_join_exists | 2.07 | 3862 | 41 | 2 | OK |
| complex_joins_complex_joins_07_semi_join_in | 1.60 | 8724 | 290 | 2 | OK |
| complex_joins_complex_joins_08_case_in_join | 2.01 | 4265 | 86 | 2 | OK |
| complex_joins_complex_joins_09_derived_table | 1.97 | 4770 | 96 | 2 | OK |
| complex_joins_complex_joins_10_multi_self_join | 1.90 | 3864 | 99 | 2 | OK |
| complex_joins_complex_joins_11_full_outer | 1.22 | 6296 | 63 | 2 | OK |
| complex_joins_complex_joins_12_five_table_agg | 57.87 | 11655 | 4145 | 7 | OK |
| complex_joins_complex_joins_13_anti_join_not_in | 2.08 | 3864 | 26 | 2 | OK |
| complex_joins_complex_joins_14_bushy_join | 1.50 | 6702 | 203 | 2 | OK |
| complex_joins_complex_joins_15_theta_join | 2.18 | 3865 | 207 | 2 | OK |
| complex_joins_complex_joins_16_multi_key_join | 1.69 | 6294 | 375 | 2 | OK |
| complex_joins_complex_joins_17_correlated_anti | 1.96 | 6693 | 69 | 2 | OK |
| complex_joins_complex_joins_18_cross_join_filtered | 1.92 | 3963 | 40 | 2 | OK |
| complex_joins_complex_joins_19_seven_table | 18.46 | 16015 | 4835 | 4 | OK |
| complex_joins_complex_joins_20_diamond_join | 71.49 | 13582 | 5292 | 7 | OK |

### aggregations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| aggregations_aggregations_01_group_by_multi | 1.97 | 1937 | 42 | 2 | OK |
| aggregations_aggregations_02_having | 1.90 | 2337 | 44 | 2 | OK |
| aggregations_aggregations_03_count_distinct | 1.89 | 1836 | 44 | 2 | OK |
| aggregations_aggregations_04_mixed_aggregates | 1.76 | 1835 | 40 | 2 | OK |
| aggregations_aggregations_05_expression_group | 1.87 | 1937 | 39 | 2 | OK |
| aggregations_aggregations_06_join_aggregate | 19.04 | 11654 | 4062 | 4 | OK |
| aggregations_aggregations_07_nested_aggregate | 2.16 | 6700 | 94 | 2 | OK |
| aggregations_aggregations_08_having_complex | 2.02 | 2341 | 97 | 2 | OK |
| aggregations_aggregations_09_percentile | 1.85 | 1935 | 45 | 2 | OK |
| aggregations_aggregations_10_multi_level | 1.34 | 7197 | 155 | 2 | OK |
| aggregations_aggregations_11_rollup | 1.47 | 6694 | 282 | 2 | OK |
| aggregations_aggregations_12_cube | 1.77 | 1836 | 46 | 2 | OK |
| aggregations_aggregations_13_conditional_agg | 1.87 | 1937 | 50 | 2 | OK |
| aggregations_aggregations_14_distinct_on_join | 1.52 | 9223 | 348 | 2 | OK |
| aggregations_aggregations_15_aggregate_filter | 2.04 | 7203 | 114 | 2 | OK |

### unsupported

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| unsupported_unsupported_01_pivot | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_02_json_path | 1.80 | 832 | 34 | 2 | OK |
| unsupported_unsupported_03_xmltable | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_04_full_outer_complex | 2.04 | 3869 | 95 | 2 | OK |
| unsupported_unsupported_05_merge | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_06_multi_table_update | 0.03 | 0 | 0 | 0 | OK |
| unsupported_unsupported_07_match_recognize | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_08_multi_table_delete | 0.00 | 0 | 0 | 0 | OK |

### ctes

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| ctes_ctes_01_simple | 1.22 | 5202 | 93 | 2 | OK |
| ctes_ctes_02_multiple | 1.28 | 6043 | 115 | 2 | OK |
| ctes_ctes_03_with_aggregation | 1.43 | 10666 | 194 | 2 | OK |
| ctes_ctes_04_recursive | 1.32 | 7226 | 144 | 2 | OK |
| ctes_ctes_05_cte_in_join | 1.43 | 8638 | 227 | 2 | OK |
| ctes_ctes_06_cte_window | 1.31 | 5541 | 146 | 2 | OK |
| ctes_ctes_07_three_chain | 1.50 | 11524 | 219 | 2 | OK |
| ctes_ctes_08_recursive_numbers | 1.25 | 7625 | 73 | 2 | OK |
| ctes_ctes_09_multi_use | 1.33 | 9968 | 82 | 2 | OK |
| ctes_ctes_10_cte_subquery | 1.93 | 15527 | 477 | 2 | OK |
| ctes_ctes_11_cte_exists | 1.23 | 4803 | 60 | 2 | OK |
| ctes_ctes_12_recursive_depth | 2.10 | 8031 | 164 | 2 | OK |

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 1.88 | 3862 | 99 | 2 | OK |
| basic_joins_02_left_join | 1.75 | 4263 | 28 | 2 | OK |
| basic_joins_03_right_join | 1.74 | 3861 | 22 | 2 | OK |
| basic_joins_04_equi_join_filter | 1.94 | 3863 | 151 | 2 | OK |
| basic_joins_05_three_table_join | 1.37 | 6292 | 275 | 2 | OK |
| basic_joins_06_foreign_key | 1.85 | 3862 | 100 | 2 | OK |
| basic_joins_07_multi_predicate_join | 1.95 | 3863 | 148 | 2 | OK |
| basic_joins_08_cross_product | 1.78 | 3863 | 29 | 2 | OK |
| basic_joins_09_join_aggregate | 1.85 | 4263 | 103 | 2 | OK |
| basic_joins_10_self_join | 1.85 | 3863 | 100 | 2 | OK |
| basic_joins_11_dimension_table | 1.80 | 4362 | 70 | 2 | OK |
| basic_joins_12_join_with_in | 1.84 | 3862 | 100 | 2 | OK |
| basic_joins_13_non_equi_join | 1.85 | 3864 | 113 | 2 | OK |
| basic_joins_14_join_distinct | 1.78 | 4161 | 59 | 2 | OK |
| basic_joins_15_join_computed | 1.95 | 3863 | 153 | 2 | OK |

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 1.69 | 830 | 17 | 2 | OK |
| simple_02_simple_aggregate | 1.68 | 1834 | 30 | 1 | OK |
| simple_03_group_by | 1.64 | 1833 | 28 | 1 | OK |
| simple_04_filter_aggregate | 1.73 | 1936 | 41 | 2 | OK |
| simple_05_selective_filter | 1.80 | 831 | 45 | 2 | OK |
| simple_06_order_limit | 1.70 | 1533 | 21 | 1 | OK |
| simple_07_distinct_count | 1.70 | 1832 | 16 | 1 | OK |
| simple_08_having_clause | 1.75 | 2235 | 30 | 2 | OK |
| simple_09_multiple_filters | 1.76 | 831 | 42 | 2 | OK |
| simple_10_offset | 1.65 | 1533 | 23 | 1 | OK |

## Feature Coverage

- Parser success rate: 96.7%
- Optimizer success rate: 100.0%

## Failed Queries

| Query ID | Category | Error |
|----------|----------|-------|
| unsupported_unsupported_01_pivot | unsupported | Parse error: syntax error: unexpected IDENT 'PIVOT' (expected one of: end of input, SEMICOLON) |
| unsupported_unsupported_03_xmltable | unsupported | Parse error: syntax error: unexpected IDENT 'PASSING' (expected one of: COMMA, RPAREN) |
| unsupported_unsupported_05_merge | unsupported | Parse error: syntax error: unexpected IDENT 'MERGE' (expected one of: LPAREN, SELECT, WITH, VALUES, INSERT, UPDATE, DELETE); syntax error: unexpected IDENT 'source' (expected one of: end of input, SEMICOLON) |
| unsupported_unsupported_07_match_recognize | unsupported | Parse error: failed to parse SQL: unexpected character '{' at position 336 |

