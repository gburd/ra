# Planner Comparison Benchmark Report

**Generated**: 2026-05-29T19:10:03.738602+00:00
**Git Commit**: cb6a24e3

## Overall Summary

- Total queries: 120
- Parsed successfully: 116 (96.7%)
- Optimized successfully: 116 (100.0%)
- Median plan time: 1.86ms
- P95 plan time: 56.46ms
- Total plan time: 836.00ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| advanced | 9 | 9 | 9 | 2.13ms | 235.15ms | 48 | 2 |
| ctes | 12 | 12 | 12 | 1.43ms | 2.26ms | 146 | 2 |
| set_operations | 11 | 11 | 11 | 1.78ms | 1.98ms | 49 | 2 |
| complex_joins | 20 | 20 | 20 | 2.06ms | 73.51ms | 203 | 2 |
| unsupported | 8 | 4 | 4 | 2.02ms | 2.12ms | 34 | 2 |
| aggregations | 15 | 15 | 15 | 1.85ms | 19.31ms | 50 | 2 |
| subqueries | 20 | 20 | 20 | 1.85ms | 52.71ms | 59 | 2 |
| simple | 10 | 10 | 10 | 1.81ms | 2.00ms | 30 | 2 |
| basic_joins | 15 | 15 | 15 | 1.91ms | 2.62ms | 100 | 2 |

## Detailed Query Results

### advanced

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| advanced_advanced_01_row_number | 235.15 | 966 | 41 | 2 | OK |
| advanced_advanced_02_rank_dense_rank | 2.42 | 1571 | 40 | 1 | OK |
| advanced_advanced_03_lag_lead | 2.14 | 970 | 48 | 2 | OK |
| advanced_advanced_04_ntile | 2.04 | 966 | 39 | 2 | OK |
| advanced_advanced_05_window_frame | 2.10 | 970 | 48 | 2 | OK |
| advanced_advanced_06_multi_window | 2.13 | 973 | 78 | 2 | OK |
| advanced_advanced_07_filter_clause | 2.03 | 1937 | 57 | 2 | OK |
| advanced_advanced_08_grouping_sets | 1.75 | 6695 | 358 | 2 | OK |
| advanced_advanced_09_window_with_join | 2.24 | 4004 | 138 | 2 | OK |

### ctes

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| ctes_ctes_01_simple | 1.40 | 5202 | 93 | 2 | OK |
| ctes_ctes_02_multiple | 1.39 | 6043 | 115 | 2 | OK |
| ctes_ctes_03_with_aggregation | 1.58 | 10666 | 194 | 2 | OK |
| ctes_ctes_04_recursive | 1.41 | 7226 | 144 | 2 | OK |
| ctes_ctes_05_cte_in_join | 1.58 | 8638 | 227 | 2 | OK |
| ctes_ctes_06_cte_window | 1.43 | 5541 | 146 | 2 | OK |
| ctes_ctes_07_three_chain | 1.63 | 11524 | 219 | 2 | OK |
| ctes_ctes_08_recursive_numbers | 1.34 | 7625 | 73 | 2 | OK |
| ctes_ctes_09_multi_use | 1.40 | 9968 | 82 | 2 | OK |
| ctes_ctes_10_cte_subquery | 2.02 | 15527 | 477 | 2 | OK |
| ctes_ctes_11_cte_exists | 1.26 | 4803 | 60 | 2 | OK |
| ctes_ctes_12_recursive_depth | 2.26 | 8031 | 164 | 2 | OK |

### set_operations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| set_operations_set_operations_01_union | 1.90 | 1710 | 44 | 2 | OK |
| set_operations_set_operations_02_union_all | 1.98 | 1712 | 66 | 2 | OK |
| set_operations_set_operations_03_intersect | 1.81 | 2912 | 17 | 2 | OK |
| set_operations_set_operations_04_intersect_all | 1.82 | 1709 | 36 | 2 | OK |
| set_operations_set_operations_05_except | 1.78 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_06_except_all | 1.73 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_07_nested | 1.21 | 2589 | 54 | 2 | OK |
| set_operations_set_operations_08_order_limit | 1.88 | 1812 | 52 | 2 | OK |
| set_operations_set_operations_09_union_join | 1.49 | 7775 | 214 | 2 | OK |
| set_operations_set_operations_10_three_way_union | 1.17 | 3794 | 49 | 2 | OK |
| set_operations_set_operations_11_except_with_join | 1.46 | 10803 | 277 | 2 | OK |

