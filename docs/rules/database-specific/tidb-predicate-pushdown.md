# Rule: "Predicate Pushdown Through Operators"

**Category:** logical/predicate-pushdown
**File:** `rules/database-specific/tidb/tidb-predicate-pushdown.rra`

## Metadata

- **ID:** `tidb-predicate-pushdown`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Predicate Pushdown Through Operators

## Description

Pushes WHERE predicates closer to the data source.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(filter (join (scan t1) (scan t2)) (and (eq t1.a 5) (eq t2.b 10)))

-- After
(filter (join (filter (scan t1) (eq a 5)) (filter (scan t2) (eq b 10))))
```

## Preconditions

- Predicates only reference one side of join

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
