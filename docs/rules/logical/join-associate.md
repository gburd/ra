# Rule: "JoinAssociate"

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/join-associate.rra`

## Metadata

- **ID:** `join-associate`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner ?c2 (join inner ?c1 ?r ?s) ?t)"
    description: "Nested inner join for associative reordering"
  - type: "predicate"
    condition: "is_inner_join(?c1) && is_inner_join(?c2)"
    description: "Both joins must be inner joins"
```


# JoinAssociate

## Description

Reorders joins associatively

## Relational Algebra

```algebra
Join(Join(R, S), T) <=> Join(R, Join(S, T))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