### complex_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| complex_joins_complex_joins_01_star_schema | 56.46 | 11153 | 4131 | 7 | OK |
| complex_joins_complex_joins_02_snowflake | 61.07 | 11153 | 4556 | 7 | OK |
| complex_joins_complex_joins_03_six_table | 71.98 | 13583 | 4878 | 7 | OK |
| complex_joins_complex_joins_04_self_join_alias | 2.46 | 3864 | 149 | 2 | OK |
| complex_joins_complex_joins_05_anti_join | 2.10 | 3862 | 23 | 2 | OK |
| complex_joins_complex_joins_06_semi_join_exists | 1.88 | 3862 | 41 | 2 | OK |
| complex_joins_complex_joins_07_semi_join_in | 1.52 | 8724 | 290 | 2 | OK |
| complex_joins_complex_joins_08_case_in_join | 1.88 | 4265 | 86 | 2 | OK |
| complex_joins_complex_joins_09_derived_table | 1.91 | 4770 | 96 | 2 | OK |
| complex_joins_complex_joins_10_multi_self_join | 1.93 | 3864 | 99 | 2 | OK |
| complex_joins_complex_joins_11_full_outer | 1.19 | 6296 | 63 | 2 | OK |
| complex_joins_complex_joins_12_five_table_agg | 57.53 | 11655 | 4145 | 7 | OK |
| complex_joins_complex_joins_13_anti_join_not_in | 2.06 | 3864 | 26 | 2 | OK |
| complex_joins_complex_joins_14_bushy_join | 1.69 | 6702 | 203 | 2 | OK |
| complex_joins_complex_joins_15_theta_join | 2.52 | 3865 | 207 | 2 | OK |
| complex_joins_complex_joins_16_multi_key_join | 1.63 | 6294 | 375 | 2 | OK |
| complex_joins_complex_joins_17_correlated_anti | 1.92 | 6693 | 69 | 2 | OK |
| complex_joins_complex_joins_18_cross_join_filtered | 1.88 | 3963 | 40 | 2 | OK |
| complex_joins_complex_joins_19_seven_table | 18.53 | 16015 | 4835 | 4 | OK |
| complex_joins_complex_joins_20_diamond_join | 73.51 | 13582 | 5292 | 7 | OK |

### unsupported

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| unsupported_unsupported_01_pivot | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_02_json_path | 2.12 | 832 | 34 | 2 | OK |
| unsupported_unsupported_03_xmltable | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_04_full_outer_complex | 2.02 | 3869 | 95 | 2 | OK |
| unsupported_unsupported_05_merge | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_06_multi_table_update | 0.04 | 0 | 0 | 0 | OK |
| unsupported_unsupported_07_match_recognize | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_08_multi_table_delete | 0.00 | 0 | 0 | 0 | OK |

### aggregations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| aggregations_aggregations_01_group_by_multi | 1.85 | 1937 | 42 | 2 | OK |
| aggregations_aggregations_02_having | 1.91 | 2337 | 44 | 2 | OK |
| aggregations_aggregations_03_count_distinct | 1.86 | 1836 | 44 | 2 | OK |
| aggregations_aggregations_04_mixed_aggregates | 1.76 | 1835 | 40 | 2 | OK |
| aggregations_aggregations_05_expression_group | 1.78 | 1937 | 39 | 2 | OK |
| aggregations_aggregations_06_join_aggregate | 19.31 | 11654 | 4062 | 4 | OK |
| aggregations_aggregations_07_nested_aggregate | 2.13 | 6700 | 94 | 2 | OK |
| aggregations_aggregations_08_having_complex | 2.03 | 2341 | 97 | 2 | OK |
| aggregations_aggregations_09_percentile | 1.83 | 1935 | 45 | 2 | OK |
| aggregations_aggregations_10_multi_level | 1.32 | 7197 | 155 | 2 | OK |
| aggregations_aggregations_11_rollup | 1.44 | 6694 | 282 | 2 | OK |
| aggregations_aggregations_12_cube | 1.77 | 1836 | 46 | 2 | OK |
| aggregations_aggregations_13_conditional_agg | 1.88 | 1937 | 50 | 2 | OK |
| aggregations_aggregations_14_distinct_on_join | 1.52 | 9223 | 348 | 2 | OK |
| aggregations_aggregations_15_aggregate_filter | 1.99 | 7203 | 114 | 2 | OK |

