# PostgreSQL Planner Features Missing from RA

**Date:** 2026-03-21 (updated from 2026-03-20)
**Comparison basis:** PostgreSQL 18 planner vs RA optimizer (969 rules)

## Overview

PostgreSQL's planner represents decades of production optimization engineering. While RA
has mined rules from PostgreSQL and other databases, many of PostgreSQL's core planning
mechanisms are not represented as rules in RA. This document catalogs what PostgreSQL
does that RA does not.

**Update:** Expanded from 29 gaps to 37 gaps based on deeper analysis of PostgreSQL
runtime configuration, cardinality estimation formulas, and recent feature additions.

## Category 1: Planning Architecture

### 1.1 Bottom-Up Dynamic Programming
**PostgreSQL:** Enumerates join orderings via DP for < 12 tables, tracking cost and
interesting orderings at each combination level.
**RA:** Uses egg e-graph equality saturation. Different approach, but lacks the
systematic enumeration guarantees of DP.
**Gap severity:** Medium - e-graphs handle this differently but need fallback for edge cases.

### 1.2 Genetic Query Optimizer (GEQO)
**PostgreSQL:** Switches to genetic algorithm for 12+ table joins.
**RA:** Has large join graph fallback (RFC 0017, implemented). Check status.
**Gap severity:** Low - addressed by recent implementation.

### 1.3 Physical Property Tracking (Pathkeys)
**PostgreSQL:** Tracks sort ordering (pathkeys) through every plan node. Propagates
required orderings top-down and available orderings bottom-up.
**RA:** Has PhysicalProperties struct in ra-core but no integration with e-graph extraction.
**Gap severity:** Critical - this is fundamental to avoiding redundant sorts and choosing
optimal join methods. Blocks GROUP BY reordering, incremental sort, and merge join optimization.

### 1.4 Interesting Orderings
**PostgreSQL:** Retains plans that are not cheapest overall but have useful sort orderings
for downstream operations (ORDER BY, GROUP BY, merge join).
**RA:** No concept of interesting orderings in cost extraction.
**Gap severity:** Critical - may miss plans that are globally optimal due to ordering reuse.

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
in physical order.
**RA:** Has BitmapIndexScan, BitmapAnd, BitmapOr, BitmapHeapScan operators (recently added).
**Gap severity:** Low - operators exist. Need rules for automatic selection.

### 2.2 Index-Only Scan
**PostgreSQL:** When index covers all needed columns, never accesses heap table.
**RA:** Has IndexOnlyScan operator (recently added).
**Gap severity:** Low - operator exists.

### 2.3 Partial Index Matching
**PostgreSQL:** Matches query predicates to partial index WHERE clauses. Uses partial
indexes when query implies the index predicate. Limited theorem proving (simple
inequality implication, exact match).
**RA:** No partial index matching rules.
**Gap severity:** Medium - common in production schemas (e.g., index WHERE status='active').

### 2.4 Index Skip Scan
**PostgreSQL (v18):** Skips leading index column values to use multi-column index
for non-prefix queries.
**RA:** Has index-skip-scan.rra rule.
**Gap severity:** Low - rule exists.

### 2.5 Selectivity-Based Access Path Selection
**PostgreSQL:** Chooses SeqScan vs IndexScan vs BitmapScan based on selectivity
thresholds: < ~1% IndexScan, 1-20% BitmapScan, > 20% SeqScan. Thresholds
adjusted by correlation.
**RA:** No selectivity-threshold-based access path selection.
**Gap severity:** High - fundamental to choosing the right scan strategy.

### 2.6 Index-Provides-Ordering
**PostgreSQL:** When ORDER BY matches index sort order, eliminates Sort node.
**RA:** Limited - has some rules but not systematic detection.
**Gap severity:** Medium - eliminates full sort operations.

## Category 3: Join Processing

### 3.1 Self-Join Elimination
**PostgreSQL (v17):** Detects and eliminates self-joins, replacing with single scan.
Conditions: join on unique/primary key, one side's columns subset of other.
**RA:** No self-join elimination rules.
**Gap severity:** High - common pattern in ORM-generated queries.

