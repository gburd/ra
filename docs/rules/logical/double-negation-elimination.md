# Rule: Double Negation Elimination

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/double-negation-elimination.rra`

## Metadata

- **ID:** `double-negation-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, mssql, oracle
- **Tags:** subquery, negation, elimination, simplification
- **Authors:** "RA Contributors"


# Double Negation Elimination

## Description

Eliminates double negation patterns like `NOT NOT EXISTS` or `NOT IN (SELECT ... EXCEPT SELECT ...)`. These patterns arise from query rewrites and can be simplified.

**When to apply**: NOT applied to a NOT EXISTS, or NOT IN applied to a complement set.

## Relational Algebra

```algebra
Filter[NOT NOT EXISTS(sub)](input)
  -> Filter[EXISTS(sub)](input)

Filter[NOT (col NOT IN sub)](input)
  -> Filter[col IN sub](input)
```

## Implementation

```rust
rw!("double-negation-elimination";
    "(filter (not (not ?pred)) ?input)" =>
    "(filter ?pred ?input)"
),
```

## Test Cases

### Positive: NOT NOT EXISTS

```sql
SELECT * FROM users u
WHERE NOT (NOT EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id));

-- Simplify to: WHERE EXISTS (...)
```

## References

- Boolean algebra simplification in query optimization
- De Morgan's laws in predicate pushdown
