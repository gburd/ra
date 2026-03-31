# RFC 0093: SQL Property Graph Queries (SQL/PGQ)

- Start Date: 2026-03-27
- Author: Ra Development Team
- Status: Draft
- Target: PostgreSQL 17+ (SQL:2023 standard)
- Tracking Issue: TBD

## Summary

Implement support for SQL/PGQ (SQL Property Graph Queries), a new SQL standard feature being added to PostgreSQL v17+ that enables graph pattern matching using declarative SQL syntax. SQL/PGQ provides MATCH clauses for path patterns, property access, and graph traversal without requiring a separate graph database.

## Motivation

Graph queries are increasingly important for:
- Social network analysis (friends of friends, influencer detection)
- Fraud detection (transaction chains, account relationships)
- Supply chain optimization (dependency graphs, route planning)
- Knowledge graphs (entity relationships, semantic queries)
- Recommendation systems (user similarity, content graphs)

**Current limitations:**
- Recursive CTEs are verbose and hard to optimize
- Traditional SQL lacks concise graph pattern syntax
- Graph databases require separate systems and ETL
- No standard way to express graph traversals

**SQL/PGQ benefits:**
- Standard SQL syntax (ISO SQL:2023)
- Native integration with relational data
- Declarative pattern matching
- Optimizable graph traversals
- No separate graph database needed

## Guide-level explanation

### Basic Graph Pattern Matching

SQL/PGQ introduces the `MATCH` clause for graph patterns:

```sql
-- Find friends of friends
SELECT p.name, f.name, fof.name
FROM GRAPH_TABLE (
  social_graph
  MATCH (p:Person)-[:KNOWS]-&gt;(f:Person)-[:KNOWS]-&gt;(fof:Person)
  COLUMNS (p.name AS person, f.name AS friend, fof.name AS friend_of_friend)
) AS t;
```

### Graph Schema Definition

Property graphs are defined over existing tables:

```sql
-- Define a property graph from relational tables
CREATE PROPERTY GRAPH social_graph
  VERTEX TABLES (
    users AS Person
      KEY (user_id)
      PROPERTIES (name, age, city)
  )
  EDGE TABLES (
    friendships AS KNOWS
      SOURCE KEY (user_id1) REFERENCES Person
      DESTINATION KEY (user_id2) REFERENCES Person
      PROPERTIES (since, strength)
  );
```

### Path Patterns

**Fixed-length paths:**
```sql
-- Exactly 2 hops
MATCH (a)-[]-()-[]-&gt;(b)
```

**Variable-length paths:**
```sql
-- 1 to 3 hops
MATCH (a)-[]-{1,3}(b)

-- Any length (shortest path)
MATCH SHORTEST (a)-[]-*-&gt;(b)
```

**Named paths:**
```sql
-- Capture entire path
MATCH p = (a)-[]-*-&gt;(b)
RETURN nodes(p), edges(p), length(p)
```

### Property Filtering

```sql
-- Filter on vertex and edge properties
MATCH (p:Person {city: 'NYC'})-[k:KNOWS WHERE k.strength &gt; 0.8]-&gt;(f:Person)
WHERE f.age &gt; 25
RETURN p.name, f.name, k.since
```

### Aggregations and Analytics

```sql
-- Count connections by city
SELECT city, COUNT(*) as connections
FROM GRAPH_TABLE (
  social_graph
  MATCH (p:Person)-[:KNOWS]-&gt;(f:Person)
  WHERE p.city = f.city
  COLUMNS (p.city AS city)
)
GROUP BY city;
```

### Path Quantifiers

```sql
-- All paths (multiple results per source)
MATCH ALL (a)-[]-*-&gt;(b)

-- Any path (one arbitrary path)
MATCH ANY (a)-[]-*-&gt;(b)

-- Shortest path (minimum hops)
MATCH SHORTEST (a)-[]-*-&gt;(b)

-- All shortest paths (all paths with minimum hops)
MATCH ALL SHORTEST (a)-[]-*-&gt;(b)
```

