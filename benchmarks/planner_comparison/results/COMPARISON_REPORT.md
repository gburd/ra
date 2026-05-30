# Planner Comparison Benchmark Report

**Generated**: 2026-05-30T00:10:31.412971+00:00
**Git Commit**: d3273195

## Overall Summary

- Total queries: 120
- Parsed successfully: 117 (97.5%)
- Optimized successfully: 117 (100.0%)
- Median plan time: 1.71ms
- P95 plan time: 54.34ms
- Total plan time: 591.86ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| simple | 10 | 10 | 10 | 1.75ms | 2.07ms | 30 | 2 |
| set_operations | 11 | 11 | 11 | 1.61ms | 1.74ms | 49 | 2 |
| aggregations | 15 | 15 | 15 | 1.71ms | 21.07ms | 50 | 2 |
| advanced | 9 | 9 | 9 | 1.69ms | 1.84ms | 48 | 2 |
| subqueries | 20 | 20 | 20 | 1.71ms | 54.34ms | 59 | 2 |
| complex_joins | 20 | 20 | 20 | 1.80ms | 73.02ms | 203 | 2 |
| basic_joins | 15 | 15 | 15 | 1.76ms | 1.99ms | 100 | 2 |
| unsupported | 8 | 5 | 5 | 0.00ms | 1.77ms | 0 | 0 |
| ctes | 12 | 12 | 12 | 1.20ms | 1.98ms | 146 | 2 |

## Detailed Query Results

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 2.07 | 830 | 17 | 2 | OK |
| simple_02_simple_aggregate | 1.98 | 1834 | 30 | 1 | OK |
| simple_03_group_by | 1.74 | 1833 | 28 | 1 | OK |
| simple_04_filter_aggregate | 1.89 | 1936 | 41 | 2 | OK |
| simple_05_selective_filter | 1.76 | 831 | 45 | 2 | OK |
| simple_06_order_limit | 1.65 | 1533 | 21 | 1 | OK |
| simple_07_distinct_count | 1.63 | 1832 | 16 | 1 | OK |
| simple_08_having_clause | 1.69 | 2235 | 30 | 2 | OK |
| simple_09_multiple_filters | 1.75 | 831 | 42 | 2 | OK |
| simple_10_offset | 1.66 | 1533 | 23 | 1 | OK |

### set_operations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| set_operations_set_operations_01_union | 1.70 | 1710 | 44 | 2 | OK |
| set_operations_set_operations_02_union_all | 1.74 | 1712 | 66 | 2 | OK |
| set_operations_set_operations_03_intersect | 1.67 | 2912 | 17 | 2 | OK |
| set_operations_set_operations_04_intersect_all | 1.69 | 1709 | 36 | 2 | OK |
| set_operations_set_operations_05_except | 1.61 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_06_except_all | 1.61 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_07_nested | 1.07 | 2589 | 54 | 2 | OK |
| set_operations_set_operations_08_order_limit | 1.71 | 1812 | 52 | 2 | OK |
| set_operations_set_operations_09_union_join | 1.31 | 7775 | 214 | 2 | OK |
| set_operations_set_operations_10_three_way_union | 1.04 | 3794 | 49 | 2 | OK |
| set_operations_set_operations_11_except_with_join | 1.34 | 10803 | 277 | 2 | OK |

### aggregations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| aggregations_aggregations_01_group_by_multi | 1.71 | 1937 | 42 | 2 | OK |
| aggregations_aggregations_02_having | 1.71 | 2337 | 44 | 2 | OK |
| aggregations_aggregations_03_count_distinct | 1.69 | 1836 | 44 | 2 | OK |
| aggregations_aggregations_04_mixed_aggregates | 1.68 | 1835 | 40 | 2 | OK |
| aggregations_aggregations_05_expression_group | 1.68 | 1937 | 39 | 2 | OK |
| aggregations_aggregations_06_join_aggregate | 21.07 | 11654 | 4062 | 4 | OK |
| aggregations_aggregations_07_nested_aggregate | 1.84 | 6700 | 94 | 2 | OK |
| aggregations_aggregations_08_having_complex | 1.85 | 2341 | 97 | 2 | OK |
| aggregations_aggregations_09_percentile | 1.72 | 1935 | 45 | 2 | OK |
| aggregations_aggregations_10_multi_level | 1.22 | 7197 | 155 | 2 | OK |
| aggregations_aggregations_11_rollup | 1.34 | 6694 | 282 | 2 | OK |
| aggregations_aggregations_12_cube | 1.71 | 1836 | 46 | 2 | OK |
| aggregations_aggregations_13_conditional_agg | 1.70 | 1937 | 50 | 2 | OK |
| aggregations_aggregations_14_distinct_on_join | 1.43 | 9223 | 348 | 2 | OK |
| aggregations_aggregations_15_aggregate_filter | 1.89 | 7203 | 114 | 2 | OK |

