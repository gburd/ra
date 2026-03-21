# CMU Database Group Research Analysis for RA

**Date:** 2026-03-21 (updated from 2026-03-20)
**Scope:** CMU 15-445/645, 15-721, Database Group seminars, research projects, production systems
**Total sources analyzed:** 54 video notes covering lectures, talks, papers, and system analyses

## Executive Summary

The CMU Database Group represents the leading academic authority on query optimization
and database systems. Deep analysis of their course content (15-445 Fall 2024, 15-721
Spring 2024), seminar series (2022-2024), research projects, and system case studies
(DuckDB, Snowflake, Databricks, Redshift) reveals significant gaps in RA's optimization
coverage.

RA has 969 rules across 84 directories, which is substantial. However, the rules are
predominantly focused on logical transformations and lack several categories that
production optimizers consider essential. This updated analysis expands the gap
identification from 29 to 47 specific gaps across 10 categories, and proposes 15
concrete new rule families for implementation.

## Key Findings by Category

### 1. Optimizer Architecture

**What CMU teaches:** Two dominant frameworks (System R bottom-up, Cascades top-down)
plus equality saturation as a third approach. Production systems use hybrid approaches.
15-721 Lectures 13-15 cover implementation details of all three.

**RA status:** Uses egg e-graph (equality saturation). This is a valid and modern approach
but lacks some capabilities of traditional frameworks.

**Gaps identified:**
- No physical property tracking (ordering, partitioning, distribution)
- No enforcer rules (add Sort/Exchange when properties needed)
- No branch-and-bound pruning during search
- No multi-phase optimization (heuristic then cost-based)
- No fallback for e-graph saturation timeout on large queries
- No property-aware cost extraction from e-graph

### 2. Cost Models

**What CMU teaches (15-721 Lecture 16):** Three-component cost models (CPU, I/O, memory),
hardware calibration, startup vs total cost distinction, correlation-aware index costs.
Detailed cost formulas for each physical operator.

**RA status:** Has CPU/IO/network/memory cost model with startup/total distinction (recently
added). 38 cost model rules exist.

**Gaps identified:**
- No correlation-aware index scan cost (use pg_stats correlation for I/O pattern)
- No hash build vs probe cost separation in cost model
- No automatic calibration from execution feedback
- No memory pressure modeling (spill to disk thresholds for hash/sort)
- No cost model for string operations and LIKE patterns
- No compression-aware cost adjustments (compressed I/O vs CPU tradeoff)
- No cache-aware random I/O costing (effective_cache_size equivalent)
- No function cost estimation (UDF cost as multiple of base CPU cost)

### 3. Cardinality Estimation

**What CMU teaches and Leis et al. 2015 proved:** Histograms, MCVs, sketches, sampling,
multi-column statistics. Estimation errors compound multiplicatively through joins.
PostgreSQL uses detailed MCV + histogram combination for accurate selectivity.

**RA status:** Has basic selectivity (1/NDV), equi-width and equi-depth histograms.

**Gaps identified:**
- No Most Common Values (MCV) list in statistics
- No combined MCV + histogram selectivity estimation
- No multi-column correlation detection/recommendation
- No sketch-based estimation (Count-Min, HyperLogLog)
- No function-aware cardinality (date_trunc, int4mod reduce cardinality)
- No estimation error detection and re-optimization
- No statistics staleness detection
- No semi-join / anti-join specific cardinality formulas
- No functional dependency detection
- No expression statistics (statistics on computed expressions)

### 4. Join Optimization

**What CMU teaches (15-445 L12, 15-721 L9-10):** Parallel hash/merge joins,
WCOJ for cyclic queries, skew handling, NUMA-aware joins, runtime filters.
Grace/hybrid hash join for out-of-memory joins. Radix hash join for analytics.

**RA status:** 18 join algorithm rules, 9 join reordering rules.

**Gaps identified:**
- No "interesting orderings" framework (System R concept)
- No runtime filter generation (bloom filters from hash join build)
- No skew detection and handling for hash joins
- No parallel hash join variant selection (shared vs partitioned vs radix)
- No WCOJ cost model or automatic cycle detection
- No self-join elimination
- No outer-to-inner join conversion (LEFT -> INNER when WHERE rejects NULLs)
- No full outer join reduction (FULL -> LEFT/RIGHT/INNER)
- No Grace/hybrid hash join modeling for out-of-memory
- No filter null join keys optimization