## Reference-level explanation

### SQL/PGQ Syntax Extensions

**Grammar additions:**

```
graph_table_reference ::=
  GRAPH_TABLE (
    graph_name
    MATCH graph_pattern
    [WHERE search_condition]
    COLUMNS (column_definition_list)
  )

graph_pattern ::=
  path_pattern [, path_pattern]*

path_pattern ::=
  [path_mode] [path_variable =] path_element_chain

path_mode ::=
  WALK | TRAIL | SIMPLE | ACYCLIC

path_element_chain ::=
  vertex_pattern [edge_pattern vertex_pattern]*

vertex_pattern ::=
  '(' [vertex_variable] [':' label] [WHERE predicate] [property_spec] ')'

edge_pattern ::=
  [-|-&gt;|&lt;-] '[' [edge_variable] [':' label] [WHERE predicate] ']' [quantifier]

quantifier ::=
  '*' | '+' | '{' m ',' n '}' | '{' m ',' '}' | '{' ',' n '}'
```

**Path modes:**
- `WALK`: Allow repeated vertices and edges (default)
- `TRAIL`: No repeated edges, vertices may repeat
- `SIMPLE`: No repeated vertices (implies TRAIL)
- `ACYCLIC`: No cycles in the path

**Quantifiers:**
- `*`: Zero or more (Kleene star)
- `+`: One or more
- `{m,n}`: Between m and n repetitions
- `{m,}`: At least m repetitions
- `{,n}`: At most n repetitions

### Relational Algebra Mapping

SQL/PGQ constructs map to relational operators:

**1. Vertex pattern → Scan/Filter:**
```
(p:Person {city: 'NYC'})
→ σ[label='Person' ∧ city='NYC'](vertex_table)
```

**2. Edge pattern → Join:**
```
(a)-[k:KNOWS]-&gt;(b)
→ a ⋈[a.id = k.src ∧ k.label='KNOWS'] k ⋈[k.dst = b.id] b
```

**3. Path quantifiers → Recursive operators:**
```
(a)-[]-*-&gt;(b)
→ Fix(λX. a ∪ (X ⋈ edge_table ⋈ vertex_table))
```

**4. Shortest path → Dijkstra/BFS:**
```
MATCH SHORTEST (a)-[e]-*-&gt;(b)
→ ShortestPath(a, b, cost=length(e))
```

### Optimization Strategies

#### 1. Join Order Optimization

Graph patterns are joins - use Ra's join ordering:

```sql
-- Pattern: (a)-[]-&gt;(b)-[]-&gt;(c)&lt;-[]-(d)
-- Join order depends on selectivity:
-- If b is highly selective: b → a, b → c, c → d
-- If d is highly selective: d → c, c → b, b → a
```

**Cost model:**
```
cost(pattern) = Σ (vertex_scan_cost + edge_join_cost)
edge_join_cost = |left| × |edge_table| × selectivity
```

#### 2. Index Selection

Property graphs benefit from specialized indexes:

- **Adjacency list indexes**: `(src, dst)` on edge tables
- **Reverse adjacency**: `(dst, src)` for backward traversal
- **Label indexes**: B-tree on label columns
- **Property indexes**: For filtered vertex/edge properties

**Index recommendation:**
```sql
-- For pattern (a)-[:KNOWS]-&gt;(b)
CREATE INDEX ON friendships(user_id1, user_id2)
  WHERE label = 'KNOWS';
```

#### 3. Path Quantifier Optimization

**Fixed quantifier `{2,2}` (exactly 2 hops):**
```sql
-- Expand to explicit joins (faster)
(a)-[]-()-[]-&gt;(b)
→ a ⋈ e1 ⋈ mid ⋈ e2 ⋈ b
```

**Small quantifier `{1,3}` (1-3 hops):**
```sql
-- Bounded recursion → UNION of explicit joins
UNION(
  (a)-[]-&gt;(b),           -- 1 hop
  (a)-[]-()-[]-&gt;(b),      -- 2 hops
  (a)-[]-()-[]-()-[]-&gt;(b) -- 3 hops
)
```

