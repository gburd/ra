# CockroachDB Transformation Rules

This directory contains 16 database-specific transformation rules extracted from CockroachDB v26.3.17.

## Source Information
- **Repository**: https://github.com/cockroachdb/cockroach
- **Commit**: 6e210ba6aa33cea5e27b1a8fae212c27941781f4
- **Date**: 2026-03-17
- **Primary Source**: `pkg/sql/opt/xform/rules/*.opt`

## Rules by Category

### Join Optimizations (6 rules)
1. **semi-join-to-inner-with-distinct.rra** - Converts semi-joins to inner joins with distinct for join reordering
2. **commute-left-to-right-join.rra** - Swaps left/right join inputs for exploration
3. **split-disjunction-join-to-union.rra** - Splits OR conditions in joins into unions
4. **anti-join-disjunction-to-union.rra** - Splits anti-join OR conditions into intersections
5. **reorder-joins.rra** - Main join reordering algorithm (bushy trees)
6. **locality-optimized-lookup-join.rra** - Multi-region locality-aware lookup joins
7. **convert-semi-to-inner-non-equality.rra** - Converts semi-joins with non-equality conditions

### Aggregation Optimizations (3 rules)
8. **scalar-min-max-to-limit.rra** - Replaces MIN/MAX with indexed LIMIT 1
9. **replace-min-with-limit.rra** - Eliminates GROUP BY for constant grouping columns
10. **scalar-min-max-to-subqueries.rra** - Splits multiple MIN/MAX into subqueries

### Scan & Index Optimizations (4 rules)
11. **locality-optimized-scan.rra** - Multi-region locality-aware scans
12. **generate-index-scans.rra** - Creates alternatives for each secondary index
13. **generate-limited-index-scans.rra** - Pushes limits into index scans
14. **generate-inverted-index-scans.rra** - Uses GIN indexes for JSON/array queries

### Limit Pushdown (3 rules)
15. **push-limit-into-scan.rra** - Pushes limits into filtered scans
16. **push-limit-into-project-scan.rra** - Pushes limits through projects into partial index scans

## Key Features

### Multi-Region Support
CockroachDB's unique multi-region capabilities are reflected in several rules:
- Locality-optimized scans and lookup joins
- Regional by row table support
- Gateway-aware query planning

### Advanced Join Techniques
- Lookup joins (point queries into indexed side)
- Join reordering with DP-based algorithm
- Semi-join to inner-join transformations

### Index-Aware Optimizations
- Secondary index exploration
- Inverted indexes for JSON/arrays
- Partial index support
- Index-only scans

## References

1. CockroachDB Documentation: https://www.cockroachlabs.com/docs/stable/cost-based-optimizer.html
2. Join Ordering Algorithm: Citation [8] in source code
3. Multi-Region: https://www.cockroachlabs.com/docs/stable/multiregion-overview.html
