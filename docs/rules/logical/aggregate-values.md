# Rule: "Aggregate Values"

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-values.rra`

## Metadata

- **ID:** `aggregate-values`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Aggregate Values

## Description

Aggregates over constant values

## Relational Algebra

```algebra
Agg(VALUES) => constant
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