### subqueries

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| subqueries_subqueries_01_scalar_select | 1.88 | 4268 | 43 | 2 | OK |
| subqueries_subqueries_02_scalar_where | 1.83 | 4264 | 59 | 2 | OK |
| subqueries_subqueries_03_exists | 1.76 | 3862 | 41 | 2 | OK |
| subqueries_subqueries_04_not_exists | 1.18 | 6292 | 112 | 2 | OK |
| subqueries_subqueries_05_in_simple | 1.84 | 3867 | 33 | 2 | OK |
| subqueries_subqueries_06_not_in | 1.79 | 3864 | 23 | 2 | OK |
| subqueries_subqueries_07_derived_table | 1.46 | 6795 | 222 | 2 | OK |
| subqueries_subqueries_08_multi_derived | 2.12 | 7605 | 178 | 2 | OK |
| subqueries_subqueries_09_correlated_agg | 1.88 | 4264 | 59 | 2 | OK |
| subqueries_subqueries_10_nested_in | 1.27 | 8730 | 58 | 2 | OK |
| subqueries_subqueries_11_gt_all | 1.77 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_12_gt_any | 1.84 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_13_lateral | 1.96 | 3971 | 54 | 2 | OK |
| subqueries_subqueries_14_lateral_agg | 1.91 | 4271 | 61 | 2 | OK |
| subqueries_subqueries_15_exists_multi | 1.96 | 3863 | 76 | 2 | OK |
| subqueries_subqueries_16_scalar_multi | 1.87 | 9935 | 67 | 2 | OK |
| subqueries_subqueries_17_in_having | 1.85 | 5068 | 46 | 2 | OK |
| subqueries_subqueries_18_correlated_exists_join | 1.24 | 6292 | 139 | 2 | OK |
| subqueries_subqueries_19_scalar_case | 1.91 | 6301 | 66 | 2 | OK |
| subqueries_subqueries_20_anti_join_complex | 52.71 | 11152 | 5023 | 6 | OK |

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 2.00 | 830 | 17 | 2 | OK |
| simple_02_simple_aggregate | 1.96 | 1834 | 30 | 1 | OK |
| simple_03_group_by | 1.81 | 1833 | 28 | 1 | OK |
| simple_04_filter_aggregate | 1.81 | 1936 | 41 | 2 | OK |
| simple_05_selective_filter | 1.81 | 831 | 45 | 2 | OK |
| simple_06_order_limit | 1.69 | 1533 | 21 | 1 | OK |
| simple_07_distinct_count | 1.74 | 1832 | 16 | 1 | OK |
| simple_08_having_clause | 1.73 | 2235 | 30 | 2 | OK |
| simple_09_multiple_filters | 1.78 | 831 | 42 | 2 | OK |
| simple_10_offset | 1.69 | 1533 | 23 | 1 | OK |

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 1.91 | 3862 | 99 | 2 | OK |
| basic_joins_02_left_join | 1.73 | 4263 | 28 | 2 | OK |
| basic_joins_03_right_join | 1.78 | 3861 | 22 | 2 | OK |
| basic_joins_04_equi_join_filter | 2.09 | 3863 | 151 | 2 | OK |
| basic_joins_05_three_table_join | 1.47 | 6292 | 275 | 2 | OK |
| basic_joins_06_foreign_key | 2.06 | 3862 | 100 | 2 | OK |
| basic_joins_07_multi_predicate_join | 2.00 | 3863 | 148 | 2 | OK |
| basic_joins_08_cross_product | 1.80 | 3863 | 29 | 2 | OK |
| basic_joins_09_join_aggregate | 1.98 | 4263 | 103 | 2 | OK |
| basic_joins_10_self_join | 2.00 | 3863 | 100 | 2 | OK |
| basic_joins_11_dimension_table | 1.86 | 4362 | 70 | 2 | OK |
| basic_joins_12_join_with_in | 2.62 | 3862 | 100 | 2 | OK |
| basic_joins_13_non_equi_join | 1.91 | 3864 | 113 | 2 | OK |
| basic_joins_14_join_distinct | 1.73 | 4161 | 59 | 2 | OK |
| basic_joins_15_join_computed | 1.90 | 3863 | 153 | 2 | OK |

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

