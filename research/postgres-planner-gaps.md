# PostgreSQL Planner Features Missing from RA

**Date:** 2026-03-20
**Comparison basis:** PostgreSQL 18 planner vs RA optimizer (969 rules)

## Overview

PostgreSQL's planner represents decades of production optimization engineering. While RA
has mined rules from PostgreSQL and other databases, many of PostgreSQL's core planning
mechanisms are not represented as rules in RA. This document catalogs what PostgreSQL
does that RA does not.

## Category 1: Planning Architecture

### 1.1 Bottom-Up Dynamic Programming
**PostgreSQL:** Enumerates join orderings via DP for < 12 tables, tracking cost and
interesting orderings at each combination level.
**RA:** Uses egg e-graph equality saturation. Different approach, but lacks the
systematic enumeration guarantees of DP.
**Gap severity:** Medium - e-graphs handle this differently but need fallback for edge cases.

### 1.2 Genetic Query Optimizer (GEQO)
**PostgreSQL:** Switches to genetic algorithm for 12+ table joins.
**RA:** No fallback for large join graphs when e-graph saturation is too expensive.
**Gap severity:** High - large join queries may timeout without a heuristic fallback.

### 1.3 Physical Property Tracking (Pathkeys)
**PostgreSQL:** Tracks sort ordering (pathkeys) through every plan node. Propagates
required orderings top-down and available orderings bottom-up.
**RA:** No physical property tracking system.
**Gap severity:** High - this is fundamental to avoiding redundant sorts and choosing
optimal join methods.

### 1.4 Interesting Orderings
**PostgreSQL:** Retains plans that are not cheapest overall but have useful sort orderings
for downstream operations (ORDER BY, GROUP BY, merge join).
**RA:** No concept of interesting orderings in cost extraction.
**Gap severity:** High - may miss plans that are globally optimal due to ordering reuse.

### 1.5 Preprocessing / Query Rewriting
**PostgreSQL:** Before cost-based optimization, applies:
- Subquery flattening (FROM subqueries to joins)
- Outer-to-inner join conversion
- CTE inlining (PostgreSQL 12+)
- Constraint exclusion
**RA:** Has some CTE and subquery rules but not the systematic preprocessing phase.
**Gap severity:** Medium - some rules exist but not organized as preprocessing.

## Category 2: Access Path Selection

### 2.1 Bitmap Scan
**PostgreSQL:** Combines multiple indexes via bitmap AND/OR, then accesses heap pages
in physical order. Critical for multi-predicate queries where no single index suffices.
**RA:** No bitmap scan modeling.
**Gap severity:** Medium - important for real-world multi-predicate queries.

### 2.2 Index-Only Scan
**PostgreSQL:** When index covers all needed columns, never accesses heap table.
Checks visibility map for visibility.
**RA:** Has covering-index-scan.rra but may lack visibility map cost modeling.
**Gap severity:** Low - basic rule exists.

### 2.3 Partial Index Matching
**PostgreSQL:** Matches query predicates to partial index WHERE clauses. Uses partial
indexes when query implies the index predicate.
**RA:** No partial index matching rules.
**Gap severity:** Medium - common in production schemas.

### 2.4 Index Skip Scan
**PostgreSQL (v18):** Skips leading index column values to use multi-column index
for non-prefix queries.
**RA:** Has index-skip-scan.rra rule.
**Gap severity:** Low - rule exists.

### 2.5 TID Scan
**PostgreSQL:** Direct tuple access by physical TID for ctid-based queries.
**RA:** No TID scan modeling.
**Gap severity:** Low - rare in practice.

## Category 3: Join Processing

### 3.1 Self-Join Elimination
**PostgreSQL (v17):** Detects and eliminates self-joins, replacing with single scan.
**RA:** No self-join elimination rules.
**Gap severity:** Medium - common pattern in generated queries.

### 3.2 Outer-to-Inner Join Conversion
**PostgreSQL:** Converts LEFT JOIN to INNER JOIN when WHERE clause makes outer rows
impossible (e.g., WHERE right.col IS NOT NULL).
**RA:** Limited - has some join elimination but not systematic conversion.
**Gap severity:** High - significant optimization opportunity.

### 3.3 Parameterized Nested Loop
**PostgreSQL:** Inner side of nested loop uses outer values as index scan parameters.
PlannerInfo tracks parameterization paths.
**RA:** Limited modeling of parameterized scans.
**Gap severity:** Medium - key for index nested loop joins.

### 3.4 Memoize Node
**PostgreSQL (v14):** Caches results of parameterized inner scans. When outer side
has repeated values, avoids redundant inner executions.
**RA:** No memoize optimization rules.
**Gap severity:** Medium - significant speedup for skewed joins.

## Category 4: Sort and Aggregation

### 4.1 Incremental Sort
**PostgreSQL (v13):** When data partially sorted on prefix of required sort key,
only sort within each prefix group. O(n log m) vs O(n log n).
**RA:** No incremental sort rules.
**Gap severity:** High - common optimization opportunity.

