# Rule: "CalcMerge"

**Category:** logical/expression-simplification
**File:** `rules/logical/expression-simplification/calc-merge.rra`

## Metadata

- **ID:** `calc-merge`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# CalcMerge

## Description

Merges consecutive calculations

## Relational Algebra

```algebra
Calc2(Calc1(R)) => Calc_merged(R)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
