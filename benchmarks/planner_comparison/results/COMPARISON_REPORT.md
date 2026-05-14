# Planner Comparison Benchmark Report

**Generated**: 2026-05-12T19:47:13.922715+00:00
**Git Commit**: dc7e9178

## Overall Summary

- Total queries: 120
- Parsed successfully: 114 (95.0%)
- Optimized successfully: 114 (100.0%)
- Median plan time: 1.20ms
- P95 plan time: 58.43ms
- Total plan time: 836.81ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| subqueries | 20 | 20 | 20 | 1.23ms | 223.90ms | 57 | 2 |
| advanced | 9 | 9 | 9 | 1.19ms | 1.53ms | 48 | 2 |
| aggregations | 15 | 15 | 15 | 1.24ms | 68.93ms | 50 | 2 |
| unsupported | 8 | 2 | 2 | 1.28ms | 1.28ms | 59 | 2 |
| basic_joins | 15 | 15 | 15 | 1.16ms | 1.35ms | 111 | 2 |
| complex_joins | 20 | 20 | 20 | 1.31ms | 76.34ms | 147 | 2 |
| ctes | 12 | 12 | 12 | 1.27ms | 1.71ms | 146 | 2 |
| set_operations | 11 | 11 | 11 | 1.04ms | 1.31ms | 49 | 2 |
| simple | 10 | 10 | 10 | 1.02ms | 1.18ms | 30 | 2 |

## Detailed Query Results

### subqueries

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| subqueries_subqueries_01_scalar_select | 223.90 | 4268 | 43 | 2 | OK |
| subqueries_subqueries_02_scalar_where | 1.34 | 4264 | 57 | 2 | OK |
| subqueries_subqueries_03_exists | 1.22 | 3867 | 43 | 2 | OK |
| subqueries_subqueries_04_not_exists | 1.40 | 6294 | 158 | 2 | OK |
| subqueries_subqueries_05_in_simple | 1.15 | 3867 | 33 | 2 | OK |
| subqueries_subqueries_06_not_in | 1.10 | 3864 | 23 | 2 | OK |
| subqueries_subqueries_07_derived_table | 1.55 | 6995 | 222 | 2 | OK |
| subqueries_subqueries_08_multi_derived | 1.68 | 7805 | 197 | 2 | OK |
| subqueries_subqueries_09_correlated_agg | 1.22 | 4264 | 57 | 2 | OK |
| subqueries_subqueries_10_nested_in | 1.21 | 8730 | 58 | 2 | OK |
| subqueries_subqueries_11_gt_all | 1.05 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_12_gt_any | 1.02 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_13_lateral | 1.14 | 4169 | 54 | 2 | OK |
| subqueries_subqueries_14_lateral_agg | 1.19 | 4269 | 59 | 2 | OK |
| subqueries_subqueries_15_exists_multi | 1.26 | 6301 | 52 | 2 | OK |
| subqueries_subqueries_16_scalar_multi | 1.23 | 9935 | 67 | 2 | OK |
| subqueries_subqueries_17_in_having | 1.23 | 5065 | 41 | 2 | OK |
| subqueries_subqueries_18_correlated_exists_join | 1.48 | 6295 | 171 | 2 | OK |
| subqueries_subqueries_19_scalar_case | 1.27 | 6301 | 68 | 2 | OK |
| subqueries_subqueries_20_anti_join_complex | 23.22 | 11154 | 6149 | 4 | OK |

### advanced

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| advanced_advanced_01_row_number | 1.19 | 1233 | 41 | 2 | OK |
| advanced_advanced_02_rank_dense_rank | 1.04 | 1837 | 40 | 1 | OK |
| advanced_advanced_03_lag_lead | 1.09 | 1236 | 48 | 2 | OK |
| advanced_advanced_04_ntile | 1.14 | 1233 | 39 | 2 | OK |
| advanced_advanced_05_window_frame | 1.14 | 1236 | 48 | 2 | OK |
| advanced_advanced_06_multi_window | 1.25 | 1240 | 77 | 2 | OK |
| advanced_advanced_07_filter_clause | 1.28 | 2137 | 57 | 2 | OK |
| advanced_advanced_08_grouping_sets | 1.53 | 6695 | 333 | 2 | OK |
| advanced_advanced_09_window_with_join | 1.34 | 4271 | 149 | 2 | OK |

