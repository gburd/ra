# Rule: Neo4j Expand Into vs Expand All

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/expand-into-optimization.rra`

## Metadata

- **ID:** `neo4j-expand-into`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** expand-into, expand-all, traversal-strategy
- **Authors:** "Neo4j Inc."


# Neo4j Expand Into vs Expand All

## Description

Chooses between "Expand(Into)" and "Expand(All)" operators based on whether
the target node is already bound. Expand(Into) checks if a relationship exists
to a known node (O(degree)), while Expand(All) explores all relationships
(O(degree * subsequent work)).

**When to apply:** Queries where both ends of a relationship pattern are known
before the expansion. Expand(Into) is much faster as it only verifies existence
rather than exploring all possibilities.

## Test Cases

### Positive: Both nodes bound (use Expand Into)

```cypher
// Both Alice and Bob are bound before checking relationship
MATCH (alice:Person {name: 'Alice'}), (bob:Person {name: 'Bob'})
MATCH (alice)-[:KNOWS]->(bob)
RETURN alice, bob

// Uses Expand(Into) - just checks if KNOWS relationship exists
// explain shows: Expand(Into)
```

### Negative: Target unbound (use Expand All)

```cypher
// Target unknown - must explore all friends
MATCH (alice:Person {name: 'Alice'})
MATCH (alice)-[:KNOWS]->(friend)
RETURN friend.name

// Uses Expand(All) - explores all KNOWS relationships
// explain shows: Expand(All)
```

## References

**Documentation:**
- Neo4j Manual: "Expand(Into) and Expand(All)"
- https://neo4j.com/docs/cypher-manual/current/planning-and-tuning/operators/
