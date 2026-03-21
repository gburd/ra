# Rule: "Predicate Pushdown Before Expansion"

**Category:** logical/predicate-pushdown
**File:** `rules/database-specific/neo4j/neo4j-predicate-pushdown.rra`

## Metadata

- **ID:** `neo4j-predicate-pushdown`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** database-mining, neo4j, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Predicate Pushdown Before Expansion

## Description

Pushes WHERE clauses before graph pattern expansion.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(expand (filter (scan users) (age > 18)) :FOLLOWS)

-- After
(filter (expand (scan users) :FOLLOWS) (age > 18))
```

## Preconditions

- Predicate independent of expansion

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
