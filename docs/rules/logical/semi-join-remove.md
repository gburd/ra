# Rule: "SemiJoinRemove"

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/semi-join-remove.rra`

## Metadata

- **ID:** `semi-join-remove`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# SemiJoinRemove

## Description

Removes unnecessary semi-join

## Relational Algebra

```algebra
SemiJoin(R, S) => R if S cols not used
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
