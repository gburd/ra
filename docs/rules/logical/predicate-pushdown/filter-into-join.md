# "Filter Into Join"

**Rule ID:** `filter-into-join`
**Category:** logical/predicate-pushdown
**Supported Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
**Tags:** academic, calcite

## Description


# Filter Into Join

## Description

Pushes predicates into join operands

## Relational Algebra

```algebra
$\sigma$(Join) => Join($\sigma$(R), $\sigma$(S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References