### 3.2 Outer-to-Inner Join Conversion
**PostgreSQL:** Converts LEFT JOIN to INNER JOIN when WHERE clause makes outer rows
impossible. Detects null-rejecting predicates (equality, comparison, IS NOT NULL,
strict functions). Cascading: converting one join may enable converting others.
**RA:** Limited - has some join elimination but not systematic conversion.
**Gap severity:** Critical - enables join reordering that was previously blocked.

### 3.3 Full Outer Join Reduction
**PostgreSQL:** FULL OUTER -> LEFT/RIGHT/INNER based on WHERE clause null rejection
on one or both sides.
**RA:** No full outer join reduction.
**Gap severity:** Medium.

### 3.4 Parameterized Nested Loop
**PostgreSQL:** Inner side of nested loop uses outer values as index scan parameters.
**RA:** Limited modeling of parameterized scans.
**Gap severity:** Medium - key for index nested loop joins.

### 3.5 Memoize Node
**PostgreSQL (v14):** Caches results of parameterized inner scans using LRU cache.
When outer side has repeated join key values, avoids redundant inner executions.
Cost model: `hit_ratio = 1 - (n_distinct / outer_rows)`.
**RA:** No memoize optimization rules.
**Gap severity:** High - significant speedup for skewed joins (10-1000x).

### 3.6 Filter Null Join Keys
**PostgreSQL / DataFusion:** For inner joins, add IS NOT NULL filter on join key
columns before the join. Removes null rows early.
**RA:** No filter null join keys rule.
**Gap severity:** Low - small optimization but easy to implement.

## Category 4: Sort and Aggregation

### 4.1 Incremental Sort
**PostgreSQL (v13):** When data partially sorted on prefix of required sort key,
only sort within each prefix group. O(n log m) vs O(n log n).
**RA:** Has IncrementalSort operator in algebra. Need selection rules.
**Gap severity:** Medium - operator exists, need rules for automatic use.

### 4.2 GROUP BY Key Reordering
**PostgreSQL (v16):** Reorder GROUP BY keys to match available sort ordering from
child nodes. Any permutation of GROUP BY keys produces same result.
**RA:** No GROUP BY reordering rules.
**Gap severity:** High - avoids unnecessary sorts for common GROUP BY patterns.

### 4.3 DISTINCT Key Reordering
**PostgreSQL (v16):** Reorder DISTINCT keys to match input pathkeys.
**RA:** No DISTINCT key reordering rules.
**Gap severity:** Medium - similar benefit to GROUP BY reordering.

### 4.4 Presorted Aggregate
**PostgreSQL (v16):** Provide presorted rows for ORDER BY/DISTINCT within aggregates.
**RA:** No presorted aggregate rules.
**Gap severity:** Low - specialized but useful for array_agg(x ORDER BY x).

### 4.5 Top-N Sort (ORDER BY + LIMIT)
**PostgreSQL:** Heap sort for top-N queries instead of full sort. O(n log k) for LIMIT k.
BusTub implements this as `sort_limit_as_topn`.
**RA:** Has limit-pushdown rules but no specific Top-N sort algorithm selection.
**Gap severity:** High - very common query pattern, easy to implement.

### 4.6 Empty Result Propagation
**PostgreSQL / DuckDB / DataFusion:** When any input to an operator is provably empty
(WHERE false, contradictory predicates, empty table), short-circuit the entire subtree.
**RA:** No empty result propagation rules.
**Gap severity:** Medium - eliminates unnecessary computation.

## Category 5: Partitioning

### 5.1 Partition Pruning (Static)
**PostgreSQL:** Eliminate non-matching partitions at plan time based on constant predicates.
**RA:** Has distributed/partition-pruning/ (5 rules).
**Gap severity:** Low - basic rules exist.

### 5.2 Partition Pruning (Runtime)
**PostgreSQL:** Eliminate partitions at execution time using parameter values not known
during planning.
**RA:** No runtime partition pruning.
**Gap severity:** Medium - important for prepared statements and subquery-derived filters.

### 5.3 Partitionwise Join
**PostgreSQL:** Join matching partitions separately when both tables partitioned on
join key. Reduces memory and enables parallelism.
**RA:** No partitionwise join rules.
**Gap severity:** Medium - important for large partitioned tables.

