# SQL Feature Coverage

Feature matrix for the `ra-parser` SQL-to-RelExpr converter.

## Supported SQL Features

| Feature | Status | RelExpr Variant | Tests |
|---------|--------|-----------------|-------|
| SELECT columns | Supported | `Project` | 3 |
| SELECT * | Supported | passthrough | 1 |
| SELECT DISTINCT | Supported | `Distinct` | 2 |
| FROM table | Supported | `Scan` | all |
| FROM subquery | Supported | nested `RelExpr` | 2 |
| FROM multiple tables | Supported | `Join(Cross)` | 1 |
| WHERE | Supported | `Filter` | 6 |
| WHERE BETWEEN | Supported | `Filter(And(Ge, Le))` | 2 |
| WHERE IN (list) | Supported | `Filter(Or(Eq, ...))` | 2 |
| WHERE IS NULL | Supported | `UnaryOp(IsNull)` | 1 |
| WHERE IS NOT NULL | Supported | `UnaryOp(IsNotNull)` | 1 |
| INNER JOIN ... ON | Supported | `Join(Inner)` | 2 |
| LEFT JOIN | Supported | `Join(LeftOuter)` | 1 |
| RIGHT JOIN | Supported | `Join(RightOuter)` | 1 |
| FULL OUTER JOIN | Supported | `Join(FullOuter)` | 1 |
| CROSS JOIN | Supported | `Join(Cross)` | 1 |
| JOIN ... USING | Supported | `Join` with Eq conditions | 1 |
| GROUP BY | Supported | `Aggregate` | 3 |
| HAVING | Supported | `Filter` after `Aggregate` | 2 |
| ORDER BY | Supported | `Sort` | 4 |
| LIMIT | Supported | `Limit` | 2 |
| OFFSET | Supported | `Limit` | 2 |
| WITH / CTE | Supported | `Cte` | 3 |
| UNION [ALL] | Supported | `Union` | 2 |
| INTERSECT | Supported | `Intersect` | 1 |
| EXCEPT | Supported | `Except` | 1 |
| VALUES | Supported | `Values` | 1 |
| CASE WHEN | Supported | `Expr::Case` | 1 |
| CAST | Supported | `Expr::Cast` | 1 |
| Function calls | Supported | `Expr::Function` | 2 |

## Aggregate Functions

| Function | Status |
|----------|--------|
| COUNT | Supported |
| SUM | Supported |
| AVG | Supported |
| MIN | Supported |
| MAX | Supported |
| STDDEV_POP / STDDEV | Supported |
| STDDEV_SAMP | Supported |
| VAR_POP / VARIANCE | Supported |
| VAR_SAMP | Supported |
| STRING_AGG | Supported |
| ARRAY_AGG | Supported |
| MODE | Supported |
| BOOL_AND | Supported |
| BOOL_OR | Supported |

## Window Functions (RelExpr types)

| Function | Status |
|----------|--------|
| ROW_NUMBER | Defined |
| RANK | Defined |
| DENSE_RANK | Defined |
| PERCENT_RANK | Defined |
| CUME_DIST | Defined |
| NTILE | Defined |
| LAG | Defined |
| LEAD | Defined |
| FIRST_VALUE | Defined |
| LAST_VALUE | Defined |
| NTH_VALUE | Defined |
| Aggregate OVER | Defined |

Window frame modes: ROWS, RANGE, GROUPS.

## Optimization Rules

### CTE Optimization (13 rules)

| Rule | Description |
|------|-------------|
| cte-inline-single-use | Inline CTEs referenced once |
| cte-materialize-multi-use | Materialize multiply-referenced CTEs |
| cte-filter-pushdown | Push filters into CTE definitions |
| cte-projection-pushdown | Push projections into CTE definitions |
| cte-eliminate-unused | Remove unreferenced CTEs |
| cte-merge-identical | Merge CTEs with identical definitions |
| cte-unnest-simple | Unnest CTEs that are simple scans |
| cte-limit-pushdown | Push LIMIT into single-use CTEs |
| cte-aggregate-pushdown | Push aggregates into single-use CTEs |
| cte-join-to-scan | Simplify CTE self-joins |
| cte-recursive-to-iterative | Unroll bounded recursive CTEs |
| cte-sort-pushdown | Push sorts into single-use CTEs |
| cte-distinct-pushdown | Push DISTINCT into single-use CTEs |

### Window Pushdown (12 rules)

| Rule | Description |
|------|-------------|
| window-filter-pushdown | Push filters below window functions |
| window-partition-filter | Push partition-key filters |
| window-merge-same-spec | Merge windows with same specification |
| window-sort-elimination | Remove redundant sorts |
| window-limit-optimization | Top-N optimization (ROW_NUMBER + filter) |
| window-project-pushdown | Push projections below windows |
| window-aggregate-split | Reuse aggregate results in windows |
| window-reorder-by-cost | Reorder windows to minimize sorts |
| window-frame-optimization | RANGE to ROWS conversion |
| window-remove-unused | Remove unused window computations |
| window-to-aggregate | Convert global window to aggregate |
| window-distinct-elimination | Remove DISTINCT after unique windows |

### Distinct Elimination (11 rules)

| Rule | Description |
|------|-------------|
| distinct-on-key | Remove DISTINCT when key is projected |
| distinct-after-aggregate | Remove DISTINCT after GROUP BY |
| distinct-pushdown-through-union | DISTINCT(UNION ALL) to UNION |
| distinct-over-distinct | Remove redundant nested DISTINCT |
| distinct-over-limit-one | Remove DISTINCT over LIMIT 1 |
| distinct-over-values | Compile-time dedup of constant VALUES |
| distinct-filter-swap | Push filters below DISTINCT |
| distinct-to-group-by | Convert DISTINCT to GROUP BY |
| distinct-over-single-row | Remove DISTINCT over scalar aggregate |
| distinct-intersect-elimination | Remove DISTINCT over INTERSECT/EXCEPT |
| distinct-over-union | Remove DISTINCT over UNION |

## Test Counts

| Package | Test Count |
|---------|------------|
| ra-core (algebra, pattern, etc.) | 64 |
| ra-parser (sql_to_relexpr) | 58 |
| ra-parser (rule validation) | 5 |
| **Total new/updated** | **127** |

## TPC-H Query Coverage

The parser handles TPC-H style queries including:
- Q1: Multi-column GROUP BY with multiple aggregates and ORDER BY
- Q3: Multi-table JOIN with WHERE, GROUP BY, ORDER BY, LIMIT

## Not Yet Supported

| Feature | Reason |
|---------|--------|
| Window OVER clause in SQL parser | Types defined; parser uses `Expr::Function` |
| NATURAL JOIN | Requires schema information |
| Recursive CTEs | Types not yet in RelExpr |
| LATERAL joins | Requires correlated subquery support |
| GROUPING SETS / ROLLUP / CUBE | Not yet in RelExpr |
