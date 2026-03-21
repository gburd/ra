# Rule: Neo4j Property Index Seek

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/property-index-seek.rra`

## Metadata

- **ID:** `neo4j-property-index-seek`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** index, property, seek, lookup, cypher
- **Authors:** "Neo4j Inc."


# Neo4j Property Index Seek

## Description

Replaces label scans with property index seeks when the Cypher query includes
equality or range predicates on indexed node properties. The Cypher planner
selects the most selective index when multiple indexes are available for the
same label.

**When to apply**: Cypher queries with WHERE clauses that filter on indexed
node properties. The planner computes index selectivity from stored statistics
and chooses the index with the lowest estimated cardinality.

**Why it works**: A label scan examines all nodes with a given label, O(N).
An index seek on a selective predicate finds matching nodes in O(log N + k)
where k is the result count. For selective predicates (k << N), this is orders
of magnitude faster.

## Relational Algebra

```algebra
-- Label scan with post-filter
sigma[name = "Alice"](label-scan(:Person))

-- Index seek
index-seek(:Person(name), "Alice")
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("neo4j-label-scan-to-index-seek";
    "(filter (eq (property ?var ?prop) ?val)
       (label-scan ?label ?var))" =>
    "(index-seek ?label ?prop ?val ?var)"
    if has_property_index("?label", "?prop")
),

rw!("neo4j-range-index-seek";
    "(filter (and (>= (property ?var ?prop) ?lo)
                  (<= (property ?var ?prop) ?hi))
       (label-scan ?label ?var))" =>
    "(index-range-seek ?label ?prop ?lo ?hi ?var)"
    if has_property_index("?label", "?prop")
),

rw!("neo4j-composite-index-seek";
    "(filter (and (eq (property ?var ?p1) ?v1)
                  (eq (property ?var ?p2) ?v2))
       (label-scan ?label ?var))" =>
    "(composite-index-seek ?label (?p1 ?p2) (?v1 ?v2) ?var)"
    if has_composite_index("?label", "?p1", "?p2")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    stats.has_property_index
        && stats.predicate_selectivity < 0.3
        && stats.label_node_count > 100
}
```

**Restrictions:**
- Only B-tree indexes support range seeks; full-text indexes use different operators
- Composite indexes require predicates on all leading columns for seek
- CONTAINS and ENDS WITH predicates cannot use standard B-tree indexes
- Index seek requires exact type match (string index won't match integer query)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let label_count = stats.label_node_count as f64;
    let selectivity = stats.predicate_selectivity;
    let result_count = label_count * selectivity;

    let label_scan_cost = label_count * 0.001;
    let index_seek_cost = label_count.log2() * 0.00001
        + result_count * 0.001;

    if label_scan_cost > index_seek_cost {
        (label_scan_cost - index_seek_cost) / label_scan_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 30% to 100x for selective lookups on large label sets.

## Test Cases

### Positive: Equality index seek

```cypher
// Index on :Person(email)
CREATE INDEX FOR (p:Person) ON (p.email)

// Query uses index seek instead of label scan
MATCH (p:Person {email: "alice@example.com"})
RETURN p

// EXPLAIN shows: NodeIndexSeek (estimated rows: 1)
// Without index: NodeByLabelScan + Filter (scans all Person nodes)
```

### Positive: Composite index seek

```cypher
// Composite index on :Order(status, date)
CREATE INDEX FOR (o:Order) ON (o.status, o.date)

// Both predicates use composite index
MATCH (o:Order)
WHERE o.status = "shipped" AND o.date >= date("2024-01-01")
RETURN o

// EXPLAIN shows: NodeIndexSeek with composite bounds
```

### Negative: Non-indexed property

```cypher
// No index on :Person(age)
MATCH (p:Person)
WHERE p.age > 21
RETURN p

// Must use NodeByLabelScan + Filter
// All Person nodes scanned and filtered
```

## References

**Implementation:**
- Neo4j source: `org.neo4j.cypher.internal.compiler.planner.logical.steps.IndexSeekLeafPlanner`
- Index selection: `org.neo4j.cypher.internal.compiler.planner.logical.IndexCompatiblePredicatesProviderContext`
- Cost model: `org.neo4j.cypher.internal.compiler.planner.logical.cardinality`

**Documentation:**
- Neo4j Manual: "Indexes for Search Performance"
  - https://neo4j.com/docs/cypher-manual/current/indexes-for-search-performance/

**Papers:**
- Robinson, I., Webber, J., Eifrem, E., "Graph Databases", O'Reilly 2015
