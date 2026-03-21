# Rule: "JoinPushThroughJoin"

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/join-push-through-join.rra`

## Metadata

- **ID:** `join-push-through-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join ?type1 ?c1 (join ?type2 ?c2 ?a ?b) ?c)"
    description: "Join above another join"
  - type: "predicate"
    condition: "is_inner_or_cross(?type1) && is_inner_or_cross(?type2)"
    description: "Both joins must be inner or cross"
  - type: "predicate"
    condition: "can_push_through(?c1, ?a, ?b, ?c)"
    description: "Outer join condition must be pushable through inner join"
```


# JoinPushThroughJoin

## Description

Pushes joins through other joins

## Relational Algebra

```algebra
Reorder join operators
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
