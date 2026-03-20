# SQL Feature Coverage

Status of SQL features supported by the `ra-parser` SQL-to-relational-algebra converter.

## Query Structure

| Feature | Status | Notes |
|---------|--------|-------|
| SELECT | Supported | Column list, expressions |
| SELECT DISTINCT | Supported | DISTINCT and DISTINCT ON |
| FROM single table | Supported | With optional alias |
| FROM subquery | Supported | Derived tables |
| FROM multiple tables | Supported | Implicit cross join |
| WHERE | Supported | Arbitrary predicates |
| GROUP BY | Supported | Column and expression grouping |
| HAVING | Supported | Post-aggregate filter |
| ORDER BY | Supported | ASC/DESC, NULLS FIRST/LAST |
| LIMIT | Supported | Integer literal |
| OFFSET | Supported | Integer literal |
| WITH/CTE | Supported | Non-recursive CTEs |
| WITH RECURSIVE | Not supported | Planned |

## Join Types

| Feature | Status | Notes |
|---------|--------|-------|
| INNER JOIN | Supported | ON clause |
| LEFT OUTER JOIN | Supported | ON clause |
| RIGHT OUTER JOIN | Supported | ON clause |
| FULL OUTER JOIN | Supported | ON clause |
| CROSS JOIN | Supported | |
| JOIN ... USING | Supported | Column list |
| NATURAL JOIN | Not supported | |
| SEMI JOIN | Algebra only | Not parseable from SQL |
| ANTI JOIN | Algebra only | Not parseable from SQL |

## Set Operations

| Feature | Status | Notes |
|---------|--------|-------|
| UNION | Supported | |
| UNION ALL | Supported | |
| INTERSECT | Supported | |
| INTERSECT ALL | Supported | |
| EXCEPT | Supported | |
| EXCEPT ALL | Supported | |

## Aggregate Functions

| Function | Status |
|----------|--------|
| COUNT | Supported |
| SUM | Supported |
| AVG | Supported |
| MIN | Supported |
| MAX | Supported |
| STDDEV | Supported |
| STDDEV_POP | Supported |
| STDDEV_SAMP | Supported |
| VARIANCE | Supported |
| VAR_POP | Supported |
| VAR_SAMP | Supported |
| STRING_AGG | Supported |
| GROUP_CONCAT | Supported (alias for STRING_AGG) |
| ARRAY_AGG | Supported |
| COUNT(DISTINCT x) | Supported |

## Window Functions

| Function | Status |
|----------|--------|
| ROW_NUMBER() | Supported |
| RANK() | Supported |
| DENSE_RANK() | Supported |
| PERCENT_RANK() | Supported |
| NTILE(n) | Supported |
| LAG() | Supported |
| LEAD() | Supported |
| FIRST_VALUE() | Supported |
| LAST_VALUE() | Supported |
| NTH_VALUE() | Supported |
| SUM() OVER | Supported |
| AVG() OVER | Supported |
| COUNT() OVER | Supported |
| MIN() OVER | Supported |
| MAX() OVER | Supported |

### Window Specifications

| Feature | Status |
|---------|--------|
| PARTITION BY | Supported |
| ORDER BY | Supported |
| ROWS frame | Supported |
| RANGE frame | Supported |
| GROUPS frame | Supported |
| UNBOUNDED PRECEDING | Supported |
| N PRECEDING | Supported |
| CURRENT ROW | Supported |
| N FOLLOWING | Supported |
| UNBOUNDED FOLLOWING | Supported |
| Named windows | Not supported |

## Expressions

| Feature | Status |
|---------|--------|
| Column references | Supported |
| Qualified columns (t.col) | Supported |
| Numeric literals | Supported |
| String literals | Supported |
| Boolean literals | Supported |
| NULL | Supported |
| Arithmetic (+, -, *, /) | Supported |
| Comparisons (=, <>, <, >, <=, >=) | Supported |
| AND, OR, NOT | Supported |
| IS NULL, IS NOT NULL | Supported |
| BETWEEN | Supported |
| CAST | Supported |
| CASE WHEN | Supported |
| IN (subquery) | Supported |
| EXISTS (subquery) | Supported |
| Scalar subquery | Partial |

## Optimization Rules

### CTE Optimization (5 rules)

- `cte-inlining` - Inline single-use CTEs
- `cte-materialization` - Materialize multi-use CTEs
- `cte-predicate-pushdown` - Push predicates into CTE definitions
- `cte-projection-pushdown` - Push projections into CTE definitions
- `cte-merge-duplicate` - Merge CTEs with identical definitions

### Window Function Optimization (5 rules)

- `window-function-pushdown` - Push filters below window functions
- `window-partition-elimination` - Remove redundant partition columns
- `window-projection-pushdown` - Push projections below windows
- `window-merge` - Merge windows with same specification
- `window-to-aggregate` - Convert trivial windows to aggregates

### Distinct Elimination (5 rules)

- `distinct-on-unique-key` - Remove DISTINCT when unique key present
- `distinct-after-group-by` - Remove DISTINCT after GROUP BY
- `distinct-pushdown-through-union` - Push DISTINCT into UNION branches
- `distinct-to-limit-one` - Simplify DISTINCT on scalar/single-row
- `distinct-filter-reorder` - Reorder filter and DISTINCT