**Unbounded `*` or `+` (any length):**
```sql
-- Use recursive CTE with cycle detection
WITH RECURSIVE paths AS (
  SELECT src, dst, 1 AS depth, ARRAY[src] AS visited
  FROM edges WHERE src = a_id
  UNION ALL
  SELECT p.src, e.dst, p.depth + 1, p.visited || e.src
  FROM paths p JOIN edges e ON p.dst = e.src
  WHERE e.src != ALL(p.visited)  -- Cycle detection
)
SELECT * FROM paths WHERE dst = b_id;
```

#### 4. Shortest Path Algorithms

**Single-source shortest path (SSSP):**
- Use Dijkstra's algorithm with priority queue
- Materialize frontier in temp table
- Early termination when target reached

**All-pairs shortest path (APSP):**
- Use Floyd-Warshall for dense graphs
- Use Johnson's algorithm for sparse graphs
- Consider materialized path cache

**Optimization heuristics:**
```
IF graph_size &lt; 1000 AND pattern = SHORTEST THEN
  use_dijkstra()
ELSE IF quantifier.bounded AND quantifier.max &lt;= 5 THEN
  expand_to_unions()
ELSE
  use_recursive_cte()
```

#### 5. Materialized Graph Views

For frequently queried patterns, materialize paths:

```sql
-- Materialize 2-hop friends
CREATE MATERIALIZED VIEW friends_of_friends AS
SELECT p.user_id, f.user_id AS friend_id, fof.user_id AS fof_id
FROM users p
JOIN friendships f1 ON p.user_id = f1.user_id1
JOIN users f ON f1.user_id2 = f.user_id
JOIN friendships f2 ON f.user_id = f2.user_id1
JOIN users fof ON f2.user_id2 = fof.user_id;
```

### Rewrite Rules

**Rule 1: Push filters into vertex patterns**
```
Before:
MATCH (a)-[]-&gt;(b) WHERE a.city = 'NYC'

After:
MATCH (a:Person {city: 'NYC'})-[]-&gt;(b)
```

**Rule 2: Merge adjacent vertex scans**
```
Before:
MATCH (a) WHERE a.age &gt; 25
MATCH (a)-[]-&gt;(b)

After:
MATCH (a {age &gt; 25})-[]-&gt;(b)
```

**Rule 3: Convert bounded quantifiers to joins**
```
Before:
MATCH (a)-[]-{2}(b)

After:
MATCH (a)-[]-()-[]-&gt;(b)
```

**Rule 4: Bidirectional search for SHORTEST**
```
Before:
MATCH SHORTEST (a)-[]-*-&gt;(b)

After:
-- Search from both ends, meet in middle
WITH forward AS (BFS from a),
     backward AS (BFS from b)
SELECT * WHERE forward.node = backward.node
```

**Rule 5: Index-based path filtering**
```
Before:
MATCH (a)-[:KNOWS]-&gt;(b) WHERE a.user_id = 123

After:
-- Use index scan on (src, label)
IndexScan(edges, (src=123, label='KNOWS')) ⋈ b
```

### PostgreSQL Implementation Details

PostgreSQL v17+ implements SQL/PGQ via:

1. **Graph catalog tables:**
   - `pg_property_graph`: Graph definitions
   - `pg_graph_vertex`: Vertex table mappings
   - `pg_graph_edge`: Edge table mappings

2. **Execution strategy:**
   - Parser: Extend grammar with GRAPH_TABLE, MATCH
   - Planner: Convert patterns to join trees
   - Executor: Use standard join/scan nodes

3. **Storage:**
   - No special storage format (uses existing tables)
   - Adjacency lists via foreign keys
   - Optional label columns for vertex/edge types

4. **Performance:**
   - Specialized graph indexes (GIN for labels, adjacency)
   - Query plan caching for repeated patterns
   - Parallel execution for independent path searches

