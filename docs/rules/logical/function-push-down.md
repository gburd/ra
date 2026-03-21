# Rule: "Function Push-Down"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/function-push-down.rra`

## Metadata

- **ID:** `function-push-down`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, optimization
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (func ?fname ?args) ?input)"
    description: "Filter using a function predicate"
  - type: "predicate"
    condition: "is_deterministic(func(?fname, ?args))"
    description: "Function must be deterministic"
  - type: "predicate"
    condition: "is_pushable_function(?fname)"
    description: "Function must be safe to push down to storage"
```


# Function Push-Down

## Description

Pushes user-defined functions to storage

## Implementation

Add implementation details for Function Push-Down

## Tests

Add test cases for this optimization

## References

- Paper: Function Push-Down Optimization
