# Rule: "Join Union Transpose"

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/join-union-transpose.rra`

## Metadata

- **ID:** `join-union-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join ?type ?cond (union ?r1 ?r2) ?s)"
    description: "Join with a union as one input"
  - type: "predicate"
    condition: "is_inner_or_cross(?type)"
    description: "Join must be inner or cross"
```


# Join Union Transpose

## Description

Transposes join and union

## Relational Algebra

```algebra
Join(Union(R1, R2), S) => Union(Join(R1, S), Join(R2, S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
