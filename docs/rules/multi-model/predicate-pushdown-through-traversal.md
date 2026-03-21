# Rule: Predicate Pushdown Through Traversal

**Category:** multi-model/graph
**File:** `rules/multi-model/graph/predicate-pushdown-through-traversal.rra`

## Metadata

- **ID:** `predicate-pushdown-through-traversal`
- **Version:** "1.0.0"
- **Databases:** neo4j, janusgraph, neptune, tigergraph
- **Tags:** graph, predicate, pushdown, traversal, filter
- **SQL Standard:** "cypher:9"
- **Authors:** "RA Contributors"


# Predicate Pushdown Through Traversal

## Description

Pushes node or edge property predicates through graph traversal
operators so they are applied at each traversal step rather than after
the full pattern is matched. This prunes non-qualifying vertices and
edges early, reducing the traversal frontier at each hop.

**When to apply**: A filter on a node or edge property sits above a
multi-hop traversal, and the filtered property belongs to an intermediate
vertex or edge in the path.

**Why it works**: Without pushdown, all paths are enumerated first,
then filtered. With pushdown, each traversal step immediately discards
non-matching neighbors, preventing exponential blowup of intermediate
results.

## Relational Algebra

```algebra
sigma[p(v_i)](traverse(v_0 -[*1..k]-> v_k))
  -> traverse_with_filter(v_0 -[*1..k]-> v_k, i, p)
  where p references properties of intermediate vertex v_i
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("pred-pushdown-traversal";
    "(filter ?pred (var-length-path ?src ?etype ?dst ?min ?max))" =>
    "(var-length-path-filtered ?src ?etype ?dst ?min ?max ?pred)"
    if references_intermediate("?pred", "?etype")
),
```

## Preconditions

```rust
fn applicable(pred: &Expr, traversal: &Traversal) -> bool {
    let refs = pred.property_references();
    refs.iter().any(|r| traversal.involves_intermediate(r))
    && pred.is_deterministic()
}
```

**Restrictions:**
- Predicate must reference intermediate vertices/edges, not only endpoints
- Non-deterministic predicates cannot be pushed
- Aggregation predicates (e.g., path length) cannot be pushed

## Cost Model

```rust
fn estimated_benefit(
    branching_factor: f64,
    selectivity: f64,
    hops: u32,
) -> f64 {
    let unfiltered = branching_factor.powi(hops as i32);
    let filtered =
        (branching_factor * selectivity).powi(hops as i32);
    (unfiltered - filtered) / unfiltered
}
```

**Typical benefit**: 0.7-0.99 for selective predicates on multi-hop paths.

## Test Cases

```cypher
-- Positive: filter on intermediate edge property
MATCH (a:Person)-[r:KNOWS*1..4]->(b:Person)
WHERE ALL(rel IN r WHERE rel.since > 2015)
RETURN a, b;
-- Pushes since > 2015 into each traversal step

-- Negative: predicate on aggregate over full path
MATCH p = (a:Person)-[:KNOWS*1..4]->(b:Person)
WHERE length(p) = 3
RETURN p;
-- Cannot push: length is a path-level aggregate
```

## References

Neo4j: org.neo4j.cypher.internal.runtime.interpreted.pipes.VarLengthExpandPipe
Yakovets et al. "Query Planning for Evaluating SPARQL Property Paths" (SIGMOD 2016)