### 5. Execution Techniques

**What CMU teaches (15-445 L13-14, 15-721 L6-8):** Vectorized execution, query
compilation, morsel-driven parallelism, pipeline fusion. Three execution models:
iterator, materialization, vectorized.

**RA status:** 99 execution model rules, broadly covering categories.

**Gaps identified:**
- No pipeline breaker analysis (annotate blocking vs pipelined operators)
- No pipeline fusion rules (merge adjacent pipelined operators)
- No adaptive batch size for vectorized execution
- No exchange operator placement optimization
- No compilation threshold decision rules
- No adaptive compilation (interpret vs JIT based on execution time)
- No worker allocation strategy rules

### 6. Adaptive Query Processing

**What CMU researches and Databricks/Spark implements:** Adaptive Query Execution (AQE),
sideways information passing, runtime re-optimization, adaptive join switching.

**RA status:** 11 + 13 adaptive rules (execution-models and experimental).

**Gaps identified:**
- No sideways information passing infrastructure
- No mid-query re-optimization at materialization points
- No runtime algorithm switching (sort-merge -> broadcast join)
- No execution feedback for cost model correction
- No bloom filter pushdown from joins to scans
- No post-shuffle partition coalescing
- No skew-aware join partitioning
- No dynamic partition pruning (Spark/Snowflake-style)

### 7. PostgreSQL-Specific Optimizations

**What PostgreSQL implements (v13-18):** Self-join elimination, incremental sort, memoize,
partition pruning, partitionwise join/aggregation, DISTINCT/GROUP BY reordering, presorted
aggregate, Top-N sort.

**RA status:** Only 2 PostgreSQL-specific rules. Core algebra now has IncrementalSort.

**Gaps identified:**
- No self-join elimination
- No memoize/caching for parameterized scans
- No runtime partition pruning
- No partitionwise join
- No partitionwise aggregation
- No DISTINCT key reordering
- No GROUP BY key reordering
- No presorted aggregate
- No Top-N sort (Sort + Limit -> heap sort)

### 8. Modern Index Techniques

**What CMU teaches (15-445 L8-9, 15-721 L4):** Zone maps, bitmap indexes, bloom filters,
learned indexes, covering indexes, partial indexes, index skip scan.

**RA status:** 36 index selection rules. Has BitmapIndexScan/BitmapAnd/BitmapOr/BitmapHeapScan
and IndexOnlyScan (recently added).

**Gaps identified:**
- No zone map / min-max index utilization rules
- No bloom filter index rules
- No partial index matching (query predicate implies index predicate)
- No selectivity-based access path selection (seq vs index vs bitmap threshold)
- No index-provides-ordering detection (eliminate Sort when index sorts)

### 9. Production System Techniques (from CMU system analyses)

**What DuckDB, Snowflake, Databricks, Redshift implement:**

**Gaps identified:**
- No empty result propagation (short-circuit when input provably empty)
- No cross join to inner conversion (implicit join predicates in WHERE)
- No dictionary-aware filtering (filter on dictionary codes for compressed data)
- No constant column elimination (single-value column -> constant)
- No distribution-aware join planning (co-located vs broadcast vs redistribute)
- No late materialization rule (evaluate predicates before materializing rows)
- No micro-partition pruning (file-level pruning from column metadata)
- No result caching at subquery level

### 10. DataFusion/Calcite Common Rules

**What DataFusion and Calcite implement but Ra may lack:**

**Gaps identified:**
- No filter null join keys (add IS NOT NULL for non-nullable join inputs)
- No single distinct to group by conversion
- No group by constant elimination
- No nested union flattening
- No extract equijoin predicate (separate = from non-= in join conditions)

## Comprehensive New Rule Ideas (15 Families, 47 Individual Rules)

### Family 1: Join Transformation Rules
1. Self-join elimination
2. Outer-to-inner join conversion (LEFT/RIGHT -> INNER)
3. Full outer join reduction (FULL -> LEFT/RIGHT/INNER)
4. Cross join to inner conversion
5. Filter null join keys

