# CMU 15-721 Lecture 17: Optimizer Implementation (Top-Down vs. Bottom-Up)

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Two dominant optimization frameworks: Volcano/Cascades (top-down) and System R (bottom-up)
- Each has different tradeoffs for search efficiency and implementation complexity
- Modern systems increasingly use hybrid approaches

## Top-Down Optimization (Cascades Framework)

### Architecture
- Start from logical plan root
- Apply transformation rules to generate equivalent expressions
- Use memoization (memo table / groups) to avoid redundant computation
- Branch-and-bound: prune subplans that exceed current best cost

### Key Data Structures
- **Memo**: hash table of equivalent expression groups
- **Group**: set of logically equivalent expressions
- **Expression**: operator + input groups
- **Winner**: cheapest physical plan for a group + required properties

### Rule Types
1. Transformation rules: logical -> logical equivalence
2. Implementation rules: logical -> physical operator
3. Enforcer rules: insert operators to satisfy properties

### Advantages
- Lazy evaluation: only explores promising branches
- Property-driven: physical properties (ordering, partitioning) guide search
- Naturally handles interesting orderings
- Better for complex queries with many alternatives

### Disadvantages
- Complex implementation
- Overhead of memoization structure
- Recursive nature can be harder to debug

## Bottom-Up Optimization (System R / Dynamic Programming)

### Architecture
- Enumerate base table access paths
- Progressively combine into two-way, three-way, etc. joins
- Store optimal plan for each subset of tables + interesting orderings
- Select cheapest complete plan

### Algorithm
1. For each single table, enumerate: seq scan, index scans
2. For each pair of tables, try: NL join, merge join, hash join (both orderings)
3. For each triple, combine pairs with single tables
4. Continue until all tables joined
5. Track "interesting orderings" at each step

### Advantages
- Simpler implementation
- Deterministic
- Well-understood guarantees
- Good for OLTP with few tables

### Disadvantages
- O(2^n) space for n tables
- Must fully enumerate each level before moving on
- Cannot prune early using upper bounds
- No natural handling of physical properties

## Practical Systems

| System | Framework | Notes |
|--------|-----------|-------|
| PostgreSQL | Bottom-up (System R) + GEQO | DP for <12 tables, genetic for 12+ |
| SQL Server | Top-down (Cascades) | Full memo-based search |
| CockroachDB | Top-down (Cascades variant) | Custom implementation |
| Apache Calcite | Top-down (Volcano) | Rule-based, pluggable |
| DuckDB | Bottom-up + heuristics | Custom DP-based |
| DataFusion | Bottom-up + heuristics | Rule phases |

## Applicable to RA
- RA uses egg e-graph (equality saturation) - a distinct third approach
- E-graphs explore ALL equivalent expressions simultaneously
- Extraction selects cheapest plan from saturated e-graph
- Gap: No physical property tracking during optimization
- Gap: No enforcer rules (Sort, Exchange operators)
- Gap: No multi-property optimization (order + partitioning)
- Gap: Equality saturation may not scale for very large search spaces
- Gap: No comparison/hybrid with traditional Cascades for complex queries

## References
- Graefe. "The Cascades Framework" (1995)
- Pellenkoft, Galindo-Legaria, Kersten. "The Complexity of Transformation-Based Join Enumeration" (1997)
- Moerkotte & Neumann. "Analysis of Two Existing and One New Dynamic Programming Algorithm for the Generation of Optimal Bushy Join Trees" (2006)
