# Rule: Neo4j Index-Backed ORDER BY

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/index-backed-order-by.rra`

## Metadata

- **ID:** `neo4j-index-order-by`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** index, order-by, sorting
- **Authors:** "Neo4j Inc."


# Neo4j Index-Backed ORDER BY

## Description

Uses indexes to return results in sorted order without explicit sorting step.
When ORDER BY clause matches an index, Neo4j reads from the index in order,
eliminating the O(n log n) sort operation.

**When to apply:** Queries with ORDER BY on indexed properties. The planner
uses index-backed ordering when the ORDER BY exactly matches an index definition.

## Test Cases

### Positive: ORDER BY on indexed property

```cypher
// Index: CREATE INDEX FOR (p:Person) ON (p.age)
MATCH (p:Person)
WHERE p.city = 'Seattle'
RETURN p.name, p.age
ORDER BY p.age

// Reads from age index in order - no sort needed
// explain shows: OrderedAggregation or no Sort operator
```

### Positive: LIMIT with index-backed order

```cypher
// Top-K query using index
// Index: CREATE INDEX FOR (p:Product) ON (p.price)
MATCH (p:Product)
WHERE p.category = 'electronics'
RETURN p.name, p.price
ORDER BY p.price DESC
LIMIT 10

// Index scan in descending order, stops after 10
// No need to sort all products
```

## References

**Documentation:**
- Neo4j Manual: "Index-backed ORDER BY"
- https://neo4j.com/docs/cypher-manual/current/planning-and-tuning/execution-plans/
