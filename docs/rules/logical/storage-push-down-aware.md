# Rule: "Storage Push-Down Aware"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/storage-push-down-aware.rra`

## Metadata

- **ID:** `storage-push-down-aware`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, optimization
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (scan ?table))"
    description: "Filter pushable to storage engine"
  - type: "predicate"
    condition: "is_storage_pushable(?pred)"
    description: "Predicate must be expressible in storage engine's filter language"
  - type: "capability"
    database: "current"
    requires: "storage_pushdown"
    description: "Storage engine must support predicate pushdown"
```


# Storage Push-Down Aware

## Description

Respects storage engine push-down capabilities

## Implementation

Add implementation details for Storage Push-Down Aware

## Tests

Add test cases for this optimization

## References

- Paper: Storage Aware Query Optimization
