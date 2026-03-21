# Rule: "Pattern Expansion Reordering"

**Category:** logical/pattern-reorder
**File:** `rules/database-specific/neo4j/neo4j-pattern-reorder.rra`

## Metadata

- **ID:** `neo4j-pattern-reorder`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** database-mining, neo4j, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Pattern Expansion Reordering

## Description

Reorders graph pattern matching to expand from most selective predicates first.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(expand (expand (scan users) :FOLLOWS) :CREATED_POST)

-- After
(expand (expand (filter (scan users) (verified)) :FOLLOWS) :CREATED_POST)
```

## Preconditions

- Selectivity estimated for predicates

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
