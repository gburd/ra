# Rule: Neo4j Bidirectional BFS for Shortest Path

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/bidirectional-bfs.rra`

## Metadata

- **ID:** `neo4j-bidirectional-bfs`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** bfs, bidirectional, shortest-path, unweighted
- **Authors:** "Neo4j Inc."


# Neo4j Bidirectional BFS for Shortest Path

## Description

Uses bidirectional breadth-first search for finding shortest unweighted paths,
searching simultaneously from both source and target nodes. When the frontiers
meet, the shortest path is found. This is exponentially faster than unidirectional
BFS for long paths.

**When to apply:** `shortestPath()` queries on unweighted graphs where both
source and target are known. Neo4j automatically uses bidirectional BFS when
appropriate.

**Why it works:** Unidirectional BFS explores O(b^d) nodes for depth d and
branching factor b. Bidirectional BFS explores O(2 * b^(d/2)) = O(b^(d/2))
nodes, dramatically reducing search space for long paths.

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("neo4j-bidirectional-bfs";
    "(shortest-path ?start ?end ?rel-type ?max-depth)" =>
    "(bidirectional-bfs ?start ?end ?rel-type ?max-depth)"
    if both-nodes-bound("?start", "?end")
       && !has-weight-property("?rel-type")
       && depth-exceeds("?max-depth", 3)
),
```

## Test Cases

### Positive: Long shortest path between known nodes

```cypher
// Find shortest path between two specific people
MATCH (alice:Person {name: 'Alice'}), (bob:Person {name: 'Bob'})
MATCH path = shortestPath((alice)-[:KNOWS*]-(bob))
RETURN length(path)

// Bidirectional BFS: search from Alice and Bob simultaneously
// Meet in middle, exponentially faster than one direction
// explain shows: BidirectionalShortestPath
```

### Positive: Social network "degrees of separation"

```cypher
// 6 degrees of Kevin Bacon
MATCH (kevin:Actor {name: 'Kevin Bacon'}), (target:Actor {name: 'Tom Hanks'})
MATCH path = shortestPath((kevin)-[:ACTED_WITH*..6]-(target))
RETURN length(path) as degreesOfSeparation

// Bidirectional BFS dramatically reduces search for long paths
```

### Negative: Only source known (unidirectional BFS)

```cypher
// Target not bound - must use unidirectional
MATCH (alice:Person {name: 'Alice'})
MATCH path = shortestPath((alice)-[:KNOWS*..4]-(someone:Person))
WHERE someone.city = 'Seattle'
RETURN path

// Cannot use bidirectional (target unknown)
// Uses unidirectional BFS from Alice
```

## References

**Implementation:**
- Neo4j source: `org.neo4j.cypher.internal.runtime.interpreted.pipes.ShortestPathPipe`
- Bidirectional algorithm: `BidirectionalShortestPath.java`

**Documentation:**
- Neo4j Manual: "Shortest Path Planning"
- https://neo4j.com/docs/cypher-manual/current/planning-and-tuning/

**Papers:**
- Pohl, I., "Bi-directional Search", Machine Intelligence 1971
  - Original bidirectional search algorithm
- Kaindl, H., Kainz, G., "Bidirectional Heuristic Search Reconsidered", 1997
  - Modern analysis of bidirectional algorithms