## Drawbacks

**1. Complexity:** Graph queries add significant parser/planner complexity.

**2. Performance unpredictability:** Graph traversals can have exponential complexity.

**3. Limited analytics:** No built-in graph algorithms (PageRank, community detection, centrality).

**4. Storage overhead:** Property graphs require additional metadata tables.

**5. Vendor fragmentation:** SQL/PGQ adoption varies across databases.

## Rationale and alternatives

### Why SQL/PGQ over recursive CTEs?

**Recursive CTE:**
```sql
-- Verbose and hard to optimize
WITH RECURSIVE paths(src, dst, depth) AS (
  SELECT user_id1, user_id2, 1 FROM friendships WHERE user_id1 = 123
  UNION ALL
  SELECT p.src, f.user_id2, p.depth + 1
  FROM paths p JOIN friendships f ON p.dst = f.user_id1
  WHERE p.depth &lt; 3
)
SELECT * FROM paths;
```

**SQL/PGQ:**
```sql
-- Concise and declarative
SELECT * FROM GRAPH_TABLE(
  social_graph MATCH (a)-[]-{1,3}(b) WHERE a.user_id = 123
  COLUMNS (a.user_id AS src, b.user_id AS dst)
);
```

**Advantages:**
- Declarative pattern syntax
- Optimizer can recognize graph idioms
- Easier to write and maintain
- Standard across databases

### Alternative: Separate graph database

**Option:** Use Neo4j, TigerGraph, or other graph database.

**Downsides:**
- Requires ETL to sync relational and graph data
- Separate query language (Cypher, Gremlin)
- Additional infrastructure
- No transactional consistency with relational data

**SQL/PGQ wins:** Unified data model, single transaction, standard SQL.

## Prior art

**Neo4j Cypher:**
```cypher
MATCH (a:Person)-[:KNOWS*1..3]-&gt;(b:Person)
WHERE a.user_id = 123
RETURN a, b
```

**Oracle SQL/PGQ (since Oracle 23c):**
```sql
SELECT *
FROM GRAPH_TABLE (my_graph
  MATCH (a)-[]-&gt;{1,3}(b)
  WHERE a.id = 123
  COLUMNS (a.name, b.name)
);
```

**GQL (Graph Query Language):**
- New ISO standard (ISO/IEC 39075)
- Separate language (like SQL for graphs)
- Influenced SQL/PGQ design

**Apache AGE (PostgreSQL extension):**
- Cypher queries in PostgreSQL
- Separate graph storage
- Inspired PostgreSQL's SQL/PGQ implementation

## Unresolved questions

1. **How to detect graph-eligible queries?**
   - Should Ra automatically suggest graph patterns for recursive CTEs?
   - Can we rewrite recursive CTEs to SQL/PGQ internally?

2. **Index recommendations:**
   - How to recommend graph-specific indexes?
   - When to suggest materialized path views?

3. **Cost estimation:**
   - How to estimate cardinality of variable-length paths?
   - Should we use sampling or statistics?

4. **Parallel execution:**
   - Can we parallelize graph traversals?
   - How to partition graph workloads?

5. **Incremental computation:**
   - Can we cache path results for incremental updates?
   - How to invalidate cached paths when graph changes?

## Future possibilities

**1. Graph algorithms library:**
```sql
-- PageRank
SELECT node_id, pagerank
FROM GRAPH_ALGORITHM(social_graph, 'pagerank', iterations =&gt; 20);

-- Community detection
SELECT node_id, community_id
FROM GRAPH_ALGORITHM(social_graph, 'louvain');
```

**2. Temporal graphs:**
```sql
-- Time-varying graphs
MATCH (a)-[k:KNOWS WHERE k.valid_from &lt;= now() AND k.valid_to &gt; now()]-&gt;(b)
```

**3. Multi-graphs:**
```sql
-- Query across multiple graphs
MATCH (a)-[]-&gt; IN graph1, graph2 (b)
```

