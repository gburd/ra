# Rule: "Merge Cardinality Rules"

**Category:** logical/merge-cardinality
**File:** `rules/database-specific/neo4j/neo4j-merge-cardinality.rra`

## Metadata

- **ID:** `neo4j-merge-cardinality`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** database-mining, neo4j, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Merge Cardinality Rules

## Description

Combines adjacent patterns with optimal ordering.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(expand (expand p1 :REL1) :REL2)

-- After
(expand (expand p1 :REL2) :REL1)
```

## Preconditions

- REL2 has better selectivity

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
