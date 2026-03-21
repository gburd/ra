# Rule: "Property Projection Pruning"

**Category:** logical/projection-pruning
**File:** `rules/database-specific/neo4j/neo4j-property-projection-pruning.rra`

## Metadata

- **ID:** `neo4j-property-projection-pruning`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** database-mining, neo4j, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Property Projection Pruning

## Description

Removes unused property accesses in projections.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(return u.name, u.email, u.phone, u.address)

-- After
(return u.name, u.email)
```

## Preconditions

- phone and address not used in result

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
