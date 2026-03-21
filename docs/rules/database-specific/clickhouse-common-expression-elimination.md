# Rule: "Common Expression Elimination"

**Category:** logical/cse
**File:** `rules/database-specific/clickhouse/clickhouse-common-expression-elimination.rra`

## Metadata

- **ID:** `clickhouse-common-expression-elimination`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Common Expression Elimination

## Description

Identifies and deduplicates redundant expressions in queries.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(select (add col1 col2) (add col1 col2))

-- After
(select (as expr_1 expr_1))
```

## Preconditions

- Multiple identical subexpressions present

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
