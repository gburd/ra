# Rule: Neo4j Variable-Length Path Expansion Optimization

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/variable-length-path-expansion.rra`

## Metadata

- **ID:** `neo4j-var-length-expansion`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** variable-length, path-expansion, pruning
- **Authors:** "Neo4j Inc."


# Neo4j Variable-Length Path Expansion Optimization

## Description

Optimizes variable-length pattern matching `()-[*min..max]->()` by pruning
paths early based on relationship types, directions, and property filters.
Uses depth-first or breadth-first traversal depending on result requirements.

**When to apply:** Cypher patterns with variable-length relationships like
`[:KNOWS*1..3]`. The planner chooses expansion strategy based on min/max
bounds and filtering requirements.

**Why it works:** Variable-length matching can explore exponential paths.
Early pruning (relationship type filters, property predicates) and appropriate
traversal order (DFS for first result, BFS for shortest) dramatically reduce
search space.

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("neo4j-prune-var-length";
    "(var-length-expand ?start ?rel-type ?min ?max ?predicates)" =>
    "(pruned-expand ?start ?rel-type ?min ?max
       (push-predicates ?predicates))"
    if can-prune-during-expansion("?predicates")
),
```

## Test Cases

### Positive: Filtered variable-length expansion

```cypher
// Find friends-of-friends who like skiing
MATCH (me:Person {name: 'Alice'})-[:KNOWS*1..2]->(friend:Person)
WHERE friend.interests CONTAINS 'skiing'
RETURN DISTINCT friend.name

// Prunes paths where friend.interests doesn't contain 'skiing'
// Avoids exploring all 2-hop paths
```

### Positive: Bounded depth prevents explosion

```cypher
// Max depth limits search space
MATCH path = (a:Person)-[:KNOWS*..4]->(b:Person)
WHERE a.id = 123 AND b.city = 'Seattle'
RETURN path

// Stops at depth 4, doesn't explore entire connected component
```

## References

**Documentation:**
- Neo4j Manual: "Variable Length Patterns"
- https://neo4j.com/docs/cypher-manual/current/syntax/patterns/#cypher-pattern-varlength