### Family 2: Sort Optimization Rules
6. GROUP BY key reordering (match input ordering)
7. DISTINCT key reordering (match input ordering)
8. Presorted aggregate (provide sorted input to ORDER BY/DISTINCT aggregates)
9. Top-N sort (Sort + Limit -> heap-based top-N)

### Family 3: Access Path Selection Rules
10. Selectivity-based access path selection (seq vs index vs bitmap thresholds)
11. Partial index matching (query implies index predicate)
12. Index-provides-ordering detection

### Family 4: Cardinality Estimation Rules
13. MCV-aware selectivity estimation
14. Semi-join / anti-join cardinality formulas
15. Combined MCV + histogram estimation
16. Estimation error safety margin for deep join trees

### Family 5: Cost Model Enhancement Rules
17. Correlation-aware index scan cost
18. Cache-aware random I/O costing
19. Memory spill threshold modeling
20. Compression-aware cost adjustment

### Family 6: Execution Optimization Rules
21. Empty result propagation
22. Constant column elimination
23. Pipeline breaker annotation
24. Exchange operator placement

### Family 7: Distributed / Partitioned Rules
25. Partitionwise join
26. Partitionwise aggregation
27. Distribution-aware join planning
28. Micro-partition pruning

### Family 8: Runtime Filter Rules
29. Hash join bloom filter generation
30. Bloom filter pushdown to scan
31. Semi-join reduction insertion
32. Dynamic partition pruning

### Family 9: Caching / Memoization Rules
33. Memoize for parameterized nested loop scans
34. Result cache matching for repeated subqueries

### Family 10: Adaptive Execution Rules
35. Mid-query re-optimization at materialization points
36. Adaptive join strategy switching
37. Post-shuffle partition coalescing
38. Skew-aware join partitioning

### Family 11: Statistics Improvement Rules
39. Statistics staleness detection
40. Correlation detection advisor
41. Extended statistics recommendation
42. Function-aware cardinality estimation

### Family 12: Expression Optimization Rules
43. Group by constant elimination
44. Single distinct to group by conversion
45. Nested union flattening
46. Extract equijoin predicate separation

### Family 13: Physical Property Rules
47. Enforcer sort insertion
48. Enforcer exchange insertion
49. Property-aware cost extraction

## Prioritized Recommendations

### Critical (blocks other optimizations)
1. Physical property tracking (ordering, partitioning) -- RFC Proposal 1
2. Outer-to-inner join conversion -- RFC Proposal 5

### High Priority (most production impact)
3. Self-join elimination -- RFC Proposal 5
4. Runtime filter generation (bloom filters) -- RFC Proposal 3
5. Top-N sort (Sort + Limit -> heap sort)
6. Empty result propagation
7. GROUP BY / DISTINCT key reordering -- RFC Proposal 4
8. Correlation-aware index scan cost

### Medium Priority (common scenarios)
9. Memoize for parameterized scans
10. Partitionwise join/aggregation
11. MCV-aware selectivity estimation
12. Partial index matching
13. Cross join to inner conversion
14. Filter null join keys

### Lower Priority (advanced/specialized)
15. Mid-query re-optimization
16. Compression-aware processing
17. Skew-aware join partitioning
18. Distribution-aware join planning

## Source Summary

| Source Category | Count | Key Topics |
|----------------|-------|-----------|
| CMU 15-445 lectures | 8 | Core optimization, joins, execution, sorting, indexes |
| CMU 15-721 lectures | 12 | Advanced optimization, compilation, parallelism, cost models |
| CMU seminars (2022-24) | 8 | System-specific optimizers (StarRocks, MariaDB, DataFusion, etc.) |
| CMU research projects | 3 | Adaptive processing, hardware co-design, UDF compilation |
| Production systems | 4 | DuckDB, Snowflake, Databricks/Spark, Redshift |
| PostgreSQL documentation | 10 | Planner internals, statistics, cost formulas, new features |
| Robert Haas content | 2 | pg_plan_advice, plan stability |
| Research papers | 3 | Leis 2015 (cardinality errors), Selinger 1979, Graefe 1995 |
| DataFusion source | 2 | Optimizer rules, physical planning |
| BusTub source | 2 | Teaching optimizer, minimum viable rules |
| **Total** | **54** | |
