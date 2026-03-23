# Rule: Recursive CTE Base Case Filter Pushdown

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/recursive-cte-base-filter-pushdown.rra`

## Metadata

- **ID:** `recursive-cte-base-filter-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite
- **Tags:** recursive, cte, filter, pushdown, base-case
- **Authors:** "RA Contributors"


# Recursive CTE Base Case Filter Pushdown

## Description

Pushes a filter from the body of a recursive CTE into the base case when the filter predicate only references columns produced by the base case and does not depend on the recursive accumulation.

**When to apply**: Filter on the body of a RecursiveCTE where the predicate references only columns available in the base case.

**Why it works**: Reducing the base case cardinality reduces the number of iterations the recursive member performs, since fewer seed rows means fewer recursive expansions.

## Relational Algebra

```algebra
Filter[p](RecursiveCTE[name, base, rec](body))
  -> RecursiveCTE[name, Filter[p](base), rec](body)
  where columns(p) $\subseteq$ columns(base)
    and not references_cte(p, name)
```

## Implementation

```rust
rw!("recursive-cte-base-filter-pushdown";
    "(filter ?pred (recursive-cte ?name ?base ?rec ?body))" =>
    "(recursive-cte ?name (filter ?pred ?base) ?rec ?body)"
    if predicate_references_only("?pred", "?base")
),
```

## Test Cases

```sql
-- Before: filter after CTE
WITH RECURSIVE r AS (
  SELECT id FROM nodes WHERE type = 'root'
  UNION ALL
  SELECT edges.dst FROM edges JOIN r ON edges.src = r.id
)
SELECT * FROM r WHERE id < 100;

-- After: filter pushed into base case (if applicable)
-- This reduces initial seed rows
```

## References

- PostgreSQL recursive CTE optimization (post-9.6)
- SQL standard ISO/IEC 9075-2:2023 Section 7.18