### advanced

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| advanced_advanced_01_row_number | 1.67 | 966 | 41 | 2 | OK |
| advanced_advanced_02_rank_dense_rank | 1.62 | 1571 | 40 | 1 | OK |
| advanced_advanced_03_lag_lead | 1.69 | 970 | 48 | 2 | OK |
| advanced_advanced_04_ntile | 1.70 | 966 | 39 | 2 | OK |
| advanced_advanced_05_window_frame | 1.69 | 970 | 48 | 2 | OK |
| advanced_advanced_06_multi_window | 1.78 | 973 | 78 | 2 | OK |
| advanced_advanced_07_filter_clause | 1.72 | 1937 | 57 | 2 | OK |
| advanced_advanced_08_grouping_sets | 1.46 | 6695 | 358 | 2 | OK |
| advanced_advanced_09_window_with_join | 1.84 | 4004 | 138 | 2 | OK |

### subqueries

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| subqueries_subqueries_01_scalar_select | 1.69 | 4268 | 43 | 2 | OK |
| subqueries_subqueries_02_scalar_where | 1.74 | 4264 | 59 | 2 | OK |
| subqueries_subqueries_03_exists | 1.67 | 3862 | 41 | 2 | OK |
| subqueries_subqueries_04_not_exists | 1.11 | 6292 | 112 | 2 | OK |
| subqueries_subqueries_05_in_simple | 1.67 | 3867 | 33 | 2 | OK |
| subqueries_subqueries_06_not_in | 1.63 | 3864 | 23 | 2 | OK |
| subqueries_subqueries_07_derived_table | 1.30 | 6795 | 222 | 2 | OK |
| subqueries_subqueries_08_multi_derived | 2.00 | 7605 | 178 | 2 | OK |
| subqueries_subqueries_09_correlated_agg | 1.77 | 4264 | 59 | 2 | OK |
| subqueries_subqueries_10_nested_in | 1.11 | 8730 | 58 | 2 | OK |
| subqueries_subqueries_11_gt_all | 1.58 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_12_gt_any | 1.63 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_13_lateral | 1.74 | 3971 | 54 | 2 | OK |
| subqueries_subqueries_14_lateral_agg | 1.75 | 4271 | 61 | 2 | OK |
| subqueries_subqueries_15_exists_multi | 1.74 | 3863 | 76 | 2 | OK |
| subqueries_subqueries_16_scalar_multi | 1.76 | 9935 | 67 | 2 | OK |
| subqueries_subqueries_17_in_having | 1.71 | 5068 | 46 | 2 | OK |
| subqueries_subqueries_18_correlated_exists_join | 1.14 | 6292 | 139 | 2 | OK |
| subqueries_subqueries_19_scalar_case | 1.74 | 6301 | 66 | 2 | OK |
| subqueries_subqueries_20_anti_join_complex | 54.34 | 11152 | 5023 | 6 | OK |

