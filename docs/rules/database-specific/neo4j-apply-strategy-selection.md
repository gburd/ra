# Rule: "Apply Strategy Selection (Eager vs Lazy)"

**Category:** physical/apply-strategy
**File:** `rules/database-specific/neo4j/neo4j-apply-strategy-selection.rra`

## Metadata

- **ID:** `neo4j-apply-strategy-selection`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** database-mining, neo4j, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Apply Strategy Selection (Eager vs Lazy)

## Description

Chooses between eager and lazy evaluation strategies.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(apply (scan users) :FOLLOWS)

-- After
(lazy-apply (scan users) :FOLLOWS)
```

## Preconditions

- Memory constraints suggest lazy evaluation

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
