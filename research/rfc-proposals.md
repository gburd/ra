# RFC Proposals for Missing Optimizations

**Date:** 2026-03-20
**Based on:** CMU Database Group research analysis and PostgreSQL planner gap analysis

These proposals address the highest-impact gaps identified in RA's optimization
coverage. Each is a brief proposal summary suitable for expansion into a full RFC.

---

## RFC Proposal 1: Physical Property Tracking Framework

### Problem
RA's e-graph optimizer does not track physical properties (sort ordering, data
partitioning, data distribution) through plan nodes. This means the optimizer cannot
reason about when a sort is redundant, when a merge join is free because data is
already sorted, or when an exchange operator is needed in distributed plans.

### Proposed Solution
Extend the e-graph extraction phase with a physical property tracking layer:

1. Define a `PhysicalProperties` trait with variants:
   - `Ordering`: list of (column, ASC/DESC) pairs
   - `Partitioning`: hash/range/round-robin on columns
   - `Distribution`: replicated/partitioned/singleton
2. Each physical operator declares:
   - Required input properties
   - Provided output properties
   - Enforcer cost (cost to add Sort/Exchange if needed)
3. During extraction, property requirements propagate top-down while
   available properties propagate bottom-up
4. Enforcers (Sort, Exchange) are inserted only when needed

### Impact
- Eliminates redundant sorts (ORDER BY after merge join on same key)
- Enables interesting orderings (keep costlier plan with useful ordering)
- Required for correct distributed query planning
- Estimated: affects 30-40% of multi-table queries

### Complexity
High - requires changes to e-graph extraction and cost model.

### References
- Graefe. "The Cascades Framework for Query Optimization" (1995)
- Selinger et al. "Access Path Selection" (1979) - interesting orderings
- PostgreSQL pathkeys system

---

## RFC Proposal 2: Adaptive Cost Model Calibration

### Problem
RA's cost model uses fixed parameters that may not match the actual hardware and
workload. PostgreSQL requires manual tuning of seq_page_cost, random_page_cost, etc.
There is no feedback loop from actual execution to cost model adjustment.

### Proposed Solution
Implement a cost model calibration system with three tiers:

1. **Static calibration**: Run micro-benchmarks on target hardware to measure
   actual I/O, CPU, and memory operation costs. Store as hardware profile.
2. **Dynamic calibration**: After query execution, compare estimated cost/rows
   with actual. Maintain running statistics of estimation accuracy.
3. **Adaptive correction**: When systematic bias is detected (e.g., hash join
   cost consistently underestimated by 3x), apply correction factors.

Data flow:
```
EXPLAIN ANALYZE output -> (actual_time, actual_rows, actual_buffers)
                       -> compare with (estimated_cost, estimated_rows)
                       -> update correction factors
                       -> apply to future cost estimates
```

### Impact
- More accurate cost estimates -> better plan selection
- Eliminates need for manual cost parameter tuning
- Self-improving optimizer over time
- Estimated: 10-30% improvement for workloads with miscalibrated costs

### Complexity
Medium - primarily new infrastructure, minimal changes to existing rules.

### References
- CMU 15-721 Lecture 18: Cost Models
- DuckDB cost model calibration approach
- Van Aken et al. "OtterTune" (2017) - ML-based tuning

---

## RFC Proposal 3: Sideways Information Passing (Runtime Filters)

### Problem
RA has no mechanism for passing information between operators during execution.
Specifically, hash join build phases produce bloom filters that could dramatically
reduce scan output on the probe side, but this optimization is not modeled.

### Proposed Solution
Add runtime filter rules and infrastructure:

1. **Bloom filter generation rule**: When building hash table for hash join,
   generate bloom filter on join key columns
2. **Filter pushdown rule**: Push generated bloom filter to scan operators
   on the probe side
3. **Cost model extension**: Model the cost of bloom filter creation,
   the selectivity benefit, and the overhead of false positives
4. **Semi-join reduction**: Before full join, apply semi-join filter
   from build side to reduce probe side cardinality

New rules needed:
- `hash-join-bloom-filter-generation`
- `bloom-filter-pushdown-to-scan`
- `semi-join-reduction-insertion`
- `runtime-filter-cost-estimation`

### Impact
- Can reduce probe-side data by 10-100x for selective joins
- Used by StarRocks, Spark, Presto, DataFusion in production
- Most impactful for star schema queries (fact table filtered by dimension joins)
- Estimated: 5-50x improvement for star schema queries

### Complexity
Medium - new rules and cost model extension, but well-understood technique.

### References
- CMU Adaptive Query Processing project
- Bloom. "Space/Time Trade-offs in Hash Coding with Allowable Errors" (1970)
- StarRocks runtime filter documentation

---

## RFC Proposal 4: Incremental Sort and Key Reordering

### Problem
When data is partially sorted (e.g., sorted on column A but need A, B), RA does not
model incremental sorting. Also, GROUP BY and DISTINCT key ordering does not consider
available sort orderings from child operators.

### Proposed Solution
Three related optimizations:

1. **Incremental Sort**: When input sorted on prefix of required sort key,
   sort only within each prefix group.
   - Cost: O(n log m) where m = max group size, vs O(n log n) for full sort
   - Rule: `incremental-sort-selection`
   - Condition: input has pathkeys matching prefix of required sort

2. **GROUP BY Reordering**: Reorder GROUP BY columns to maximize prefix match
   with available input ordering.
   - Rule: `group-by-key-reordering`
   - Example: input sorted on (a, b), GROUP BY (b, a, c) -> reorder to (a, b, c)

3. **DISTINCT Reordering**: Same principle for DISTINCT columns.
   - Rule: `distinct-key-reordering`
   - Reorder to match available ordering, avoiding unnecessary sort

