# Rule: ANY Subquery to Semi Join

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/any-to-semi-join.rra`

## Metadata

- **ID:** `any-to-semi-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, mssql, oracle
- **Tags:** subquery, any, semi-join, unnesting
- **Authors:** "RA Contributors"


# ANY Subquery to Semi Join

## Description

Converts `x = ANY (subquery)` into a semi-join. This is semantically equivalent to IN but the ANY syntax may not be optimized as aggressively by all planners.

**When to apply**: ANY/SOME subquery with equality comparison.

## Relational Algebra

```algebra
Filter[col = ANY(subquery)](input)
  -> SemiJoin[col = sub.col](input, subquery)
```

## Implementation

```rust
rw!("any-to-semi-join";
    "(filter (= ?col (any ?subquery)) ?input)" =>
    "(join semi (= ?col ?sub_col) ?input ?subquery)"
    if extract_any_column("?subquery", "?sub_col")
),
```

## Test Cases

### Positive: ANY to semi join

```sql
SELECT * FROM products
WHERE category_id = ANY (SELECT id FROM popular_categories);

-- Convert to semi join
```

## References

- PostgreSQL ANY/IN equivalence
- Subquery flattening in Oracle optimizer
