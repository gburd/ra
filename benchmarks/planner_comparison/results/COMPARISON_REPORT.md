# Planner Comparison Benchmark Report

**Generated**: 2026-05-16T17:04:12.907815+00:00
**Git Commit**: 892f7293

## Overall Summary

- Total queries: 120
- Parsed successfully: 116 (96.7%)
- Optimized successfully: 114 (98.3%)
- Median plan time: 1.20ms
- P95 plan time: 68.00ms
- Total plan time: 940.96ms

## Results by Category

| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |
|----------|---------|--------|-----------|-------------|----------|--------------|---------------|
| basic_joins | 15 | 15 | 15 | 1.30ms | 252.72ms | 111 | 2 |
| ctes | 12 | 12 | 12 | 1.37ms | 1.80ms | 146 | 2 |
| complex_joins | 20 | 20 | 20 | 1.42ms | 89.88ms | 147 | 2 |
| set_operations | 11 | 11 | 11 | 1.11ms | 1.38ms | 49 | 2 |
| aggregations | 15 | 15 | 15 | 1.13ms | 78.07ms | 50 | 2 |
| unsupported | 8 | 4 | 2 | 1.10ms | 1.10ms | 59 | 2 |
| advanced | 9 | 9 | 9 | 1.06ms | 1.43ms | 48 | 2 |
| simple | 10 | 10 | 10 | 0.97ms | 1.02ms | 30 | 2 |
| subqueries | 20 | 20 | 20 | 1.08ms | 24.01ms | 57 | 2 |

## Detailed Query Results

### basic_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| basic_joins_01_inner_join | 252.72 | 3862 | 110 | 2 | OK |
| basic_joins_02_left_join | 1.48 | 4263 | 28 | 2 | OK |
| basic_joins_03_right_join | 1.33 | 3861 | 22 | 2 | OK |
| basic_joins_04_equi_join_filter | 1.62 | 3863 | 149 | 2 | OK |
| basic_joins_05_three_table_join | 1.49 | 6292 | 275 | 2 | OK |
| basic_joins_06_foreign_key | 1.26 | 3862 | 111 | 2 | OK |
| basic_joins_07_multi_predicate_join | 1.35 | 3863 | 146 | 2 | OK |
| basic_joins_08_cross_product | 1.18 | 3861 | 42 | 2 | OK |
| basic_joins_09_join_aggregate | 1.31 | 4263 | 114 | 2 | OK |
| basic_joins_10_self_join | 1.22 | 3863 | 111 | 2 | OK |
| basic_joins_11_dimension_table | 1.14 | 4562 | 78 | 2 | OK |
| basic_joins_12_join_with_in | 1.23 | 3862 | 111 | 2 | OK |
| basic_joins_13_non_equi_join | 1.17 | 3864 | 77 | 2 | OK |
| basic_joins_14_join_distinct | 1.21 | 4161 | 67 | 2 | OK |
| basic_joins_15_join_computed | 1.30 | 3863 | 151 | 2 | OK |

### ctes

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| ctes_ctes_01_simple | 1.23 | 5402 | 93 | 2 | OK |
| ctes_ctes_02_multiple | 1.29 | 6243 | 115 | 2 | OK |
| ctes_ctes_03_with_aggregation | 1.37 | 10866 | 165 | 2 | OK |
| ctes_ctes_04_recursive | 1.30 | 7426 | 144 | 2 | OK |
| ctes_ctes_05_cte_in_join | 1.45 | 8838 | 227 | 2 | OK |
| ctes_ctes_06_cte_window | 1.38 | 6008 | 146 | 2 | OK |
| ctes_ctes_07_three_chain | 1.49 | 11724 | 197 | 2 | OK |
| ctes_ctes_08_recursive_numbers | 1.15 | 7825 | 65 | 2 | OK |
| ctes_ctes_09_multi_use | 1.20 | 9968 | 82 | 2 | OK |
| ctes_ctes_10_cte_subquery | 1.80 | 15727 | 474 | 2 | OK |
| ctes_ctes_11_cte_exists | 1.15 | 5007 | 64 | 2 | OK |
| ctes_ctes_12_recursive_depth | 1.44 | 8231 | 175 | 2 | OK |

