# Rule: Recursive CTE Memoization

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/recursive-cte-memoization.rra`

## Metadata

- **ID:** `recursive-cte-memoization`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, mssql, oracle
- **Tags:** cte, recursive, memoization, performance
- **Authors:** "RA Contributors"


# Recursive CTE Memoization

## Description

Adds a hash-based memoization layer to the recursive step to avoid recomputing results for inputs that have already been processed. Particularly beneficial for graph traversal queries where the same node may be reached via multiple paths.

**When to apply**: Recursive CTE where the recursive step may produce duplicate intermediate results.

## Relational Algebra

```algebra
RecursiveCTE[name, base, recursive, body]
  -> RecursiveCTE[name, base, Distinct(recursive), body]
  where may_produce_duplicates(recursive, name)
```

## Implementation

```rust
rw!("recursive-cte-memoization";
    "(recursive-cte ?name ?base ?recursive ?body ?cycle)" =>
    "(recursive-cte ?name ?base (distinct ?recursive) ?body ?cycle)"
    if may_produce_duplicates("?recursive", "?name")
),
```

## Test Cases

### Positive: Graph reachability with cycles

```sql
WITH RECURSIVE reachable AS (
    SELECT dst FROM edges WHERE src = 1
    UNION ALL
    SELECT e.dst FROM edges e
    JOIN reachable r ON e.src = r.dst
)
SELECT DISTINCT * FROM reachable;

-- Add DISTINCT to recursive step to prune paths
```

## References

- Semi-naive evaluation in Datalog
- Magic sets optimization for recursive queries