### aggregations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| aggregations_aggregations_01_group_by_multi | 1.13 | 2137 | 42 | 2 | OK |
| aggregations_aggregations_02_having | 1.22 | 2537 | 44 | 2 | OK |
| aggregations_aggregations_03_count_distinct | 1.20 | 1836 | 43 | 2 | OK |
| aggregations_aggregations_04_mixed_aggregates | 1.14 | 1835 | 40 | 2 | OK |
| aggregations_aggregations_05_expression_group | 1.12 | 2137 | 39 | 2 | OK |
| aggregations_aggregations_06_join_aggregate | 68.93 | 11854 | 5644 | 8 | OK |
| aggregations_aggregations_07_nested_aggregate | 1.53 | 6698 | 97 | 2 | OK |
| aggregations_aggregations_08_having_complex | 1.54 | 2541 | 90 | 2 | OK |
| aggregations_aggregations_09_percentile | 1.10 | 2135 | 45 | 2 | OK |
| aggregations_aggregations_10_multi_level | 1.51 | 7397 | 155 | 2 | OK |
| aggregations_aggregations_11_rollup | 1.40 | 6694 | 282 | 2 | OK |
| aggregations_aggregations_12_cube | 1.50 | 1836 | 45 | 2 | OK |
| aggregations_aggregations_13_conditional_agg | 1.11 | 2137 | 50 | 2 | OK |
| aggregations_aggregations_14_distinct_on_join | 1.43 | 9423 | 348 | 2 | OK |
| aggregations_aggregations_15_aggregate_filter | 1.24 | 7400 | 118 | 2 | OK |

### unsupported

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| unsupported_unsupported_01_pivot | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_02_json_path | 1.20 | 832 | 34 | 2 | OK |
| unsupported_unsupported_03_xmltable | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_04_full_outer_complex | 1.28 | 3869 | 59 | 2 | OK |
| unsupported_unsupported_05_merge | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_06_multi_table_update | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_07_match_recognize | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_08_multi_table_delete | 0.00 | 0 | 0 | 0 | FAILED |

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 1.22 | 3862 | 110 | 2 | OK |
| basic_joins_02_left_join | 1.09 | 4263 | 28 | 2 | OK |
| basic_joins_03_right_join | 1.04 | 3861 | 22 | 2 | OK |
| basic_joins_04_equi_join_filter | 1.27 | 3863 | 149 | 2 | OK |
| basic_joins_05_three_table_join | 1.35 | 6292 | 275 | 2 | OK |
| basic_joins_06_foreign_key | 1.17 | 3862 | 111 | 2 | OK |
| basic_joins_07_multi_predicate_join | 1.33 | 3863 | 146 | 2 | OK |
| basic_joins_08_cross_product | 1.07 | 3861 | 42 | 2 | OK |
| basic_joins_09_join_aggregate | 1.16 | 4263 | 114 | 2 | OK |
| basic_joins_10_self_join | 1.14 | 3863 | 111 | 2 | OK |
| basic_joins_11_dimension_table | 1.15 | 4562 | 78 | 2 | OK |
| basic_joins_12_join_with_in | 1.16 | 3862 | 111 | 2 | OK |
| basic_joins_13_non_equi_join | 1.12 | 3864 | 77 | 2 | OK |
| basic_joins_14_join_distinct | 1.12 | 4161 | 67 | 2 | OK |
| basic_joins_15_join_computed | 1.25 | 3863 | 151 | 2 | OK |

### complex_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| complex_joins_complex_joins_01_star_schema | 58.43 | 11153 | 4836 | 8 | OK |
| complex_joins_complex_joins_02_snowflake | 57.68 | 11153 | 4874 | 8 | OK |
| complex_joins_complex_joins_03_six_table | 71.19 | 13583 | 5771 | 8 | OK |
| complex_joins_complex_joins_04_self_join_alias | 1.31 | 3864 | 147 | 2 | OK |
| complex_joins_complex_joins_05_anti_join | 1.05 | 3866 | 31 | 2 | OK |
| complex_joins_complex_joins_06_semi_join_exists | 1.06 | 3867 | 43 | 2 | OK |
| complex_joins_complex_joins_07_semi_join_in | 1.48 | 8724 | 290 | 2 | OK |
| complex_joins_complex_joins_08_case_in_join | 1.18 | 4265 | 94 | 2 | OK |
| complex_joins_complex_joins_09_derived_table | 1.17 | 4970 | 104 | 2 | OK |
| complex_joins_complex_joins_10_multi_self_join | 1.08 | 3864 | 52 | 2 | OK |
| complex_joins_complex_joins_11_full_outer | 1.08 | 6296 | 55 | 2 | OK |
| complex_joins_complex_joins_12_five_table_agg | 58.15 | 11855 | 4850 | 8 | OK |
| complex_joins_complex_joins_13_anti_join_not_in | 1.09 | 3864 | 26 | 2 | OK |
| complex_joins_complex_joins_14_bushy_join | 1.40 | 6702 | 203 | 2 | OK |
| complex_joins_complex_joins_15_theta_join | 1.31 | 3865 | 142 | 2 | OK |
| complex_joins_complex_joins_16_multi_key_join | 1.40 | 6294 | 284 | 2 | OK |
| complex_joins_complex_joins_17_correlated_anti | 1.19 | 6697 | 88 | 2 | OK |
| complex_joins_complex_joins_18_cross_join_filtered | 1.05 | 4163 | 44 | 2 | OK |
| complex_joins_complex_joins_19_seven_table | 76.34 | 16015 | 6134 | 8 | OK |
| complex_joins_complex_joins_20_diamond_join | 72.56 | 13582 | 5798 | 8 | OK |