### complex_joins

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| complex_joins_complex_joins_01_star_schema | 66.19 | 11153 | 4836 | 8 | OK |
| complex_joins_complex_joins_02_snowflake | 68.00 | 11153 | 4874 | 8 | OK |
| complex_joins_complex_joins_03_six_table | 84.89 | 13583 | 5771 | 8 | OK |
| complex_joins_complex_joins_04_self_join_alias | 1.52 | 3864 | 147 | 2 | OK |
| complex_joins_complex_joins_05_anti_join | 1.32 | 3866 | 31 | 2 | OK |
| complex_joins_complex_joins_06_semi_join_exists | 1.28 | 3867 | 43 | 2 | OK |
| complex_joins_complex_joins_07_semi_join_in | 1.51 | 8724 | 290 | 2 | OK |
| complex_joins_complex_joins_08_case_in_join | 1.18 | 4265 | 94 | 2 | OK |
| complex_joins_complex_joins_09_derived_table | 1.26 | 4970 | 104 | 2 | OK |
| complex_joins_complex_joins_10_multi_self_join | 1.11 | 3864 | 52 | 2 | OK |
| complex_joins_complex_joins_11_full_outer | 1.07 | 6296 | 55 | 2 | OK |
| complex_joins_complex_joins_12_five_table_agg | 67.44 | 11855 | 4850 | 8 | OK |
| complex_joins_complex_joins_13_anti_join_not_in | 1.33 | 3864 | 26 | 2 | OK |
| complex_joins_complex_joins_14_bushy_join | 1.66 | 6702 | 203 | 2 | OK |
| complex_joins_complex_joins_15_theta_join | 1.26 | 3865 | 142 | 2 | OK |
| complex_joins_complex_joins_16_multi_key_join | 1.42 | 6294 | 284 | 2 | OK |
| complex_joins_complex_joins_17_correlated_anti | 1.26 | 6697 | 88 | 2 | OK |
| complex_joins_complex_joins_18_cross_join_filtered | 1.07 | 4163 | 44 | 2 | OK |
| complex_joins_complex_joins_19_seven_table | 89.88 | 16015 | 6134 | 8 | OK |
| complex_joins_complex_joins_20_diamond_join | 85.21 | 13582 | 5798 | 8 | OK |

### set_operations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| set_operations_set_operations_01_union | 1.35 | 1710 | 44 | 2 | OK |
| set_operations_set_operations_02_union_all | 1.38 | 1712 | 65 | 2 | OK |
| set_operations_set_operations_03_intersect | 1.24 | 2912 | 17 | 2 | OK |
| set_operations_set_operations_04_intersect_all | 1.10 | 1709 | 36 | 2 | OK |
| set_operations_set_operations_05_except | 0.97 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_06_except_all | 0.96 | 2912 | 16 | 1 | OK |
| set_operations_set_operations_07_nested | 1.09 | 2589 | 54 | 2 | OK |
| set_operations_set_operations_08_order_limit | 1.11 | 2012 | 52 | 2 | OK |
| set_operations_set_operations_09_union_join | 1.30 | 7775 | 214 | 2 | OK |
| set_operations_set_operations_10_three_way_union | 1.10 | 3794 | 49 | 2 | OK |
| set_operations_set_operations_11_except_with_join | 1.35 | 10803 | 277 | 2 | OK |

### aggregations

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| aggregations_aggregations_01_group_by_multi | 1.04 | 2137 | 42 | 2 | OK |
| aggregations_aggregations_02_having | 1.05 | 2537 | 44 | 2 | OK |
| aggregations_aggregations_03_count_distinct | 1.03 | 1836 | 43 | 2 | OK |
| aggregations_aggregations_04_mixed_aggregates | 1.03 | 1835 | 40 | 2 | OK |
| aggregations_aggregations_05_expression_group | 1.02 | 2137 | 39 | 2 | OK |
| aggregations_aggregations_06_join_aggregate | 78.07 | 11854 | 5644 | 8 | OK |
| aggregations_aggregations_07_nested_aggregate | 1.41 | 6698 | 97 | 2 | OK |
| aggregations_aggregations_08_having_complex | 1.26 | 2541 | 90 | 2 | OK |
| aggregations_aggregations_09_percentile | 1.07 | 2135 | 45 | 2 | OK |
| aggregations_aggregations_10_multi_level | 1.23 | 7397 | 155 | 2 | OK |
| aggregations_aggregations_11_rollup | 1.35 | 6694 | 282 | 2 | OK |
| aggregations_aggregations_12_cube | 1.13 | 1836 | 45 | 2 | OK |
| aggregations_aggregations_13_conditional_agg | 1.06 | 2137 | 50 | 2 | OK |
| aggregations_aggregations_14_distinct_on_join | 1.48 | 9423 | 348 | 2 | OK |
| aggregations_aggregations_15_aggregate_filter | 1.27 | 7400 | 118 | 2 | OK |

### unsupported

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| unsupported_unsupported_01_pivot | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_02_json_path | 1.03 | 832 | 34 | 2 | OK |
| unsupported_unsupported_03_xmltable | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_04_full_outer_complex | 1.10 | 3869 | 59 | 2 | OK |
| unsupported_unsupported_05_merge | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_06_multi_table_update | 1.04 | 0 | 0 | 0 | PARSE_ONLY |
| unsupported_unsupported_07_match_recognize | 0.00 | 0 | 0 | 0 | FAILED |
| unsupported_unsupported_08_multi_table_delete | 0.00 | 0 | 0 | 0 | PARSE_ONLY |

