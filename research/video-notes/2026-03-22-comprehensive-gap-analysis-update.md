# Comprehensive Gap Analysis Update: CMU Lecture Mining Phase 2

**Date:** 2026-03-22
**Sources analyzed in this phase:** 10 new research notes covering CMU 15-721 Spring 2024
lectures and production system optimizer comparisons
**Total sources analyzed to date:** 64 (54 previous + 10 new)

## Summary of New Findings

This phase focused on areas not well-covered in the initial research (2026-03-20/21):
1. UDF optimization (Froid, Aggify)
2. Robust/adaptive optimization (Plan Stitch, progressive re-optimization)
3. Data format encoding optimization (FastLanes, BtrBlocks, BitWeaving)
4. Scheduling and parallelism (morsel-driven, exchange placement)
5. Comprehensive optimizer taxonomy (Chaudhuri survey)
6. Advanced subquery unnesting (Neumann-Kemper framework)
7. Cloud system techniques (Dremel/BigQuery, Yellowbrick, Redshift)
8. Execution engine optimization (Velox, SIMD vectorization)
9. Network protocol optimization
10. Cross-system rule comparison (DataFusion, Calcite, BusTub)

## New Optimization Rules Identified (Not in Previous Research)

### Confirmed Missing from Ra (High Confidence)

1. **extract-equijoin-predicate** - Separate equality from non-equality join predicates
   to enable hash/merge join selection. Present in both DataFusion and Calcite.

2. **filter-null-join-keys** - Add IS NOT NULL on join key columns before join.
   NULL keys never match in equi-joins. Present in DataFusion.

3. **propagate-empty-relation** - Short-circuit computation when input is provably empty.
   Present in both DataFusion and Calcite.

4. **dictionary-predicate-rewrite** - Evaluate string predicates on dictionary codes
   instead of materialized values. Standard in DuckDB, Velox, Snowflake.

5. **late-materialization-insertion** - Defer column loading until after selective
   predicates have been evaluated. Standard in all columnar engines.

6. **adaptive-filter-reordering** - Reorder conjunctive predicates by observed
   selectivity at runtime. Present in Velox, DuckDB.

7. **apply-through-aggregate** - Push Apply/dependent-join through GroupBy by extending
   grouping key set. From Neumann-Kemper 2015 decorrelation framework.

8. **correlated-subquery-merge** - Merge multiple correlated subqueries accessing same
   table into a single decorrelated join. Common in ORM-generated SQL.

9. **materialized-view-rewrite** - Match and rewrite queries to use available materialized
   views. Present in Calcite, Redshift, BigQuery.

10. **multi-level-partial-aggregation** - In tree-structured execution, insert partial
    aggregation at each level to reduce data flow. Standard in Dremel/BigQuery, Spark.

### Previously Identified but Now Better Understood

11. **startup-cost-optimization** - For LIMIT queries, prefer plans with low startup cost.
    Chaudhuri survey highlights this as fundamental to cost-based optimization.

12. **mackert-lohman-index-cost** - Correlation-aware index I/O cost estimation.
    More impactful than initially assessed -- affects every index access path decision.

13. **distribution-key-aware-join-ordering** - In distributed execution, order joins to
    maximize co-located joins. Critical for Redshift, Yellowbrick, any MPP system.

14. **nvme-ssd-cost-adjustment** - Modern NVMe SSDs have much lower random I/O penalty.
    Cost model calibrated for HDDs makes suboptimal access path decisions.

15. **plan-checkpoint-insertion** - Insert monitoring checkpoints at materialization
    boundaries for progressive re-optimization. From Plan Stitch / Markl 2004.

## Updated Priority Matrix

| Rule | Impact | Complexity | Prerequisite | Priority |
|------|--------|-----------|-------------|----------|
| extract-equijoin-predicate | High | Low | None | Critical |
| propagate-empty-relation | High | Low | None | Critical |
| filter-null-join-keys | Medium | Low | None | High |
| dictionary-predicate-rewrite | High | Medium | Format metadata | High |
| late-materialization-insertion | High | Medium | Column access tracking | High |
| materialized-view-rewrite | High | High | MV catalog | High |
| startup-cost-optimization | Medium | Low | Cost model extension | High |
| mackert-lohman-index-cost | Medium | Medium | Column correlation stats | High |
| apply-through-aggregate | Medium | Medium | None | High |
| correlated-subquery-merge | Medium | Medium | None | Medium |
| adaptive-filter-reordering | Medium | Medium | Runtime infrastructure | Medium |
| multi-level-partial-aggregation | Medium | Medium | Tree execution model | Medium |
| distribution-key-aware-join-ordering | High | Medium | Distribution metadata | Medium |
| nvme-ssd-cost-adjustment | Medium | Low | Hardware detection | Medium |
| plan-checkpoint-insertion | Medium | High | Re-optimization infra | Low |

## Proposed New RFCs

Based on this phase's research, I propose 5 new RFCs:

### RFC: Format-Aware Query Optimization
**Covers:** dictionary-predicate-rewrite, late-materialization-insertion,
zone-map-chunk-pruning, compression-aware-cost-model
**Impact:** 2-10x for analytics on columnar data
**Prerequisite:** Storage format metadata in catalog

### RFC: Materialized View Matching and Rewriting
**Covers:** materialized-view-rewrite, automatic MV selection advisor
**Impact:** Orders of magnitude for cached analytical queries
**Prerequisite:** MV catalog, freshness tracking

### RFC: Progressive Re-optimization
**Covers:** plan-checkpoint-insertion, re-optimization-trigger, execution-feedback
**Impact:** Prevents worst-case plan regressions
**Prerequisite:** Execution monitoring infrastructure

### RFC: UDF-Aware Cost Estimation
**Covers:** udf-cost-estimation, udf-inline-scalar (where representable)
**Impact:** Better plans for function-heavy queries
**Prerequisite:** Function metadata in catalog

### RFC: Consensus Missing Rules
**Covers:** extract-equijoin-predicate, filter-null-join-keys, propagate-empty-relation,
startup-cost-optimization
**Impact:** Cumulative correctness and performance improvement
**Prerequisite:** None - these are straightforward additions

## Cross-Reference with Existing RFCs

| New Finding | Related Existing RFC | Action |
|-------------|---------------------|--------|
| dictionary-predicate-rewrite | RFC 0033 (Columnar Format) | Extend RFC 0033 |
| distribution-key-aware-join-ordering | RFC 0006 (Distributed) | Extend RFC 0006 |
| adaptive-filter-reordering | RFC 0023 (Adaptive QE) | Extend RFC 0023 |
| multi-level-partial-aggregation | RFC 0020 (Parallel QE) | Extend RFC 0020 |
| plan-checkpoint-insertion | RFC 0013 (Regression Detection) | New RFC needed |
| materialized-view-rewrite | RFC 0022 (Incremental View) | New RFC needed |
| apply-through-aggregate | None (subquery unnesting) | New rule, no RFC needed |
| extract-equijoin-predicate | None | New rule, no RFC needed |

## Conclusion

This phase of research identified 15 new optimization techniques not in the previous
54 notes. The highest-impact items are:

1. **extract-equijoin-predicate** and **propagate-empty-relation** should be implemented
   immediately as they are consensus rules present in every production optimizer.

2. **Format-aware optimization** (dictionary predicates, late materialization) represents
   the largest performance opportunity for analytical workloads.

3. **Materialized view rewriting** would give Ra a capability that few rule-based
   optimizers outside commercial databases currently offer.

4. **Progressive re-optimization** directly addresses the plan regression problem that
   Ra's regression detection crate identifies but cannot currently fix.
