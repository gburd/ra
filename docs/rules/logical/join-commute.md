# Rule: "JoinCommute"

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/join-commute.rra`

## Metadata

- **ID:** `join-commute`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner ?cond ?left ?right)"
    description: "Inner join with two inputs"
  - type: "predicate"
    condition: "is_inner_or_cross(?type)"
    description: "Only inner and cross joins are commutative"
```


# JoinCommute

## Description

Commutes join operands for optimization

## Relational Algebra

```algebra
Join(R, S) => Join(S, R)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
