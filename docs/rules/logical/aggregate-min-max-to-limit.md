# Rule: "AggregateMinMaxToLimit"

**Category:** logical/limit-pushdown
**File:** `rules/logical/limit-pushdown/aggregate-min-max-to-limit.rra`

## Metadata

- **ID:** `aggregate-min-max-to-limit`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# AggregateMinMaxToLimit

## Description

Converts MIN/MAX agg to LIMIT

## Relational Algebra

```algebra
MIN/MAX => LIMIT 1
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
