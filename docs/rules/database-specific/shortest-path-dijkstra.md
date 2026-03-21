# Rule: Neo4j Shortest Path with Dijkstra

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/shortest-path-dijkstra.rra`

## Metadata

- **ID:** `neo4j-shortest-path-dijkstra`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** shortest-path, dijkstra, graph-algorithm, weighted
- **Authors:** "Neo4j Inc."


# Neo4j Shortest Path with Dijkstra

## Description

Uses Dijkstra's algorithm for finding shortest weighted paths between nodes
instead of exhaustive path enumeration. When relationship weights are present,
Dijkstra efficiently finds the minimum-weight path by exploring nodes in order
of increasing distance from the source.

**When to apply**: Cypher queries using `shortestPath()` or `allShortestPaths()`
with weighted relationships. The planner automatically selects Dijkstra when
weights are specified via relationship properties.

**Why it works**: Exhaustive path search is O(V^depth) for variable-length patterns.
Dijkstra's algorithm is O((V + E) log V) using a priority queue, dramatically
faster for finding shortest paths in weighted graphs.

## Relational Algebra

```cypher
// Pattern matching with shortest path
MATCH path = shortestPath(
  (start:City {name: 'NYC'})-[:ROUTE*]-(end:City {name: 'LAX'})
)
RETURN path, reduce(d=0, r IN relationships(path) | d + r.distance) AS totalDistance
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("neo4j-use-dijkstra";
    "(shortest-path ?start ?end ?rel-type ?max-depth)" =>
    "(dijkstra-shortest-path ?start ?end ?rel-type ?max-depth
       (weight-property ?rel-type))"
    if has-weight-property("?rel-type")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    stats.is_shortest_path_query
        && stats.has_relationship_weights
        && stats.expected_path_length < 20  // Reasonable path length
}
```

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let vertices = stats.node_count as f64;
    let edges = stats.relationship_count as f64;
    let max_depth = stats.max_path_depth as f64;

    // Exhaustive search cost: O(V^depth)
    let exhaustive_cost = vertices.powf(max_depth);

    // Dijkstra cost: O((V + E) log V)
    let dijkstra_cost = (vertices + edges) * vertices.log2();

    if exhaustive_cost > dijkstra_cost {
        (exhaustive_cost - dijkstra_cost) / exhaustive_cost
    } else {
        0.5
    }
}
```

**Assumptions:**
- Priority queue operations: O(log V)
- Each edge relaxation: O(log V)
- Path length typically << graph diameter
- Weighted relationships available

**Typical benefit:** 50% to 100x for finding shortest paths in large graphs.

## Test Cases

### Positive: Weighted shortest path

```cypher
// Find shortest route by distance between cities
MATCH (start:City {name: 'NYC'}), (end:City {name: 'LAX'})
MATCH path = shortestPath((start)-[:ROUTE*..10]-(end))
RETURN path,
       reduce(d=0, r IN relationships(path) | d + r.distance) AS totalDistance
ORDER BY totalDistance
LIMIT 1

// Neo4j uses Dijkstra with r.distance as weights
// explain shows: ShortestPath(Dijkstra)
```

### Positive: Time-based shortest path

```cypher
// Fastest route considering travel time
MATCH (from:Station {id: 'A'}), (to:Station {id: 'Z'})
MATCH path = shortestPath(
  (from)-[:CONNECTS*..15 {mode: 'subway'}]-(to)
)
RETURN path,
  reduce(t=0, r IN relationships(path) | t + r.travel_time) AS totalTime
```

### Negative: Unweighted shortest path (use BFS)

```cypher
// No weights - BFS is faster than Dijkstra
MATCH path = shortestPath(
  (a:Person {name: 'Alice'})-[:KNOWS*..6]-(b:Person {name: 'Bob'})
)
RETURN path

// Neo4j uses BFS, not Dijkstra (no weights needed)
```

## References

**Implementation:**
- Neo4j source: `org.neo4j.cypher.internal.runtime.interpreted.pipes.ShortestPathPipe`
- Dijkstra implementation: `org.neo4j.graphalgo.impl.path.Dijkstra`

**Documentation:**
- Neo4j Manual: "shortestPath() and allShortestPaths()"
- https://neo4j.com/docs/cypher-manual/current/functions/shortestpath/

**Papers:**
- Dijkstra, E.W., "A Note on Two Problems in Connexion with Graphs", 1959
  - DOI: 10.1007/BF01386390
- Original shortest path algorithm