**4. Graph mutations:**
```sql
-- Insert edges via graph syntax
INSERT INTO GRAPH social_graph
  MATCH (a:Person {id: 1}), (b:Person {id: 2})
  CREATE (a)-[:KNOWS]-&gt;(b)
```

**5. Streaming graph queries:**
```sql
-- Continuous queries on graph changes
CREATE STREAM friend_alerts AS
  SELECT a.name, b.name
  FROM GRAPH_TABLE (
    social_graph
    MATCH (a)-[:KNOWS]-&gt;(b)
    WHERE b.created_at &gt; now() - interval '1 hour'
  );
```

## Implementation plan

### Phase 1: Parser support (2-3 weeks)

1. Extend sqlparser-rs with SQL/PGQ grammar
2. Add AST nodes for GRAPH_TABLE, MATCH, path patterns
3. Parse property graph definitions (CREATE PROPERTY GRAPH)
4. Add tests for all SQL/PGQ constructs

### Phase 2: Relational algebra translation (2-3 weeks)

1. Translate vertex patterns to Scan/Filter
2. Translate edge patterns to Join
3. Handle path quantifiers (fixed, bounded, unbounded)
4. Implement SHORTEST/ANY/ALL path modes

### Phase 3: Optimization rules (3-4 weeks)

1. Join order optimization for graph patterns
2. Filter pushdown into vertex/edge patterns
3. Bounded quantifier expansion to joins
4. Index selection for adjacency lists
5. Shortest path algorithm selection

### Phase 4: Cost model (2 weeks)

1. Estimate vertex/edge cardinalities
2. Model path quantifier costs
3. Calibrate for different graph densities
4. Add selectivity estimation for patterns

### Phase 5: Testing and documentation (2 weeks)

1. Unit tests for parser/planner
2. Integration tests with sample graphs
3. Performance benchmarks
4. User documentation and examples

**Total: 11-14 weeks**

## References

- **SQL:2023 standard:** ISO/IEC 9075-16:2023 (SQL/PGQ)
- **PostgreSQL docs:** https://www.postgresql.org/docs/devel/queries-graph.html
- **GQL standard:** ISO/IEC 39075
- **Oracle SQL/PGQ:** https://docs.oracle.com/en/database/oracle/property-graph/
- **PostgreSQL hackers list:** Graph query discussions (2023-2024)

---

## Implementation notes

This RFC will require:
- sqlparser-rs extensions for SQL/PGQ syntax
- New RelExpr variants for graph operations
- Specialized optimizer rules for graph patterns
- Documentation and examples

Expected impact: **10-100x speedup** for graph queries vs. recursive CTEs.


## Referenced By

This RFC is referenced by:

- [RFC 93: SQL Property Graph Queries (SQL/PGQ)](/maintainers/rfcs/0093-sql-property-graph-queries)


## Referenced By

This RFC is referenced by:

- [RFC 93: SQL Property Graph Queries (SQL/PGQ)](/maintainers/rfcs/0093-sql-property-graph-queries)


## Referenced By

This RFC is referenced by:

- [RFC 93: SQL Property Graph Queries (SQL/PGQ)](/maintainers/rfcs/0093-sql-property-graph-queries)


## Referenced By

This RFC is referenced by:

- [RFC 93: SQL Property Graph Queries (SQL/PGQ)](/maintainers/rfcs/0093-sql-property-graph-queries)


## Referenced By

This RFC is referenced by:

- [RFC 93: SQL Property Graph Queries (SQL/PGQ)](/maintainers/rfcs/0093-sql-property-graph-queries)


## Referenced By

This RFC is referenced by:

- [RFC 93: SQL Property Graph Queries (SQL/PGQ)](/maintainers/rfcs/0093-sql-property-graph-queries)


## Referenced By

This RFC is referenced by:

- [RFC 93: SQL Property Graph Queries (SQL/PGQ)](/maintainers/rfcs/0093-sql-property-graph-queries)
