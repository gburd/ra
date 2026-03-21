# Rule: Neo4j Degree-Based Pruning for Variable-Length Paths

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/degree-pruning.rra`

## Metadata

- **ID:** `neo4j-degree-pruning`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** degree, pruning, variable-length, path, traversal, optimization
- **Authors:** "Neo4j Inc."


# Neo4j Degree-Based Pruning for Variable-Length Paths

## Description

Prunes variable-length path expansion by using node degree information to select
the expansion direction (start-to-end or end-to-start) that minimizes
intermediate results. For queries like `(a)-[*1..5]->(b)`, the planner checks
if expanding from `a` forward or from `b` backward produces fewer intermediate
nodes, and selects the cheaper direction.

**When to apply**: Variable-length path queries where one endpoint has
significantly lower degree than the other. The planner estimates the expansion
"fan-out" from each end and chooses the direction with less branching.

**Why it works**: Variable-length expansion is exponential in depth with
branching factor d: O(d^depth) nodes visited. If expanding from the left
has branching factor 100 but from the right has branching factor 5, expanding
right-to-left visits 5^5 = 3,125 nodes instead of 100^5 = 10 billion nodes.

## Relational Algebra

```algebra
-- Before: left-to-right expansion (high branching)
var-length-expand(a, :FOLLOWS, outgoing, 1..5, b)
  where degree(a, :FOLLOWS, out) = 100

-- After: right-to-left expansion (low branching)
var-length-expand(b, :FOLLOWS, incoming, 1..5, a)
  where degree(b, :FOLLOWS, in) = 5

-- Same result set, dramatically fewer nodes visited
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("neo4j-reverse-varlen-expansion";
    "(var-expand ?start ?rel ?dir ?min ?max ?end)" =>
    "(var-expand ?end ?rel (reverse ?dir) ?min ?max ?start)"
    if cheaper_from_end("?start", "?end", "?rel", "?dir")
),

rw!("neo4j-bidirectional-varlen";
    "(var-expand ?start ?rel ?dir ?min ?max ?end)" =>
    "(bidirectional-var-expand ?start ?end ?rel ?dir ?min ?max)"
    if both_endpoints_bound("?start", "?end")
    if depth_exceeds_threshold("?min", "?max", 3)
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    stats.is_variable_length_expansion
        && stats.max_depth > 1
        && (stats.start_degree_estimate / stats.end_degree_estimate > 5.0
            || stats.end_degree_estimate / stats.start_degree_estimate > 5.0)
}
```

**Restrictions:**
- Requires degree estimates for both endpoints (may need index or statistics)
- Reversing direction only works for undirected or when both endpoints are bound
- Path semantics (no repeated nodes) must be preserved regardless of direction
- Bidirectional expansion needs meeting-point detection logic

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let depth = stats.max_depth as f64;
    let start_degree = stats.start_degree_estimate;
    let end_degree = stats.end_degree_estimate;

    let forward_cost = start_degree.powf(depth);
    let backward_cost = end_degree.powf(depth);
    let chosen = forward_cost.min(backward_cost);
    let original = forward_cost;

    if original > chosen {
        (original - chosen) / original
    } else {
        0.0
    }
}
```

**Typical benefit**: 30% to 50x for asymmetric degree distributions.

## Test Cases

### Positive: Reverse expansion for celebrity node

```cypher
// Alice follows 3 people; Bob is followed by 1M people
// Find all paths from Alice to Bob up to 4 hops via FOLLOWS
MATCH path = (alice:Person {name: "Alice"})-[:FOLLOWS*1..4]->(bob:Person {name: "Bob"})
RETURN path

// Forward from Alice: branching ~10 per hop = 10^4 = 10K nodes
// Backward from Bob: in-degree ~5 per hop = 5^4 = 625 nodes
// Planner reverses: expand backward from Bob
```

### Positive: Bidirectional for deep paths

```cypher
// Both endpoints bound, deep path
MATCH (a:Station {id: "NYC"}), (b:Station {id: "LAX"})
MATCH path = (a)-[:CONNECTS*3..8]-(b)
RETURN path

// Bidirectional expansion meets in the middle
// Depth 8: unidirectional visits d^8, bidirectional visits 2*d^4
```

### Negative: Single unbound endpoint

```cypher
// Only start is bound; cannot reverse
MATCH path = (a:Person {name: "Alice"})-[:KNOWS*1..3]->(friend)
RETURN friend

// End is unbound; cannot estimate end degree or expand backward
// Standard forward expansion is the only option
```

## References

**Implementation:**
- Neo4j source: `org.neo4j.cypher.internal.runtime.interpreted.pipes.VarLengthExpandPipe`
- Direction selection: `org.neo4j.cypher.internal.compiler.planner.logical.steps.expand`
- Degree statistics: `org.neo4j.kernel.impl.store.DegreeCounter`

**Documentation:**
- Neo4j Manual: "Variable-length Pattern Matching"
  - https://neo4j.com/docs/cypher-manual/current/patterns/variable-length-patterns/

**Papers:**
- Aberger, C.R., et al., "EmptyHeaded: A Relational Engine for Graph Processing",
  SIGMOD 2017
  - Discusses worst-case optimal join ordering for graph patterns
