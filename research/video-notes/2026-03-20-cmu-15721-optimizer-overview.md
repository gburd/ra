# CMU 15-721 Lecture 16: Optimizer Implementation (Overview)

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Query optimization is the hardest problem in database systems
- Two fundamental approaches: heuristic and cost-based
- Most production systems use a mix of both
- Search strategy determines how the space of equivalent plans is explored

## Optimizer Architecture

### Logical vs Physical Optimization
1. Logical optimization: apply equivalence-preserving transformations
   - Predicate pushdown, join reordering, subquery decorrelation
   - Does not consider physical implementation
2. Physical optimization: choose concrete algorithms
   - Hash join vs merge join vs nested loop
   - Index scan vs sequential scan
   - Sort order propagation

### Search Strategies

#### Heuristic / Rule-Based
- Apply fixed set of rules in predetermined order
- Fast but may miss better plans
- PostgreSQL rewriter applies some heuristic rules before cost-based planning
- Common heuristics: push selections, push projections, convert subqueries

#### Exhaustive Search (System R style)
- Dynamic programming: enumerate all join orderings
- Bottom-up: start from base relations, build up
- O(n!) for n relations (with pruning: O(2^n))
- Optimal within search space but expensive for many tables

#### Top-Down (Cascades/Volcano)
- Start from root, apply transformations recursively
- Memoization avoids redundant work
- Branch-and-bound pruning
- Used by SQL Server (Cascades), Greenplum (ORCA)

#### Randomized / Genetic
- Simulated annealing, genetic algorithms
- PostgreSQL GEQO for 12+ tables
- Good enough solution in bounded time
- Non-deterministic results

### Rule Application Strategies
- Transformation rules: logical -> logical (join commutativity)
- Implementation rules: logical -> physical (Join -> HashJoin)
- Enforcer rules: add operators for required properties (Sort for ORDER BY)

## Applicable to RA
- RA uses egg e-graph library for optimization (bottom-up with equality saturation)
- Gap: No top-down (Cascades) search strategy
- Gap: No branch-and-bound pruning during optimization
- Gap: No enforcer rules for property requirements (ordering, partitioning)
- Gap: No rule prioritization or cost-based rule selection
- Gap: No multi-phase optimization (heuristic phase then cost-based phase)

## References
- Selinger et al. "System R" (1979)
- Graefe. "The Volcano Optimizer Generator" (1993)
- Graefe. "The Cascades Framework for Query Optimization" (1995)
- Soliman et al. "Orca: A Modular Query Optimizer Architecture" (2014)
