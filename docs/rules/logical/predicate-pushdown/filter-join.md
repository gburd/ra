# "FilterJoin"

**Rule ID:** `filter-join`
**Category:** logical/predicate-pushdown
**Supported Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
**Tags:** academic, calcite

## Description


# FilterJoin

## Description

Pushes filter through join

## Relational Algebra

```algebra
$\sigma$(Join(R, S)) => Join($\sigma$(R), S)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