### ctes

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| ctes_ctes_01_simple | 1.23 | 5402 | 93 | 2 | OK |
| ctes_ctes_02_multiple | 1.22 | 6243 | 115 | 2 | OK |
| ctes_ctes_03_with_aggregation | 1.39 | 10866 | 165 | 2 | OK |
| ctes_ctes_04_recursive | 1.27 | 7426 | 144 | 2 | OK |
| ctes_ctes_05_cte_in_join | 1.31 | 8838 | 227 | 2 | OK |
| ctes_ctes_06_cte_window | 1.32 | 6008 | 146 | 2 | OK |
| ctes_ctes_07_three_chain | 1.44 | 11724 | 197 | 2 | OK |
| ctes_ctes_08_recursive_numbers | 1.12 | 7825 | 65 | 2 | OK |
| ctes_ctes_09_multi_use | 1.14 | 9968 | 82 | 2 | OK |
| ctes_ctes_10_cte_subquery | 1.71 | 15727 | 474 | 2 | OK |
| ctes_ctes_11_cte_exists | 1.09 | 5007 | 64 | 2 | OK |
| ctes_ctes_12_recursive_depth | 1.27 | 8231 | 175 | 2 | OK |

### set_operations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| set_operations_set_operations_01_union | 0.99 | 1710 | 44 | 2 | OK |
| set_operations_set_operations_02_union_all | 1.04 | 1712 | 65 | 2 | OK |
| set_operations_set_operations_03_intersect | 0.96 | 2912 | 17 | 2 | OK |
| set_operations_set_operations_04_intersect_all | 1.11 | 1709 | 36 | 2 | OK |
| set_operations_set_operations_05_except | 0.96 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_06_except_all | 0.92 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_07_nested | 1.15 | 2589 | 54 | 2 | OK |
| set_operations_set_operations_08_order_limit | 1.06 | 2012 | 52 | 2 | OK |
| set_operations_set_operations_09_union_join | 1.31 | 7775 | 214 | 2 | OK |
| set_operations_set_operations_10_three_way_union | 1.03 | 3794 | 49 | 2 | OK |
| set_operations_set_operations_11_except_with_join | 1.31 | 10803 | 277 | 2 | OK |

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 1.00 | 830 | 17 | 2 | OK |
| simple_02_simple_aggregate | 0.97 | 1834 | 30 | 1 | OK |
| simple_03_group_by | 0.96 | 1833 | 28 | 1 | OK |
| simple_04_filter_aggregate | 1.05 | 2136 | 40 | 2 | OK |
| simple_05_selective_filter | 1.05 | 831 | 40 | 2 | OK |
| simple_06_order_limit | 0.97 | 1733 | 21 | 1 | OK |
| simple_07_distinct_count | 0.93 | 1832 | 16 | 1 | OK |
| simple_08_having_clause | 1.06 | 2235 | 30 | 2 | OK |
| simple_09_multiple_filters | 1.18 | 831 | 41 | 2 | OK |
| simple_10_offset | 1.02 | 1733 | 23 | 1 | OK |

## Feature Coverage

- Parser success rate: 95.0%
- Optimizer success rate: 100.0%

## Failed Queries

| Query ID | Category | Error |
|----------|----------|-------|
| unsupported_unsupported_01_pivot | unsupported | Parse error: syntax error: unexpected IDENT 'PIVOT' (expected one of: end of input, SEMICOLON) |
| unsupported_unsupported_03_xmltable | unsupported | Parse error: syntax error: unexpected IDENT 'PASSING' (expected one of: COMMA, RPAREN) |
| unsupported_unsupported_05_merge | unsupported | Parse error: syntax error: unexpected IDENT 'MERGE' (expected one of: SELECT, WITH, VALUES); syntax error: unexpected RPAREN (expected one of: end of input, SEMICOLON) |
| unsupported_unsupported_06_multi_table_update | unsupported | Parse error: syntax error: unexpected IDENT 'UPDATE' (expected one of: SELECT, WITH, VALUES); parse failed: unable to recover from syntax error |
| unsupported_unsupported_07_match_recognize | unsupported | Parse error: failed to parse SQL: unexpected character '{' at position 336 |
| unsupported_unsupported_08_multi_table_delete | unsupported | Parse error: syntax error: unexpected IDENT 'DELETE' (expected one of: SELECT, WITH, VALUES); syntax error: unexpected RPAREN (expected one of: end of input, SEMICOLON) |

