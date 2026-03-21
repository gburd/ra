# Rule: Recursive CTE to Iterative Unrolling

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/recursive-cte-to-iterative.rra`

## Metadata

- **ID:** `recursive-cte-to-iterative`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** recursive, cte, unrolling, iterative, fixpoint
- **Authors:** "RA Contributors"


# Recursive CTE to Iterative Unrolling

## Description

Transforms a bounded recursive CTE into a finite chain of UNION ALL operations by unrolling the recursion. Only applicable when the maximum recursion depth is known and small enough to make unrolling practical.

**When to apply**: RecursiveCTE has a max_depth <= 20 in its cycle detection config.

**Why it works**: Eliminates the fixpoint iteration runtime overhead. The optimizer can then apply standard optimizations (predicate pushdown, join reordering) to the resulting flat plan.

## Relational Algebra

```algebra
RecursiveCTE[name, base, rec, depth=N](body)
  -> body[name := base UNION ALL sub(rec,base) UNION ALL sub(rec,sub(rec,base)) ... (N times)]
```

## Implementation

```rust
rw!("recursive-cte-to-iterative";
    "(recursive-cte ?name ?base ?rec ?body)" =>
    {
        UnrollRecursiveCTE {
            name: var("?name"),
            base: var("?base"),
            rec: var("?rec"),
            body: var("?body"),
        }
    }
    if bounded_recursion("?name", 20)
),
```

## Test Cases

```sql
-- Bounded: expand hierarchy up to 5 levels
WITH RECURSIVE levels AS (
  SELECT 1 AS level
  UNION ALL
  SELECT level + 1 FROM levels WHERE level < 5
)
SELECT * FROM levels;

-- Unrolled equivalent:
SELECT 1 AS level
UNION ALL SELECT 2
UNION ALL SELECT 3
UNION ALL SELECT 4
UNION ALL SELECT 5;
```

## References

- DuckDB recursive CTE implementation
- Unrolling strategy from "Optimization of Common Table Expressions" (Neumann & Kemper, 2015)
