# "Expand Disjunction For Join"

**Rule ID:** `expand-disjunction-for-join`
**Category:** logical/predicate-pushdown
**Supported Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
**Tags:** academic, calcite

## Description


# Expand Disjunction For Join

## Description

Expands OR conditions in join predicates

## Relational Algebra

```algebra
(A OR B) in join => optimization
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References

