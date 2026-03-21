# RFC Proposals for Missing Optimizations

**Date:** 2026-03-21 (updated from 2026-03-20)
**Based on:** CMU Database Group research analysis (54 sources) and PostgreSQL planner gap analysis

These proposals address the highest-impact gaps identified in RA's optimization
coverage. Expanded from 7 to 10 proposals based on deeper research into production
systems (DuckDB, Snowflake, Databricks) and PostgreSQL v13-18 features.

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
- **Prerequisite for:** RFC 4 (incremental sort), GROUP BY reordering, merge join optimization
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

Additional cost model extensions:
- **Correlation-aware index cost**: Use column correlation to estimate
  sequential vs random I/O. `cost = random_cost * (1 - corr^2) + seq_cost * corr^2`
- **Cache-aware random I/O**: Reduce effective random_page_cost based on
  working set size vs available cache.
- **Memory spill threshold**: When hash table exceeds memory budget, add
  2x I/O cost for spill-to-disk.

### Impact
- More accurate cost estimates -> better plan selection
- Eliminates need for manual cost parameter tuning
- Self-improving optimizer over time
- Estimated: 10-30% improvement for workloads with miscalibrated costs

### Complexity
Medium - primarily new infrastructure, minimal changes to existing rules.

### References
- CMU 15-721 Lecture 16: Cost Models
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
5. **Dynamic partition pruning**: For partitioned tables, use runtime filter
   to eliminate entire partitions at execution time

New rules needed:
- `hash-join-bloom-filter-generation`
- `bloom-filter-pushdown-to-scan`
- `semi-join-reduction-insertion`
- `runtime-filter-cost-estimation`
- `dynamic-partition-pruning`

### Impact
- Can reduce probe-side data by 10-100x for selective joins
- Used by Snowflake, StarRocks, Spark, Presto, DataFusion in production
- Most impactful for star schema queries (fact table filtered by dimension joins)
- Estimated: 5-50x improvement for star schema queries

### Complexity
Medium - new rules and cost model extension, but well-understood technique.

### References
- CMU Adaptive Query Processing project
- Snowflake runtime filter documentation
- Spark Dynamic Partition Pruning
- Bloom. "Space/Time Trade-offs in Hash Coding with Allowable Errors" (1970)

---

## RFC Proposal 4: Incremental Sort and Key Reordering

### Problem
When data is partially sorted (e.g., sorted on column A but need A, B), RA does not
model incremental sorting. Also, GROUP BY and DISTINCT key ordering does not consider
available sort orderings from child operators.

### Proposed Solution
Four related optimizations:

1. **Incremental Sort Selection**: When input sorted on prefix of required sort key,
   sort only within each prefix group.
   - Cost: O(n log m) where m = max group size, vs O(n log n) for full sort
   - RA already has IncrementalSort operator; need selection rules
   - Rule: `incremental-sort-selection`

2. **GROUP BY Reordering**: Reorder GROUP BY columns to maximize prefix match
   with available input ordering.
   - Rule: `group-by-key-reordering`
   - Example: input sorted on (a, b), GROUP BY (b, a, c) -> reorder to (a, b, c)

3. **DISTINCT Reordering**: Same principle for DISTINCT columns.
   - Rule: `distinct-key-reordering`

4. **Presorted Aggregate**: When aggregate has ORDER BY or DISTINCT, provide
   presorted input to avoid internal sort.
   - Rule: `presorted-aggregate-optimization`

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
- PostgreSQL commit: GROUP BY reordering (v16)

---

## RFC Proposal 5: Self-Join Elimination and Outer-to-Inner Conversion

### Problem
RA does not detect or eliminate self-joins (table joined to itself unnecessarily)
or convert outer joins to inner joins when WHERE clauses make the outer behavior
unnecessary. Both are common in ORM-generated and complex queries.

### Proposed Solution
Three rule families:

1. **Self-Join Elimination**:
   - Detect when a table is joined to itself on primary/unique key
   - If both sides reference same columns, eliminate one copy
   - Rule: `self-join-elimination`
   - Condition: join on unique key, no conflicting column references
   - Currently limited to INNER joins on plain tables

2. **Outer-to-Inner Join Conversion**:
   - Detect when WHERE clause rejects NULL-extended rows
   - LEFT JOIN -> INNER JOIN when null-rejecting predicate on nullable side
   - Rule: `outer-to-inner-join-conversion`
   - Null-rejecting predicates: equality, comparison, IS NOT NULL, strict functions
   - Cascading: converting one join may enable converting others

