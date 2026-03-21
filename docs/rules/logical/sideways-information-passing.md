# Rule: "Sideways Information Passing"

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/sideways-information-passing.rra`

## Metadata

- **ID:** `sideways-information-passing`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, academic
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(recursive ?base ?step ?bindings)"
    description: "Recursive query with bound variables"
  - type: "predicate"
    condition: "has_bound_variables(?bindings)"
    description: "Query must have bound variables for sideways information passing"
  - type: "capability"
    database: "current"
    requires: "recursive_queries"
    description: "Database must support recursive queries"
```


# Sideways Information Passing

## Description

Magic sets optimization for recursive queries

## Relational Algebra

```algebra

```

## Implementation

```
Implement rule transformation for academic optimization
```

## Tests

Add test cases for this rule

## References
- Paper: Magic Sets by Beeri & Ramakrishnan
- DOI: 10.1145/103813.103817
