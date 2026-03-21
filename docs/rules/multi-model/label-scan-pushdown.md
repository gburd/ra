# Rule: Label Scan Pushdown

**Category:** multi-model/graph
**File:** `rules/multi-model/graph/label-scan-pushdown.rra`

## Metadata

- **ID:** `label-scan-pushdown`
- **Version:** "1.0.0"
- **Databases:** neo4j, janusgraph, neptune, tigergraph
- **Tags:** graph, label, scan, pushdown, filter
- **SQL Standard:** "cypher:9"
- **Authors:** "RA Contributors"


# Label Scan Pushdown

## Description

Pushes node-label filters into the initial node scan operator, replacing
a full graph scan followed by a label filter with a label-specific index
scan. Graph databases maintain per-label indexes that allow scanning
only nodes of a given type.

**When to apply**: A filter on node labels sits above a full node scan,
and a label index exists.

**Why it works**: The label index provides direct access to nodes of a
specific type. Scanning only matching nodes eliminates I/O on nodes of
other types.

## Relational Algebra

```algebra
sigma[label(v) = L](all_nodes())
  -> label_scan(L)
  where label index for L exists
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("label-scan-pushdown";
    "(filter (eq (func label ?v) ?label) (all-nodes))" =>
    "(label-scan ?label)"
),
```

## Preconditions

```rust
fn applicable(label: &str, schema: &GraphSchema) -> bool {
    schema.has_label_index(label)
}
```

**Restrictions:**
- Label index must exist for the filtered label
- Multi-label conjunctions may need intersection of scans
- Dynamic labels (computed at runtime) cannot use this optimization

## Cost Model

```rust
fn estimated_benefit(
    total_nodes: f64,
    label_count: f64,
) -> f64 {
    let full_scan_cost = total_nodes;
    let label_scan_cost = label_count;
    (full_scan_cost - label_scan_cost) / full_scan_cost
}
```

**Typical benefit**: 0.5-0.99 depending on label selectivity.

## Test Cases

```cypher
-- Positive: label filter on person nodes
MATCH (n:Person)
WHERE n.age > 30
RETURN n;
-- Uses label scan on :Person instead of all-node scan

-- Negative: no specific label
MATCH (n)
WHERE n.age > 30
RETURN n;
-- Must scan all nodes
```

## References

Neo4j: org.neo4j.kernel.impl.index.schema.NativeIndexReader
JanusGraph: org.janusgraph.graphdb.database.StandardJanusGraph.query()
Robinson, Webber, Eifrem "Graph Databases" (O'Reilly, 2nd ed. 2015) Chapter 6