3. **Full Outer Join Reduction**:
   - FULL -> LEFT when predicate rejects NULLs on left side
   - FULL -> RIGHT when predicate rejects NULLs on right side
   - FULL -> INNER when predicate rejects NULLs on both sides
   - Rule: `full-outer-join-reduction`

### Impact
- Self-join elimination: removes entire join operations (2x or more improvement)
- Outer-to-inner: enables additional optimization (inner joins allow reordering)
- Common in ORM-generated SQL (Django, Rails, SQLAlchemy)
- Estimated: affects 5-15% of production queries

### Complexity
Low - well-understood transformations with clear correctness conditions.

### References
- PostgreSQL v17: enable_self_join_elimination
- TiDB: outer-join-elimination rule
- CockroachDB: EliminateJoin transformation
- DataFusion: EliminateOuterJoin rule

---

## RFC Proposal 6: Cardinality Estimation Enhancement

### Problem
Cardinality estimation errors compound exponentially through join trees (Leis et al.
2015). RA's current statistics model lacks MCV lists and has limited selectivity
estimation. Advanced estimation techniques from PostgreSQL are not implemented.

### Proposed Solution
Multi-layer enhancement:

1. **Most Common Values (MCV) Lists**:
   - Add MCV list to ColumnStats: Vec<(String, f64)> for (value, frequency)
   - Equality selectivity: check MCV first, fall back to NDV
   - Range selectivity: combine MCV frequencies with histogram
   - Join selectivity: compare MCV lists between columns

2. **Semi-Join / Anti-Join Cardinality**:
   - Semi-join: `sel = 1 - (1 - 1/ndv_inner)^n_outer`
   - Anti-join: `sel = (1 - 1/ndv_inner)^n_inner_per_key`
   - Replace heuristic estimation with probability formulas

3. **Estimation Error Detection**:
   - After execution, compare estimated vs actual row counts
   - Flag operators with > 10x error
   - Recommend ANALYZE when statistics appear stale

4. **Estimation Safety Margin**:
   - For join trees > 3 tables, multiply cardinality by safety factor
   - Factor increases with join depth: 1.5x per additional join
   - Prefer robust plans (hash join) over fragile plans (nested loop)

### Impact
- More accurate selectivity -> better plan selection
- Identifies stale statistics before they cause problems
- Prevents worst-case cardinality estimation disasters
- Estimated: 20-40% reduction in plan regression incidents

### Complexity
Medium - requires statistics model extension and new estimation formulas.

### References
- Leis et al. "How Good Are Query Optimizers, Really?" (2015)
- PostgreSQL row estimation examples documentation
- PostgreSQL extended statistics (v14)

---

## RFC Proposal 7: Large Join Graph Optimization Fallback

### Problem
E-graph equality saturation can be expensive for queries with many tables (10+).
PostgreSQL uses GEQO (genetic algorithm) for 12+ tables. RA recently implemented
a large join fallback (RFC 0017) -- verify completeness.

### Proposed Solution
Verify and extend the existing implementation:

1. **Threshold detection**: Already implemented?
2. **Simulated Annealing**: Preferred over genetic algorithm
3. **Greedy Heuristic**: Fast initial solution
4. **Bounded planning time**: Guaranteed termination

### Status
Check if RFC 0017 implementation addresses this fully.

### References
- PostgreSQL GEQO documentation
- Steinbrunn et al. "Heuristic and Randomized Optimization" (1997)

---

## RFC Proposal 8: Top-N Sort and Empty Result Propagation

### Problem
Two common micro-optimizations that RA lacks:
1. ORDER BY + LIMIT can use heap-based top-N sort instead of full sort
2. When any input to an operator is provably empty, the entire subtree can be eliminated

### Proposed Solution

1. **Top-N Sort**:
   - Detect Sort immediately followed by Limit
   - Replace with TopN operator: O(n log k) instead of O(n log n) for LIMIT k
   - Use min-heap for ascending, max-heap for descending
   - Memory: O(k) instead of O(n)
   - Rule: `sort-limit-to-topn`

2. **Empty Result Propagation**:
   - When filter has contradictory predicates (WHERE false, x > 5 AND x < 3)
   - When input table has 0 rows (from statistics)
   - Propagate empty result upward:
     - Empty JOIN anything -> Empty (for inner/semi joins)
     - Anything JOIN Empty -> Empty (for inner/semi joins)
     - Empty UNION Empty -> Empty
     - Filter(false, X) -> Empty
   - Rule: `propagate-empty-relation`

### Impact
- Top-N: 10x+ speedup for ORDER BY + LIMIT (very common pattern)
- Empty: eliminates unnecessary computation for impossible subqueries
- Both implemented by BusTub, DuckDB, DataFusion, PostgreSQL

