# Rule: Recursive CTE Projection Pushdown

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/recursive-cte-projection-pushdown.rra`

## Metadata

- **ID:** `recursive-cte-projection-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite
- **Tags:** recursive, cte, projection, pushdown, column-pruning
- **Authors:** "RA Contributors"


# Recursive CTE Projection Pushdown

## Description

Pushes a projection into both the base case and recursive case of a recursive CTE, reducing the width of the working table across all iterations.

**When to apply**: A Project over a RecursiveCTE where the projected columns are a strict subset of the CTE's output columns.

**Why it works**: Narrower working tables mean less memory per iteration, faster cycle detection hashing, and reduced I/O for intermediate results.

## Relational Algebra

```algebra
Project[cols](RecursiveCTE[name, base, rec](body))
  -> RecursiveCTE[name, Project[cols](base), Project[cols_with_join_keys](rec)](Project[cols](body))
  where cols ⊆ output_columns(base)
```

## Implementation

```rust
rw!("recursive-cte-projection-pushdown";
    "(project ?cols (recursive-cte ?name ?base ?rec ?body))" =>
    "(recursive-cte ?name (project ?cols ?base) (project ?cols ?rec) (project ?cols ?body))"
    if columns_available("?cols", "?base")
),
```

## Test Cases

```sql
-- Before: wide CTE, narrow projection
WITH RECURSIVE ancestors AS (
  SELECT id, name, parent_id, created_at, updated_at FROM people WHERE id = 1
  UNION ALL
  SELECT p.id, p.name, p.parent_id, p.created_at, p.updated_at
  FROM people p JOIN ancestors a ON p.id = a.parent_id
)
SELECT id, name FROM ancestors;

-- After: projection pushed into both members
-- Only id, name, parent_id carried through iterations
```

## References

- Column pruning in recursive CTEs (DuckDB implementation)
