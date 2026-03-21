# Rule: "MinusToAntiJoin"

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/minus-to-anti-join.rra`

## Metadata

- **ID:** `minus-to-anti-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# MinusToAntiJoin

## Description

Converts MINUS to anti-join

## Relational Algebra

```algebra
R MINUS S => AntiJoin(R, S)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