### Complexity
Low - straightforward pattern-matching rules.

### References
- BusTub: sort_limit_as_topn.cpp
- DataFusion: PropagateEmptyRelation rule
- DuckDB: empty result optimization

---

## RFC Proposal 9: Memoize for Parameterized Scans

### Problem
In nested loop joins, the inner side is rescanned for each outer row. When outer
join key values repeat, the same inner scan produces identical results. PostgreSQL's
Memoize node (v14) caches these results.

### Proposed Solution

1. **Memoize Node**:
   - Add Memoize as a new plan node that wraps parameterized inner scans
   - LRU cache keyed on parameter values
   - Cache entries: result sets from inner scan

2. **Memoize Insertion Rule**:
   - Pattern: NestedLoop(outer, ParameterizedScan(params))
   - Condition: estimated hit ratio > threshold (e.g., > 0.5)
   - Hit ratio = 1 - (n_distinct_join_key / outer_rows)
   - Result: NestedLoop(outer, Memoize(key=params, child=ParameterizedScan))

3. **Cost Model**:
   - Miss cost: full inner scan cost per distinct key
   - Hit cost: cache lookup cost (near zero CPU, zero I/O)
   - Total: outer_rows * (miss_ratio * inner_cost + hit_ratio * lookup_cost)
   - Memory: cache_entry_size * min(n_distinct, cache_capacity)

### Impact
- 10-1000x speedup for joins with repeated outer key values
- Common in OLTP: orders -> customers, line_items -> products
- PostgreSQL v14 showed major TPC-H improvements
- Estimated: affects 10-20% of nested loop joins

### Complexity
Medium - new plan node and insertion rule.

### References
- PostgreSQL v14: Memoize node
- PostgreSQL EXPLAIN output: Cache Hits/Misses/Evictions

---

## RFC Proposal 10: Expression Simplification Extensions

### Problem
Several expression-level optimizations from DataFusion and other systems are not
implemented in Ra:
- Constants in GROUP BY waste aggregation work
- COUNT(DISTINCT x) can be rewritten as COUNT over GROUP BY
- Nested UNIONs can be flattened for better optimization

### Proposed Solution

1. **Group By Constant Elimination**:
   - Remove constant expressions from GROUP BY key list
   - Example: `GROUP BY 1, col1, col2` -> `GROUP BY col1, col2`
   - Constants don't affect grouping but consume hash/compare resources

2. **Single Distinct to Group By**:
   - `SELECT COUNT(DISTINCT col) FROM t`
   - -> `SELECT COUNT(*) FROM (SELECT col FROM t GROUP BY col)`
   - Enables hash-based distinct elimination instead of sort-based

3. **Nested Union Flattening**:
   - `UNION ALL(UNION ALL(A, B), C)` -> `UNION ALL(A, B, C)`
   - Reduces plan tree depth, enables better optimization of all branches

4. **Cross Join to Inner Conversion**:
   - When CROSS JOIN followed by WHERE with equality on both sides
   - `FROM a CROSS JOIN b WHERE a.id = b.aid` -> `FROM a INNER JOIN b ON a.id = b.aid`
   - Enables join algorithm selection (hash join, merge join)

5. **Extract Equijoin Predicate**:
   - Separate equality predicates from non-equality in join conditions
   - Equality predicates determine join method eligibility (hash, merge)
   - Non-equality predicates applied as post-join filter

### Impact
- Each individually small (2-5% improvement)
- Collectively significant for complex queries
- All implemented in DataFusion and other production systems

### Complexity
Low - straightforward pattern-matching rules.

### References
- DataFusion: EliminateGroupByConstant, SingleDistinctToGroupBy
- DataFusion: EliminateCrossJoin, ExtractEquijoinPredicate

---

## Implementation Priority

| RFC | Priority | Prerequisite | Complexity | Impact |
|-----|----------|-------------|------------|--------|
| 1. Physical Properties | Critical | None | High | 30-40% of queries |
| 5. Self-Join/Outer-Inner | Critical | None | Low | 5-15% of queries |
| 8. Top-N/Empty Propagation | High | None | Low | Common patterns |
| 10. Expression Extensions | High | None | Low | Cumulative |
| 6. Cardinality Enhancement | High | None | Medium | 20-40% accuracy |
| 3. Runtime Filters | High | None | Medium | 5-50x star schema |
| 4. Incremental Sort | High | RFC 1 | Low-Med | 15-25% of GROUP BY |
| 9. Memoize | Medium | None | Medium | 10-20% of NLJ |
| 2. Cost Calibration | Medium | None | Medium | 10-30% improvement |
| 7. Large Join Fallback | Low | Check existing | Medium | Edge cases |