### advanced

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| advanced_advanced_01_row_number | 1.04 | 1233 | 41 | 2 | OK |
| advanced_advanced_02_rank_dense_rank | 1.04 | 1837 | 40 | 1 | OK |
| advanced_advanced_03_lag_lead | 1.04 | 1236 | 48 | 2 | OK |
| advanced_advanced_04_ntile | 1.06 | 1233 | 39 | 2 | OK |
| advanced_advanced_05_window_frame | 1.06 | 1236 | 48 | 2 | OK |
| advanced_advanced_06_multi_window | 1.11 | 1240 | 77 | 2 | OK |
| advanced_advanced_07_filter_clause | 1.04 | 2137 | 57 | 2 | OK |
| advanced_advanced_08_grouping_sets | 1.43 | 6695 | 333 | 2 | OK |
| advanced_advanced_09_window_with_join | 1.26 | 4271 | 149 | 2 | OK |

### simple

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| simple_01_simple_scan | 0.96 | 830 | 17 | 2 | OK |
| simple_02_simple_aggregate | 0.96 | 1834 | 30 | 1 | OK |
| simple_03_group_by | 0.95 | 1833 | 28 | 1 | OK |
| simple_04_filter_aggregate | 1.00 | 2136 | 40 | 2 | OK |
| simple_05_selective_filter | 1.01 | 831 | 40 | 2 | OK |
| simple_06_order_limit | 0.93 | 1733 | 21 | 1 | OK |
| simple_07_distinct_count | 0.91 | 1832 | 16 | 1 | OK |
| simple_08_having_clause | 0.97 | 2235 | 30 | 2 | OK |
| simple_09_multiple_filters | 0.98 | 831 | 41 | 2 | OK |
| simple_10_offset | 1.02 | 1733 | 23 | 1 | OK |

### subqueries

| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |
|----------|----------------|------|-------|-------|--------|
| subqueries_subqueries_01_scalar_select | 1.00 | 4268 | 43 | 2 | OK |
| subqueries_subqueries_02_scalar_where | 1.05 | 4264 | 57 | 2 | OK |
| subqueries_subqueries_03_exists | 1.05 | 3867 | 43 | 2 | OK |
| subqueries_subqueries_04_not_exists | 1.20 | 6294 | 158 | 2 | OK |
| subqueries_subqueries_05_in_simple | 0.97 | 3867 | 33 | 2 | OK |
| subqueries_subqueries_06_not_in | 1.01 | 3864 | 23 | 2 | OK |
| subqueries_subqueries_07_derived_table | 1.26 | 6995 | 222 | 2 | OK |
| subqueries_subqueries_08_multi_derived | 1.30 | 7805 | 197 | 2 | OK |
| subqueries_subqueries_09_correlated_agg | 1.06 | 4264 | 57 | 2 | OK |
| subqueries_subqueries_10_nested_in | 1.08 | 8730 | 58 | 2 | OK |
| subqueries_subqueries_11_gt_all | 0.90 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_12_gt_any | 0.92 | 1435 | 17 | 1 | OK |
| subqueries_subqueries_13_lateral | 1.02 | 4169 | 54 | 2 | OK |
| subqueries_subqueries_14_lateral_agg | 1.08 | 4269 | 59 | 2 | OK |
| subqueries_subqueries_15_exists_multi | 1.08 | 6301 | 52 | 2 | OK |
| subqueries_subqueries_16_scalar_multi | 1.09 | 9935 | 67 | 2 | OK |
| subqueries_subqueries_17_in_having | 1.01 | 5065 | 41 | 2 | OK |
| subqueries_subqueries_18_correlated_exists_join | 1.26 | 6295 | 171 | 2 | OK |
| subqueries_subqueries_19_scalar_case | 1.11 | 6301 | 68 | 2 | OK |
| subqueries_subqueries_20_anti_join_complex | 24.01 | 11154 | 6149 | 4 | OK |

## Feature Coverage

- Parser success rate: 96.7%
- Optimizer success rate: 98.3%

## Failed Queries

| Query ID | Category | Error |
|----------|----------|-------|
| unsupported_unsupported_01_pivot | unsupported | Parse error: syntax error: unexpected IDENT 'PIVOT' (expected one of: end of input, SEMICOLON) |
| unsupported_unsupported_03_xmltable | unsupported | Parse error: syntax error: unexpected IDENT 'PASSING' (expected one of: COMMA, RPAREN) |
| unsupported_unsupported_05_merge | unsupported | Parse error: syntax error: unexpected IDENT 'MERGE' (expected one of: LPAREN, SELECT, WITH, VALUES, INSERT, UPDATE, DELETE); syntax error: unexpected IDENT 'source' (expected one of: end of input, SEMICOLON) |
| unsupported_unsupported_06_multi_table_update | unsupported | Optimization error: failed to extract plan from e-graph: no plan could be extracted |
| unsupported_unsupported_07_match_recognize | unsupported | Parse error: failed to parse SQL: unexpected character '{' at position 336 |
| unsupported_unsupported_08_multi_table_delete | unsupported | Optimization error: failed to convert expression to e-graph: Subquery expressions are not yet supported in the e-graph representation |

