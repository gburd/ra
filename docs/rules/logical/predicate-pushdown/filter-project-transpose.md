# "FilterProjectTranspose"

**Rule ID:** `filter-project-transpose`
**Category:** logical/predicate-pushdown
**Supported Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
**Tags:** academic, calcite

## Description


# FilterProjectTranspose

## Description

Transposes filter and projection

## Relational Algebra

```algebra
σ(π(R)) => π(σ(R))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
