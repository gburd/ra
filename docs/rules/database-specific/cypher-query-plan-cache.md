# Rule: Neo4j Cypher Query Plan Cache

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/cypher-query-plan-cache.rra`

## Metadata

- **ID:** `neo4j-query-plan-cache`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** query-cache, plan-cache, compilation
- **Authors:** "Neo4j Inc."


# Neo4j Cypher Query Plan Cache

## Description

Caches compiled Cypher query plans to avoid re-parsing and re-planning identical
queries. Parameterized queries benefit most, as the plan is compiled once and
reused for different parameter values.

**When to apply:** Queries executed repeatedly with different parameters. Using
parameterized queries ($param syntax) enables plan reuse and eliminates compilation
overhead.

## Test Cases

### Positive: Parameterized query reuses plan

```cypher
// Compiled once, cached, reused for all names
MATCH (p:Person {name: $personName})
RETURN p

// First execution: parse + plan + execute
// Subsequent: cached plan + execute
// 10x faster for short queries
```

### Negative: Literal values prevent caching

```cypher
// Different query text for each name - no cache reuse!
MATCH (p:Person {name: 'Alice'})
RETURN p

MATCH (p:Person {name: 'Bob'})
RETURN p

// Each is treated as different query
// Must compile both
```

## References

**Documentation:**
- Neo4j Manual: "Query Plan Cache"
- https://neo4j.com/docs/cypher-manual/current/query-tuning/query-options/
