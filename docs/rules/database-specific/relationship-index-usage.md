# Rule: Neo4j Relationship Index Usage

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/relationship-index-usage.rra`

## Metadata

- **ID:** `neo4j-relationship-index`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** relationship-index, index, traversal
- **Authors:** "Neo4j Inc."


# Neo4j Relationship Index Usage

## Description

Uses relationship indexes (Neo4j 5.0+) to efficiently find relationships by
property values without scanning all relationships of a type. Particularly
beneficial for high-degree nodes with many outgoing relationships.

**When to apply:** Queries filtering relationships by properties. Relationship
indexes enable O(log R) lookup instead of O(R) scan for all relationships.

## Test Cases

### Positive: Relationship property filter

```cypher
// Index: CREATE INDEX FOR ()-[r:PURCHASED]-() ON (r.amount)
MATCH (c:Customer)-[p:PURCHASED]->(prod:Product)
WHERE p.amount > 1000 AND p.date > date('2024-01-01')
RETURN c.name, prod.name, p.amount

// Uses relationship index on amount to find expensive purchases
```

## References

**Documentation:**
- Neo4j Manual: "Relationship Indexes"
- https://neo4j.com/docs/cypher-manual/current/indexes/search-performance-indexes/managing-indexes/
