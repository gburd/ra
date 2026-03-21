# Rule: Recursive CTE Semi-Naive Evaluation

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/recursive-cte-semi-naive-eval.rra`

## Metadata

- **ID:** `recursive-cte-semi-naive-eval`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** cte, recursive, semi-naive, datalog
- **Authors:** "RA Contributors"


# Recursive CTE Semi-Naive Evaluation

## Description

Rewrites the recursive step to use semi-naive evaluation: instead of joining against the full accumulated result at each iteration, join only against the delta (new rows from the previous iteration). This avoids redundant computation.

**When to apply**: Recursive CTE with a join between the recursive reference and a base table.

## Relational Algebra

```algebra
RecursiveCTE[name, base, Join[name, table](cond), body]
  -> RecursiveCTE_SemiNaive[name, base, Join[delta_name, table](cond), body]
  where delta_name tracks only new rows per iteration
```

## Implementation

```rust
rw!("recursive-cte-semi-naive-eval";
    "(recursive-cte ?name ?base (join ?type ?cond (scan ?name) ?table) ?body ?cycle)" =>
    "(recursive-cte-semi-naive ?name ?base (join ?type ?cond (scan-delta ?name) ?table) ?body ?cycle)"
),
```

## Test Cases

### Positive: Graph traversal

```sql
WITH RECURSIVE reachable AS (
    SELECT id FROM nodes WHERE id = 1
    UNION ALL
    SELECT e.dst FROM edges e
    JOIN reachable r ON e.src = r.id
)
SELECT * FROM reachable;

-- Use only new nodes from previous iteration
```

## References

- Bancilhon & Ramakrishnan, "An Amateur's Introduction to Recursive Query Processing Strategies"
- Semi-naive evaluation in Datalog systems
