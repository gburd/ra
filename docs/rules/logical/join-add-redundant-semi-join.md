# Rule: "JoinAddRedundantSemiJoin"

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/join-add-redundant-semi-join.rra`

## Metadata

- **ID:** `join-add-redundant-semi-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# JoinAddRedundantSemiJoin

## Description

Adds semi-join to eliminate rows early

## Relational Algebra

```algebra
Join(R, S) => SemiJoin(R, S) + Join
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
