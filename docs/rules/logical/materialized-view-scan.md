# Rule: "Materialized View Scan"

**Category:** logical/semantic-rewriting
**File:** `rules/logical/semantic-rewriting/materialized-view-scan.rra`

## Metadata

- **ID:** `materialized-view-scan`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Materialized View Scan

## Description

Rewrites scan to use materialized view

## Relational Algebra

```algebra
Scan(T) => Scan(MV) if available
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
