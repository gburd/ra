# "Filter Transpose Aggregate"

**Rule ID:** `filter-into-aggregate`
**Category:** logical/predicate-pushdown
**Supported Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
**Tags:** academic, calcite

## Description


# Filter Transpose Aggregate

## Description

Transposes filters through aggregation

## Relational Algebra

```algebra
σ(Agg(R)) => Agg(σ(R))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References

