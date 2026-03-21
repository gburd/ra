# Rule: "IntersectToDistinct"

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/intersect-to-distinct.rra`

## Metadata

- **ID:** `intersect-to-distinct`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# IntersectToDistinct

## Description

Converts INTERSECT to distinct

## Relational Algebra

```algebra
R INTERSECT S => optimized
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