### 5.4 Partitionwise Aggregation
**PostgreSQL:** Aggregate within each partition, then combine.
**RA:** Has distributed/partial-aggregation/ but not partition-specific.
**Gap severity:** Medium - important for partitioned analytics.

## Category 6: Cost Estimation

### 6.1 Correlation-Aware Index Cost
**PostgreSQL:** Uses pg_stats.correlation to estimate whether index scan produces
sequential or random I/O. Cost formula:
`random_page_cost * pages * (1 - correlation^2) + seq_page_cost * pages * correlation^2`
**RA:** No correlation-based cost adjustment.
**Gap severity:** Critical - major factor in index vs sequential scan decisions.

### 6.2 Effective Cache Size
**PostgreSQL:** effective_cache_size influences cost of random page access.
Larger cache -> more likely pages are cached -> lower effective random I/O cost.
**RA:** No cache-aware cost modeling.
**Gap severity:** Medium - affects plan selection for medium-selectivity queries.

### 6.3 Memory Spill Threshold
**PostgreSQL / DuckDB:** When hash table or sort buffer exceeds work_mem, operator
spills to disk. Cost model must account for 2x I/O (write + read-back).
**RA:** No spill-to-disk cost modeling.
**Gap severity:** Medium - affects hash join and sort cost accuracy.

## Category 7: Statistics and Estimation

### 7.1 Most Common Values (MCV) Lists
**PostgreSQL:** Maintains per-column MCV list with frequencies. Used for:
- Equality: selectivity = freq[value]
- Join: direct MCV comparison between columns
- Combined with histogram for range predicates
**RA:** Has histograms but no MCV lists in statistics model.
**Gap severity:** High - MCV is the most important statistic after row count.

### 7.2 Extended Statistics
**PostgreSQL (v14):** Statistics on multi-column combinations and expressions:
- Functional dependencies (A -> B)
- Multivariate NDV
- Multivariate MCV
- Expression statistics
**RA:** No extended statistics modeling.
**Gap severity:** Medium - important for correlated columns.

### 7.3 Statistics Target Tuning
**PostgreSQL:** Per-column statistics target (histogram buckets / MCV entries).
**RA:** No advisor for optimal statistics target per column.
**Gap severity:** Low - manual tuning is usually sufficient.

### 7.4 Semi-Join / Anti-Join Cardinality
**PostgreSQL:** Specific formulas for EXISTS and NOT EXISTS:
- Semi-join: `sel = 1 - (1 - 1/ndv_inner)^n_outer`
- Anti-join: complement of semi-join selectivity
**RA:** No semi-join / anti-join specific cardinality formulas.
**Gap severity:** Medium - EXISTS/NOT EXISTS are common patterns.

## Summary

| Category | Gaps Found | Critical | High | Medium | Low |
|----------|-----------|----------|------|--------|-----|
| Planning Architecture | 5 | 2 | 0 | 2 | 1 |
| Access Path Selection | 6 | 0 | 1 | 2 | 3 |
| Join Processing | 6 | 1 | 2 | 2 | 1 |
| Sort and Aggregation | 6 | 0 | 2 | 2 | 2 |
| Partitioning | 4 | 0 | 0 | 3 | 1 |
| Cost Estimation | 3 | 1 | 0 | 2 | 0 |
| Statistics | 4 | 0 | 1 | 2 | 1 |
| **Total** | **37** | **4** | **6** | **15** | **9** |

## Top 15 Missing Features (by impact, updated)

1. Physical property tracking (pathkeys/ordering) -- CRITICAL
2. Outer-to-inner join conversion -- CRITICAL
3. Correlation-aware index scan cost -- CRITICAL
4. Interesting orderings framework -- CRITICAL
5. Self-join elimination -- HIGH
6. Top-N sort (Sort + LIMIT -> heap sort) -- HIGH
7. Memoize for parameterized scans -- HIGH
8. GROUP BY key reordering -- HIGH
9. Selectivity-based access path selection -- HIGH
10. MCV lists in statistics model -- HIGH
11. Partitionwise join -- MEDIUM
12. Empty result propagation -- MEDIUM
13. Partial index matching -- MEDIUM
14. Runtime partition pruning -- MEDIUM
15. Extended statistics support -- MEDIUM
