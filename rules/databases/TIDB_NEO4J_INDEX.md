# TiDB and Neo4j Rules Extraction Summary

Complete index of optimization rules extracted from TiDB and Neo4j codebases for Task #5 completion.

## TiDB Rules (12 rules from github.com/pingcap/tidb)

### Aggregation Optimization (3 rules)
1. ✅ **aggregation-pushdown-decomposable.rra** - Decomposable agg pushdown (MAX, MIN, SUM, COUNT)
2. 📋 **aggregation-elimination.rra** - Remove redundant GROUP BY
3. 📋 **aggregation-merge.rra** - Merge consecutive aggregations

### Coprocessor Pushdown (4 rules)
4. 📋 **coprocessor-selection-pushdown.rra** - Push filters to TiKV coprocessor
5. 📋 **coprocessor-projection-pushdown.rra** - Push projections to TiKV
6. 📋 **coprocessor-limit-pushdown.rra** - Push LIMIT to storage layer
7. 📋 **coprocessor-topn-pushdown.rra** - Push TOP-N to TiKV

### Join Optimization (3 rules)
8. 📋 **join-reorder-dp.rra** - Dynamic programming join ordering
9. 📋 **semi-join-rewrite.rra** - Semi-join to inner join transformation
10. 📋 **outer-join-to-inner.rra** - Outer join simplification

### Distributed Execution (2 rules)
11. 📋 **partition-pruning.rra** - Table partition elimination
12. 📋 **index-merge.rra** - Multiple index access merge

## Neo4j Rules (10 rules from github.com/neo4j/neo4j)

### Graph Traversal (4 rules)
13. 📋 **expand-into-optimization.rra** - Expand into vs expand all
14. 📋 **bidirectional-traversal.rra** - Meet-in-the-middle traversal
15. 📋 **depth-first-vs-breadth-first.rra** - Traversal strategy selection
16. 📋 **variable-length-path-optimization.rra** - Efficient path expansion

### Shortest Path (2 rules)
17. 📋 **dijkstra-shortest-path.rra** - Weighted shortest path
18. 📋 **all-shortest-paths-optimization.rra** - Multiple paths finding

### Index Usage (2 rules)
19. 📋 **relationship-index-lookup.rra** - Index on relationship properties
20. 📋 **composite-index-usage.rra** - Multi-property index selection

### Pattern Matching (2 rules)
21. 📋 **pattern-rewrite.rra** - Cypher pattern optimization
22. 📋 **label-scan-optimization.rra** - Node label filtering

## Implementation Status

- ✅ Completed: 1/22 (full detail with source references)
- 📋 Documented: 21/22 (patterns identified, awaiting implementation)
- **TiDB source**: `pkg/planner/core/` (shallow clone completed)
- **Neo4j source**: Pending clone for extraction

## Key Source Files Referenced

### TiDB
- `rule_aggregation_push_down.go` - Decomposable aggregation logic
- `rule_join_reorder_dp.go` - DP-based join ordering
- `rule_semi_join_rewrite.go` - Semi-join transformations
- `find_best_task.go` - Coprocessor task assignment
- `exhaust_physical_plans.go` - Physical plan generation

### Neo4j (to be extracted)
- `cypher/planner/` - Cypher query planner
- `cypher/runtime/` - Execution runtime
- `kernel/api/index/` - Index management
- `cypher/ir/` - Intermediate representation

## Next Steps

1. Complete TiDB rule extraction (11 remaining)
2. Clone Neo4j repository (shallow)
3. Extract 10 Neo4j graph optimization rules
4. Create comprehensive .rra files with source references
5. Cross-database comparison tables

**Target completion**: 22 rules total for Task #5 (TiDB + Neo4j portion)
