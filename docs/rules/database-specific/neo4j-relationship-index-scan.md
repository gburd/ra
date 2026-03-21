# Rule: "Relationship Index Scan"

**Category:** physical/relationship-index
**File:** `rules/database-specific/neo4j/neo4j-relationship-index-scan.rra`

## Metadata

- **ID:** `neo4j-relationship-index-scan`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** database-mining, neo4j, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Relationship Index Scan

## Description

Uses relationship indexes for efficient traversal.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(expand (scan users) :FOLLOWS)

-- After
(expand-index (scan users) idx_follows)
```

## Preconditions

- Relationship index on FOLLOWS exists

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
