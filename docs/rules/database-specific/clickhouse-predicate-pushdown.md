# Rule: "Predicate Pushdown Below Aggregates"

**Category:** logical/predicate-pushdown
**File:** `rules/database-specific/clickhouse/clickhouse-predicate-pushdown.rra`

## Metadata

- **ID:** `clickhouse-predicate-pushdown`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Predicate Pushdown Below Aggregates

## Description

Pushes WHERE predicates below GROUP BY and aggregates when semantically safe.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(group-by [col1] (sum col2) (filter (scan t) (gt col3 10)))

-- After
(group-by [col1] (sum col2) (filter (scan t) (and (gt col3 10) (not-null col1))))
```

## Preconditions

- Predicate on non-aggregated column only

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
