# Rule: "Label Scan Optimization"

**Category:** physical/label-scan
**File:** `rules/database-specific/neo4j/neo4j-label-scan-optimization.rra`

## Metadata

- **ID:** `neo4j-label-scan-optimization`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** database-mining, neo4j, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Label Scan Optimization

## Description

Optimizes label scans in Cypher queries.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(scan users WHERE age > 18)

-- After
(label-scan users (age > 18))
```

## Preconditions

- Users label indexed

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
