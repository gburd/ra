# Rule: Vertex-Centric Index Selection

**Category:** multi-model/graph
**File:** `rules/multi-model/graph/vertex-centric-index.rra`

## Metadata

- **ID:** `vertex-centric-index`
- **Version:** "1.0.0"
- **Databases:** janusgraph, tigergraph, neptune
- **Tags:** graph, index, vertex-centric, adjacency
- **SQL Standard:** "gremlin:3"
- **Authors:** "RA Contributors"


# Vertex-Centric Index Selection

## Description

Selects vertex-centric indexes to accelerate filtered edge lookups on
supernode vertices. When a vertex has thousands of edges and the query
filters by edge property (e.g., weight, timestamp), a vertex-centric
index narrows the scan to matching edges without iterating over all
adjacency-list entries.

**When to apply**: A traversal step includes a predicate on edge
properties and a vertex-centric index exists for that property.

**Why it works**: Without the index, filtering edges from a supernode
requires scanning all O(degree) edges. With the index, only matching
edges are visited, reducing cost to O(log(degree) + matches).

## Relational Algebra

```algebra
sigma[e.prop op value](expand(v, edge_type))
  -> vertex_centric_scan(v, edge_type, prop, op, value)
  where vertex_centric_index(edge_type, prop) exists
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("vertex-centric-index";
    "(filter ?pred (expand ?vertex ?etype))" =>
    "(vc-index-scan ?vertex ?etype ?pred)"
    if has_vc_index("?etype", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    edge_type: &str,
    pred: &Expr,
    schema: &GraphSchema,
) -> bool {
    let prop = pred.edge_property_ref();
    prop.map_or(false, |p| {
        schema.has_vertex_centric_index(edge_type, &p)
    })
}
```

**Restrictions:**
- Index must exist on the filtered edge property
- Only range and equality predicates can use the index
- Multiple predicates require a composite vertex-centric index

## Cost Model

```rust
fn estimated_benefit(
    avg_degree: f64,
    selectivity: f64,
) -> f64 {
    let scan_cost = avg_degree;
    let index_cost = avg_degree.ln() + avg_degree * selectivity;
    (scan_cost - index_cost) / scan_cost
}
```

**Typical benefit**: 0.5-0.99 on supernodes (degree > 1000) with selective predicates.

## Test Cases

```gremlin
// Positive: filter on edge property with index
g.V(alice).outE('knows').has('since', gt(2020)).inV()
// Optimizer uses vertex-centric index on 'since' property

// Negative: no index on the filtered property
g.V(alice).outE('knows').has('weight', gt(0.5)).inV()
// Falls back to full adjacency scan + filter
```

## References

JanusGraph: org.janusgraph.graphdb.query.vertex.BasicVertexCentricQueryBuilder
TigerGraph: docs.tigergraph.com/dev/restpp-api/built-in-endpoints
Angles "A Comparison of Current Graph Database Models" (ICDE 2012)
