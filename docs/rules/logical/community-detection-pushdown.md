# Rule: Community Detection Filter Pushdown

**Category:** logical/graph
**File:** `rules/logical/graph/community-detection-pushdown.rra`

## Metadata

- **ID:** `community-detection-pushdown`
- **Version:** "1.0.0"
- **Databases:** neo4j, tigergraph, memgraph, agensgraph
- **Tags:** logical, graph, community, louvain, label-propagation, pushdown
- **Authors:** "Blondel, Vincent", "Clauset, Aaron"


# Community Detection Filter Pushdown

## Description

Pushes community or partition identifiers from a pre-computed graph
partitioning down to constrain traversal queries. If the query traverses
within a known community (e.g., "find friends of Alice within her
department"), the optimizer restricts edge scans to edges within that
community partition, skipping cross-community edges entirely.

**When to apply**: Traversal queries with constraints that align with
pre-computed community/partition structure of the graph.

## Relational Algebra

```algebra
-- Before: traverse all edges from source
sigma[hops <= 3](Traverse(source, AllEdges))

-- After: restrict to community edges
sigma[hops <= 3](Traverse(source,
    sigma[community = source.community](AllEdges)))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("community-constrained-traversal";
    "(traverse ?source ?edges ?depth ?community_pred)" =>
    "(traverse ?source
        (filter (same-community ?source) ?edges)
        ?depth ?community_pred)"
    if has_community_index("?edges")
    if query_is_community_local("?community_pred")
),
```

## Preconditions

```rust
fn applicable(query: &TraversalQuery, graph: &Graph) -> bool {
    // Graph must have pre-computed community labels
    graph.has_community_labels()
        // Query must have a locality constraint
        && query.has_community_constraint()
        // Community structure must be reasonably balanced
        && graph.max_community_size() < graph.node_count() / 2
}
```

**Restrictions:**
- Requires pre-computed and up-to-date community labels
- Cross-community queries cannot use this optimization
- Community structure must be meaningful (not degenerate)

## Cost Model

```rust
fn estimated_benefit(
    total_edges: f64,
    community_edges: f64,
    traversal_depth: u32,
) -> f64 {
    let full_cost = total_edges * traversal_depth as f64;
    let community_cost = community_edges * traversal_depth as f64;
    full_cost - community_cost
}
```

**Typical benefit**: 10-60% depending on community density.

## Test Cases

```sql
-- Positive: within-community friends query
-- Cypher: MATCH (a:Person {name:'Alice'})-[:KNOWS*1..3]->(b)
-- WHERE b.community = a.community
SELECT * FROM traverse('alice', 3)
WHERE community = (SELECT community FROM vertices WHERE id = 'alice');

-- Negative: cross-community query
SELECT * FROM traverse('alice', 3);
-- No community constraint: full traversal needed
```

## References

- Blondel, V. et al. "Fast Unfolding of Communities in Large Networks" (2008)
- TigerGraph: Graph Partitioning for Query Optimization
