# Rule: "IntersectToSemiJoin"

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/intersect-to-semi-join.rra`

## Metadata

- **ID:** `intersect-to-semi-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# IntersectToSemiJoin

## Description

Converts INTERSECT to semi-join

## Relational Algebra

```algebra
R INTERSECT S => SemiJoin(R, S)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
