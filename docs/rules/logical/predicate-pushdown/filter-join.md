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
σ(Join(R, S)) => Join(σ(R), S)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
