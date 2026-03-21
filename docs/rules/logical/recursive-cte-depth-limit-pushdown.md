# Rule: Recursive CTE Depth Limit Pushdown

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/recursive-cte-depth-limit-pushdown.rra`

## Metadata

- **ID:** `recursive-cte-depth-limit-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite
- **Tags:** cte, recursive, depth-limit, pushdown
- **Authors:** "RA Contributors"


# Recursive CTE Depth Limit Pushdown

## Description

When the body of a recursive CTE has a LIMIT or a depth filter, push the depth constraint into the recursive step to terminate early instead of computing the full transitive closure.

**When to apply**: Body has LIMIT N or filters on a depth column.

## Relational Algebra

```algebra
Limit[N](RecursiveCTE[name, base, recursive, body])
  -> RecursiveCTE[name, base, recursive, Limit[N](body), max_depth=N]
```

## Implementation

```rust
rw!("recursive-cte-depth-limit-pushdown";
    "(limit ?n 0 (recursive-cte ?name ?base ?rec ?body ?cycle))" =>
    "(recursive-cte ?name ?base ?rec (limit ?n 0 ?body) (cycle-with-depth ?n))"
),
```

## Test Cases

### Positive: Top-N from recursive CTE

```sql
WITH RECURSIVE paths AS (
    SELECT src, dst, 1 AS depth FROM edges WHERE src = 1
    UNION ALL
    SELECT p.src, e.dst, p.depth + 1
    FROM paths p JOIN edges e ON p.dst = e.src
)
SELECT * FROM paths LIMIT 100;

-- Push limit into recursion termination
```

## References

- Iterative deepening in graph search
- Bounded recursive CTE evaluation