### Impact
- Reduces sort cost for partially-ordered data
- Common scenario: index provides partial ordering
- PostgreSQL added these in v13/v16 - proven production value
- Estimated: eliminates sorts in 15-25% of GROUP BY/DISTINCT queries

### Complexity
Low-Medium - straightforward rule implementations, but requires physical
property tracking (RFC Proposal 1) as prerequisite.

### References
- PostgreSQL: enable_incremental_sort, enable_group_by_reordering
- PostgreSQL commit: Incremental Sort (v13)

---

## RFC Proposal 5: Self-Join Elimination and Outer-to-Inner Conversion

### Problem
RA does not detect or eliminate self-joins (table joined to itself unnecessarily)
or convert outer joins to inner joins when WHERE clauses make the outer behavior
unnecessary. Both are common in ORM-generated and complex queries.

### Proposed Solution
Two rule families:

1. **Self-Join Elimination**:
   - Detect when a table is joined to itself on primary/unique key
   - If both sides reference same columns, eliminate one copy
   - Rule: `self-join-elimination`
   - Condition: join on unique key, no conflicting column references
   - Example: `SELECT * FROM t1 a JOIN t1 b ON a.id = b.id WHERE a.x = 1`
     -> `SELECT * FROM t1 WHERE x = 1`

2. **Outer-to-Inner Join Conversion**:
   - Detect when WHERE clause rejects NULL-extended rows
   - LEFT JOIN -> INNER JOIN when WHERE right.col IS NOT NULL (explicitly or implicitly)
   - Rule: `outer-to-inner-join-conversion`
   - Conditions:
     - WHERE clause has predicate on null-extended side
     - Predicate rejects NULLs (IS NOT NULL, equality, comparison)
   - Example: `SELECT * FROM a LEFT JOIN b ON a.id = b.aid WHERE b.x > 5`
     -> `SELECT * FROM a INNER JOIN b ON a.id = b.aid WHERE b.x > 5`

### Impact
- Self-join elimination: removes entire join operations (2x or more improvement)
- Outer-to-inner: enables additional optimization (inner joins allow reordering)
- Common in ORM-generated SQL (Django, Rails, SQLAlchemy)
- PostgreSQL added self-join elimination in v17 - proven production value
- Estimated: affects 5-15% of production queries

### Complexity
Low - well-understood transformations with clear correctness conditions.

### References
- PostgreSQL v17: enable_self_join_elimination
- TiDB: outer-join-elimination rule
- CockroachDB: EliminateJoin transformation

---

## RFC Proposal 6: Cardinality Estimation Error Detection

### Problem
Cardinality estimation errors compound exponentially through join trees. A 2x error
per join becomes 2^N for N joins (Leis et al. 2015). RA has no mechanism to detect
when estimates are wrong or to trigger re-optimization.

### Proposed Solution
Two-phase approach:

1. **Estimation Error Logging**:
   - After execution, compare estimated vs actual row counts per operator
   - Log operators where |log(actual/estimated)| > threshold (e.g., > 1.0, meaning 10x error)
   - Maintain per-table, per-join error history
   - Rule: `estimation-error-detection`

2. **Statistics Staleness Detection**:
   - Track last ANALYZE time per table
   - Track row count changes (INSERT/UPDATE/DELETE since last ANALYZE)
   - Flag tables where statistics are likely stale
   - Rule: `statistics-staleness-detection`
   - Recommend: run ANALYZE on flagged tables

3. **Extended Statistics Advisor** (stretch goal):
   - Detect correlated columns from estimation errors
   - Recommend CREATE STATISTICS for column combinations
   - Monitor improvement after statistics creation

### Impact
- Identifies root cause of bad plans (stale or missing statistics)
- Enables proactive maintenance (ANALYZE before problems manifest)
- Foundation for adaptive re-optimization
- Estimated: prevents 20-40% of plan regression incidents

### Complexity
Medium - requires execution instrumentation and statistics tracking.

### References
- Leis et al. "How Good Are Query Optimizers, Really?" (2015)
- PostgreSQL TODO: estimation error logging
- CMU 15-721 Lecture 18: Cost Models - common pitfalls

---

## RFC Proposal 7: Large Join Graph Optimization Fallback

### Problem
E-graph equality saturation can be expensive for queries with many tables (10+).
PostgreSQL uses GEQO (genetic algorithm) for 12+ tables. RA has no fallback
mechanism for large join graphs.

### Proposed Solution
Implement a configurable fallback for large join graphs:

1. **Threshold detection**: When e-graph has > N relations (configurable,
   default 10), switch to heuristic mode
2. **Simulated Annealing** (preferred over genetic):
   - Start from heuristic initial plan
   - Randomly perturb join ordering
   - Accept improvements, probabilistically accept worse plans
   - Reduce temperature over time
   - More predictable than GEQO, better theoretical properties
3. **Greedy Heuristic** (fast fallback):
   - Start from smallest relation
   - Greedily add the join with lowest estimated cost
   - O(n^2) instead of O(2^n)
   - Use as initial solution for simulated annealing

### Impact
- Handles queries that currently timeout in e-graph saturation
- Provides usable plans for 15-50 table joins
- Bounded planning time regardless of query complexity
- Estimated: enables optimization of queries currently unoptimizable

### Complexity
Medium - new search strategy, but independent of existing e-graph infrastructure.

### References
- PostgreSQL GEQO documentation
- PostgreSQL TODO: investigate compressed annealing
- Steinbrunn et al. "Heuristic and Randomized Optimization" (1997)
- Ioannidis & Kang. "Randomized Algorithms for Optimizing Large Join Queries" (1990)