### complex_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| complex_joins_complex_joins_01_star_schema | 59.20 | 11153 | 4131 | 7 | OK |
| complex_joins_complex_joins_02_snowflake | 60.81 | 11153 | 4556 | 7 | OK |
| complex_joins_complex_joins_03_six_table | 71.60 | 13583 | 4878 | 7 | OK |
| complex_joins_complex_joins_04_self_join_alias | 2.12 | 3864 | 149 | 2 | OK |
| complex_joins_complex_joins_05_anti_join | 1.74 | 3862 | 23 | 2 | OK |
| complex_joins_complex_joins_06_semi_join_exists | 1.70 | 3862 | 41 | 2 | OK |
| complex_joins_complex_joins_07_semi_join_in | 1.33 | 8724 | 290 | 2 | OK |
| complex_joins_complex_joins_08_case_in_join | 1.77 | 4265 | 86 | 2 | OK |
| complex_joins_complex_joins_09_derived_table | 1.79 | 4770 | 96 | 2 | OK |
| complex_joins_complex_joins_10_multi_self_join | 1.78 | 3864 | 99 | 2 | OK |
| complex_joins_complex_joins_11_full_outer | 1.09 | 6296 | 63 | 2 | OK |
| complex_joins_complex_joins_12_five_table_agg | 59.70 | 11655 | 4145 | 7 | OK |
| complex_joins_complex_joins_13_anti_join_not_in | 1.81 | 3864 | 26 | 2 | OK |
| complex_joins_complex_joins_14_bushy_join | 1.30 | 6702 | 203 | 2 | OK |
| complex_joins_complex_joins_15_theta_join | 1.99 | 3865 | 207 | 2 | OK |
| complex_joins_complex_joins_16_multi_key_join | 1.45 | 6294 | 375 | 2 | OK |
| complex_joins_complex_joins_17_correlated_anti | 1.80 | 6693 | 69 | 2 | OK |
| complex_joins_complex_joins_18_cross_join_filtered | 1.69 | 3963 | 40 | 2 | OK |
| complex_joins_complex_joins_19_seven_table | 20.07 | 16015 | 4835 | 4 | OK |
| complex_joins_complex_joins_20_diamond_join | 73.02 | 13582 | 5292 | 7 | OK |

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 1.99 | 3862 | 99 | 2 | OK |
| basic_joins_02_left_join | 1.72 | 4263 | 28 | 2 | OK |
| basic_joins_03_right_join | 1.64 | 3861 | 22 | 2 | OK |
| basic_joins_04_equi_join_filter | 1.87 | 3863 | 151 | 2 | OK |
| basic_joins_05_three_table_join | 1.30 | 6292 | 275 | 2 | OK |
| basic_joins_06_foreign_key | 1.76 | 3862 | 100 | 2 | OK |
| basic_joins_07_multi_predicate_join | 1.84 | 3863 | 148 | 2 | OK |
| basic_joins_08_cross_product | 1.76 | 3863 | 29 | 2 | OK |
| basic_joins_09_join_aggregate | 1.76 | 4263 | 103 | 2 | OK |
| basic_joins_10_self_join | 1.78 | 3863 | 100 | 2 | OK |
| basic_joins_11_dimension_table | 1.71 | 4362 | 70 | 2 | OK |
| basic_joins_12_join_with_in | 1.76 | 3862 | 100 | 2 | OK |
| basic_joins_13_non_equi_join | 1.77 | 3864 | 113 | 2 | OK |
| basic_joins_14_join_distinct | 1.77 | 4161 | 59 | 2 | OK |
| basic_joins_15_join_computed | 1.88 | 3863 | 153 | 2 | OK |

### unsupported

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| unsupported_unsupported_01_pivot | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_02_json_path | 1.66 | 832 | 34 | 2 | OK |
| unsupported_unsupported_03_xmltable | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_04_full_outer_complex | 1.77 | 3869 | 95 | 2 | OK |
| unsupported_unsupported_05_merge | 0.00 | 0 | 0 | 0 | OK |
| unsupported_unsupported_06_multi_table_update | 0.00 | 0 | 0 | 0 | OK |
| unsupported_unsupported_07_match_recognize | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_08_multi_table_delete | 0.00 | 0 | 0 | 0 | OK |

### ctes

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| ctes_ctes_01_simple | 1.10 | 5202 | 93 | 2 | OK |
| ctes_ctes_02_multiple | 1.18 | 6043 | 115 | 2 | OK |
| ctes_ctes_03_with_aggregation | 1.32 | 10666 | 194 | 2 | OK |
| ctes_ctes_04_recursive | 1.19 | 7226 | 144 | 2 | OK |
| ctes_ctes_05_cte_in_join | 1.32 | 8638 | 227 | 2 | OK |
| ctes_ctes_06_cte_window | 1.20 | 5541 | 146 | 2 | OK |
| ctes_ctes_07_three_chain | 1.39 | 11524 | 219 | 2 | OK |
| ctes_ctes_08_recursive_numbers | 1.11 | 7625 | 73 | 2 | OK |
| ctes_ctes_09_multi_use | 1.17 | 9968 | 82 | 2 | OK |
| ctes_ctes_10_cte_subquery | 1.74 | 15527 | 477 | 2 | OK |
| ctes_ctes_11_cte_exists | 1.13 | 4803 | 60 | 2 | OK |
| ctes_ctes_12_recursive_depth | 1.98 | 8031 | 164 | 2 | OK |

## Feature Coverage

- Parser success rate: 97.5%
- Optimizer success rate: 100.0%

## Failed Queries

| Query ID | Category | Error |
|----------|----------|-------|
| unsupported_unsupported_01_pivot | unsupported | Parse error: syntax error: unexpected IDENT 'PIVOT' (expected one of: end of input, SEMICOLON) |
| unsupported_unsupported_03_xmltable | unsupported | Parse error: syntax error: unexpected IDENT 'PASSING' (expected one of: COMMA, RPAREN) |
| unsupported_unsupported_07_match_recognize | unsupported | Parse error: failed to parse SQL: unexpected character '{' at position 336 |

