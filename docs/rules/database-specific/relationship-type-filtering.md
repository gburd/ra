# Rule: Neo4j Relationship Type Filtering

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/relationship-type-filtering.rra`

## Metadata

- **ID:** `neo4j-rel-type-filter`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** relationship-type, filtering, traversal
- **Authors:** "Neo4j Inc."


# Neo4j Relationship Type Filtering

## Description

Filters relationships by type during traversal instead of after, reducing the
number of paths explored. Relationship type filters are pushed down into the
expansion operators for efficiency.

**When to apply:** Patterns specifying relationship types like `-[:KNOWS]->` or
`-[:KNOWS|:FRIENDS]->`. Type filtering happens during traversal, not post-processing.

## Test Cases

### Positive: Specific relationship type

```cypher
// Only traverse KNOWS relationships
MATCH (a:Person)-[:KNOWS*1..3]->(b:Person)
WHERE a.name = 'Alice'
RETURN b.name

// Expansion only follows KNOWS edges
// Ignores other relationship types during traversal
```

### Positive: Multiple relationship types (union)

```cypher
// Traverse KNOWS or FRIENDS relationships
MATCH (a:Person)-[:KNOWS|:FRIENDS*1..2]->(b:Person)
WHERE a.name = 'Alice'
RETURN DISTINCT b.name

// Expansion follows both KNOWS and FRIENDS
// Type filter applied during traversal
```

## References

**Documentation:**
- Neo4j Manual: "Relationship Type Filtering"
- https://neo4j.com/docs/cypher-manual/current/syntax/patterns/
