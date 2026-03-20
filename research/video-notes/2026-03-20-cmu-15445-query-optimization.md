# CMU 15-445 Lecture 14: Query Planning & Optimization

**Source:** https://15445.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- PostgreSQL and most production databases use pure cost-based optimization
- Query optimization is the most important engineering effort in any DBMS
- Two main approaches: heuristic/rule-based and cost-based
- The optimizer translates logical algebra expressions to optimal physical plans
- Relational equivalences allow plan enumeration without changing output

## Optimization Techniques

### Heuristic / Rule-Based
1. Predicate pushdown - move WHERE filters below joins
2. Projection pushdown - remove unnecessary columns early
3. Expression simplification - constant folding, impossible/unnecessary predicates
4. De-correlation of subqueries - convert correlated subqueries to joins
5. Join elimination - remove unnecessary joins (e.g., when FK guarantees uniqueness)

### Cost-Based Optimization
1. Enumerate equivalent plans using transformation rules
2. Estimate cost of each plan using a cost model
3. Select plan with lowest estimated cost
4. Cost depends on: CPU, I/O, network (distributed), memory

### Transformation Rules
- Predicate pushdown through joins
- Join commutativity: A join B = B join A
- Join associativity: (A join B) join C = A join (B join C)
- Projection pushdown through joins
- Selection splitting and combining
- Aggregate pushdown below joins (when valid)

### Join Ordering
- Dynamic programming for small numbers of relations (< ~12)
- Genetic/heuristic algorithms for larger join graphs
- Left-deep trees vs bushy trees
- Left-deep preferred for pipelining but bushy can be better

### Cardinality Estimation
- Histograms (equi-width, equi-depth)
- Most Common Values (MCV) lists
- Sketches (Count-Min, HyperLogLog)
- Sampling-based estimation
- Independence assumption (multiply selectivities) - often wrong

### Common Pitfalls
- Cardinality estimation errors compound multiplicatively through joins
- Independence assumption fails with correlated columns
- Uniform distribution assumption rarely holds
- Cost models rely on stale statistics
- GEQO/heuristic fallback for many-table joins loses optimality

## Applicable to RA
- RA already has join reordering (9 rules) and predicate pushdown (17 rules)
- Gap: No genetic/simulated-annealing fallback for large join graphs
- Gap: No adaptive re-optimization on cardinality estimation errors
- Gap: Limited multi-column correlation handling
- Gap: No workload-aware statistics gathering

## References
- Selinger et al. "Access Path Selection in a Relational Database Management System" (System R, 1979)
- Graefe & McKenna. "The Volcano Optimizer Generator" (1993)
- Graefe. "The Cascades Framework for Query Optimization" (1995)
