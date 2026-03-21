# "JoinDeriveIsNotNullFilter"

**Rule ID:** `join-derive-is-not-null-filter`
**Category:** logical/predicate-pushdown
**Supported Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
**Tags:** academic, calcite

## Description


# JoinDeriveIsNotNullFilter

## Description

Derives NOT NULL filter from join

## Relational Algebra

```algebra
Join(R, S) => Filter(Join(R, S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
