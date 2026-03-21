# Rule: CTE Filter Pushdown Through Union

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/cte-filter-pushdown-through-union.rra`

## Metadata

- **ID:** `cte-filter-pushdown-through-union`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite
- **Tags:** cte, filter, pushdown, union
- **Authors:** "RA Contributors"


# CTE Filter Pushdown Through Union

## Description

When a CTE body filters the CTE result and the CTE definition is a UNION, push the filter into both branches of the UNION to reduce intermediate rows.

**When to apply**: CTE body has a filter on CTE columns, and the CTE definition is a UNION or UNION ALL.

## Relational Algebra

```algebra
Filter[p](CTE[name, Union[left, right]](Scan[name]))
  -> CTE[name, Union[Filter[p](left), Filter[p](right)]](Scan[name])
  where predicate_references_only(p, output_cols(name))
```

## Implementation

```rust
rw!("cte-filter-pushdown-through-union";
    "(filter ?pred (cte ?name (union ?all ?left ?right) ?body))" =>
    "(cte ?name (union ?all (filter ?pred ?left) (filter ?pred ?right)) ?body)"
    if pred_only_refs_cte_cols("?pred", "?name")
),
```

## Test Cases

### Positive: Filter on union CTE

```sql
WITH combined AS (
    SELECT id, name FROM active_users
    UNION ALL
    SELECT id, name FROM inactive_users
)
SELECT * FROM combined WHERE id > 100;

-- Push id > 100 into both branches
```

## References

- Filter pushdown through UNION in query optimizers