### 4.2 GROUP BY Key Reordering
**PostgreSQL (v16):** Reorder GROUP BY keys to match available sort ordering from
child nodes. Avoids unnecessary re-sort.
**RA:** No GROUP BY reordering rules.
**Gap severity:** Medium - avoids unnecessary sorts.

### 4.3 DISTINCT Key Reordering
**PostgreSQL (v16):** Reorder DISTINCT keys to match input pathkeys.
**RA:** No DISTINCT key reordering rules.
**Gap severity:** Medium - similar benefit to GROUP BY reordering.

### 4.4 Presorted Aggregate
**PostgreSQL (v16):** Provide presorted rows for ORDER BY/DISTINCT within aggregates.
**RA:** No presorted aggregate rules.
**Gap severity:** Low - specialized optimization.

### 4.5 Top-N Sort (ORDER BY + LIMIT)
**PostgreSQL:** Heap sort for top-N queries instead of full sort. O(n log k) for LIMIT k.
**RA:** Has limit-pushdown rules but no specific Top-N sort algorithm selection.
**Gap severity:** Medium - common query pattern.

## Category 5: Partitioning

### 5.1 Partition Pruning (Static)
**PostgreSQL:** Eliminate non-matching partitions at plan time based on constant predicates.
**RA:** Has distributed/partition-pruning/ (5 rules).
**Gap severity:** Low - basic rules exist.

### 5.2 Partition Pruning (Runtime)
**PostgreSQL:** Eliminate partitions at execution time using parameter values not known
during planning (e.g., prepared statement parameters).
**RA:** No runtime partition pruning.
**Gap severity:** Medium - important for parameterized queries.

### 5.3 Partitionwise Join
**PostgreSQL:** Join matching partitions separately when both tables partitioned on
join key. Reduces memory and enables local optimization.
**RA:** No partitionwise join rules.
**Gap severity:** Medium - important for large partitioned tables.

### 5.4 Partitionwise Aggregation
**PostgreSQL:** Aggregate within each partition, then combine. Two-phase approach.
**RA:** Has distributed/partial-aggregation/ but not partition-specific.
**Gap severity:** Medium - important for partitioned analytics.

## Category 6: Cost Estimation

### 6.1 Startup vs Total Cost
**PostgreSQL:** Every path has both startup and total cost. Startup cost matters for
LIMIT queries and cursor-based access.
**RA:** No startup/total cost distinction.
**Gap severity:** High - affects LIMIT optimization and pipelining decisions.

### 6.2 Correlation-Aware Index Cost
**PostgreSQL:** Uses pg_stats.correlation to estimate whether index scan produces
sequential or random I/O. High correlation -> low cost.
**RA:** No correlation-based cost adjustment.
**Gap severity:** High - major factor in index vs sequential scan decisions.

### 6.3 Effective Cache Size
**PostgreSQL:** effective_cache_size parameter influences cost of random page access.
Larger cache -> more likely pages are cached -> lower random_page_cost effective rate.
**RA:** No cache-aware cost modeling at this level.
**Gap severity:** Medium - affects plan selection for medium-selectivity queries.

## Category 7: Statistics

### 7.1 Extended Statistics Advisor
**PostgreSQL:** Supports functional dependencies, multivariate NDV, multivariate MCV
but requires manual CREATE STATISTICS.
**RA:** No automatic recommendation of which extended statistics to create.
**Gap severity:** Medium - would help users get better plans.

### 7.2 Statistics Target Tuning
**PostgreSQL:** Per-column statistics target (histogram buckets / MCV entries).
**RA:** No advisor for optimal statistics target per column.
**Gap severity:** Low - manual tuning is usually sufficient.

### 7.3 Expression Statistics
**PostgreSQL (v14):** Statistics on expressions, not just columns. CREATE STATISTICS
on arbitrary expressions.
**RA:** No expression statistics modeling.
**Gap severity:** Medium - useful for computed predicates.

## Summary

| Category | Gaps Found | High Severity | Medium | Low |
|----------|-----------|---------------|--------|-----|
| Planning Architecture | 5 | 3 | 2 | 0 |
| Access Path Selection | 5 | 0 | 3 | 2 |
| Join Processing | 4 | 1 | 3 | 0 |
| Sort and Aggregation | 5 | 1 | 3 | 1 |
| Partitioning | 4 | 0 | 3 | 1 |
| Cost Estimation | 3 | 2 | 1 | 0 |
| Statistics | 3 | 0 | 2 | 1 |
| **Total** | **29** | **7** | **17** | **5** |

## Top 10 Missing Features (by impact)

1. Physical property tracking (pathkeys/ordering)
2. Interesting orderings framework
3. Startup vs total cost distinction
4. Outer-to-inner join conversion
5. Incremental sort
6. GEQO-equivalent fallback for large queries
7. Correlation-aware index scan cost
8. Self-join elimination
9. Runtime filter generation (bloom filters)
10. Memoize for parameterized scans
