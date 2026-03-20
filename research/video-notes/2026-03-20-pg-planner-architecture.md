# PostgreSQL Planner/Optimizer Architecture

**Source:** https://www.postgresql.org/docs/current/planner-optimizer.html
**Date:** Reference documentation (current)
**Speaker:** PostgreSQL documentation

## Key Points
- PostgreSQL uses pure cost-based optimization with no hint system
- Bottom-up dynamic programming for join ordering (System R style)
- Genetic Query Optimizer (GEQO) for 12+ table joins
- Paths are lightweight representations; plans are built from cheapest path

## Architecture

### Planning Phases
1. **Preprocessing**: CTE handling, subquery flattening, outer-to-inner join conversion
2. **Single-table access paths**: Sequential scan, index scans for each table
3. **Join enumeration**: DP for small queries, GEQO for large
4. **Plan selection**: Cheapest path converted to full plan tree
5. **Post-processing**: Add sort nodes, aggregation, LIMIT

### Access Path Generation
- Sequential scan: always generated as baseline
- Index scan: for each matching index + predicate
- Bitmap scan: for OR conditions or low-selectivity predicates
- Index-only scan: when index covers all needed columns
- TID scan: for ctid-based access

### Join Methods
| Method | Best For | Cost Profile |
|--------|----------|-------------|
| Nested Loop | Small outer, indexed inner | Low startup, variable total |
| Merge Join | Pre-sorted inputs, large | Medium startup, linear scan |
| Hash Join | Equi-joins, large unsorted | Build cost + linear probe |

### Join Ordering
- Exhaustive DP for < geqo_threshold (default 12) relations
- Considers only join pairs with WHERE join clauses
- Tracks "interesting orderings" at each step
- GEQO: genetic algorithm for 12+ relations

### Key Parameters
| Parameter | Default | Impact |
|-----------|---------|--------|
| geqo_threshold | 12 | Switch from DP to genetic |
| from_collapse_limit | 8 | Flatten subqueries in FROM |
| join_collapse_limit | 8 | Rewrite explicit JOINs |
| enable_* | on | Toggle plan node types |

## Applicable to RA
- RA uses egg/e-graph which is fundamentally different from PostgreSQL's approach
- Gap: No DP-based join ordering for comparison/fallback
- Gap: No GEQO-equivalent for large join graphs
- Gap: No "interesting orderings" framework
- Gap: No subquery flattening / FROM collapse rules
- Gap: No outer-to-inner join conversion during preprocessing
- Gap: No bitmap scan or TID scan modeling

## References
- Selinger et al. "Access Path Selection in a Relational Database Management System" (1979)
- PostgreSQL source: src/backend/optimizer/
