# Rule: "JoinDeriveIsNotNullFilter"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/join-derive-is-not-null-filter.rra`

## Metadata

- **ID:** `join-derive-is-not-null-filter`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner ?cond ?left ?right)"
    description: "Inner join with equi-join condition"
  - type: "predicate"
    condition: "has_equi_condition(?cond)"
    description: "Join condition must have equality on non-nullable columns"
```


# JoinDeriveIsNotNullFilter

## Description

Derives NOT NULL filter from join

## Relational Algebra

```algebra
Join(R, S) => Filter(Join(R, S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
