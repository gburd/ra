# CMU Database Group Research Analysis for RA

**Date:** 2026-03-20
**Scope:** CMU 15-445/645, 15-721, Database Group seminars, research projects
**Total sources analyzed:** 25+ lectures, talks, and research projects

## Executive Summary

The CMU Database Group represents the leading academic authority on query optimization
and database systems. Analysis of their course content (15-445, 15-721), seminar series,
and research projects reveals significant gaps in RA's optimization coverage, particularly
in adaptive query processing, cost model calibration, physical property tracking, and
modern execution techniques.

RA has 969 rules across 84 directories, which is substantial. However, the rules are
predominantly focused on logical transformations and lack several categories that
production optimizers consider essential.

## Key Findings by Category

### 1. Optimizer Architecture

**What CMU teaches:** Two dominant frameworks (System R bottom-up, Cascades top-down)
plus equality saturation as a third approach. Production systems use hybrid approaches.

**RA status:** Uses egg e-graph (equality saturation). This is a valid and modern approach
but lacks some capabilities of traditional frameworks.

**Gaps identified:**
- No physical property tracking (ordering, partitioning, distribution)
- No enforcer rules (add Sort/Exchange when properties needed)
- No branch-and-bound pruning during search
- No multi-phase optimization (heuristic then cost-based)
- No fallback for e-graph saturation timeout on large queries

### 2. Cost Models

**What CMU teaches (15-721 Lecture 18):** Three-component cost models (CPU, I/O, memory),
hardware calibration, startup vs total cost distinction, correlation-aware index costs.

**RA status:** 38 cost model rules exist but are generic and formula-oriented.

**Gaps identified:**
- No startup vs total cost distinction (important for pipelining and LIMIT)
- No correlation-aware index scan cost
- No hash build vs probe cost separation
- No automatic calibration from execution feedback
- No memory pressure modeling (spill to disk thresholds)
- No cost model for string operations and LIKE patterns
- No compression-aware cost adjustments

### 3. Cardinality Estimation

**What CMU teaches:** Histograms, MCVs, sketches, sampling, multi-column statistics.
Estimation errors compound exponentially through joins (Leis et al. 2015).

**RA status:** Rules exist for basic estimation but lack advanced techniques.

**Gaps identified:**
- No multi-column correlation detection/recommendation
- No sketch-based estimation (Count-Min, HyperLogLog)
- No function-aware cardinality (date_trunc, int4mod reduce cardinality)
- No estimation error detection and re-optimization
- No statistics staleness detection
- No histogram type selection (equi-width vs equi-depth)
- No statistics propagation through operator trees

### 4. Join Optimization

**What CMU teaches (15-445 L11, 15-721 L11-13):** Parallel hash/merge joins,
WCOJ for cyclic queries, skew handling, NUMA-aware joins, runtime filters.

**RA status:** 18 join algorithm rules, 9 join reordering rules.

**Gaps identified:**
- No "interesting orderings" framework (System R concept)
- No runtime filter generation (bloom filters from hash join build)
- No skew detection and handling
- No parallel hash join variant selection
- No WCOJ cost model or automatic cycle detection
- No self-join elimination
- No outer-to-inner join conversion

### 5. Execution Techniques

**What CMU teaches (15-721 L6-10):** Vectorized execution, query compilation,
morsel-driven parallelism, pipeline fusion.

**RA status:** 99 execution model rules, broadly covering categories.

**Gaps identified:**
- No pipeline breaker analysis
- No pipeline fusion rules
- No adaptive batch size for vectorized execution
- No exchange operator placement optimization
- No compilation threshold decision rules
- No adaptive compilation (interpret vs JIT)

### 6. Adaptive Query Processing

**What CMU researches:** Sideways information passing, runtime re-optimization,
adaptive join switching, ML-based adaptation.

**RA status:** 11 + 13 adaptive rules (execution-models and experimental).

**Gaps identified:**
- No sideways information passing infrastructure
- No mid-query re-optimization
- No runtime algorithm switching
- No execution feedback for cost model correction
- No bloom filter pushdown from joins to scans

### 7. PostgreSQL-Specific Optimizations

**What PostgreSQL implements:** Self-join elimination, incremental sort, memoize,
partition pruning, partitionwise join/aggregation, DISTINCT/GROUP BY reordering.

**RA status:** Only 2 PostgreSQL-specific rules.

**Gaps identified:**
- No self-join elimination
- No incremental sort optimization
- No memoize/caching for parameterized scans
- No runtime partition pruning
- No partitionwise join/aggregation
- No DISTINCT key reordering
- No GROUP BY key reordering

### 8. Modern Index Techniques

**What CMU teaches (15-721 L4):** Zone maps, bitmap indexes, bloom filters,
learned indexes, covering indexes.

**RA status:** 36 index selection rules.

**Gaps identified:**
- No zone map / min-max index utilization
- No bitmap index combination rules
- No bloom filter index rules
- No covering index detection and promotion
- No index skip scan modeling
- No partial index matching

## Prioritized Recommendations

### High Priority (address fundamental optimizer capabilities)
1. Physical property tracking (ordering, partitioning)
2. Interesting orderings framework
3. Startup vs total cost distinction
4. Self-join elimination
5. Outer-to-inner join conversion

### Medium Priority (improve plan quality for common cases)
6. Runtime filter generation (bloom filters)
7. Incremental sort
8. Memoize for parameterized scans
9. Partition pruning (runtime)
10. Statistics staleness detection

### Lower Priority (advanced/specialized optimizations)
11. WCOJ cost model and cycle detection
12. Adaptive query processing infrastructure
13. Compression-aware query processing
14. Learned cost models
15. Pipeline fusion optimization

## Source Summary

| Source Category | Count | Key Topics |
|----------------|-------|-----------|
| CMU 15-445 lectures | 6 | Core optimization, joins, execution, sorting |
| CMU 15-721 lectures | 8 | Advanced optimization, compilation, parallelism |
| CMU seminars | 5 | System-specific optimizers (StarRocks, MariaDB, etc.) |
| CMU research projects | 3 | Adaptive processing, hardware co-design, UDF compilation |
| PostgreSQL documentation | 6 | Planner internals, statistics, cost formulas |
| Robert Haas content | 2 | pg_plan_advice, plan stability |
