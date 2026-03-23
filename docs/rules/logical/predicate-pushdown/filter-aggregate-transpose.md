# "FilterAggregateTranspose"

**Rule ID:** `filter-aggregate-transpose`
**Category:** logical/predicate-pushdown
**Supported Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
**Tags:** academic, calcite

## Description


# FilterAggregateTranspose

## Description

Transposes filter and aggregate

## Relational Algebra

```algebra
$\sigma$(Agg(R)) => Agg($\sigma$(R))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
