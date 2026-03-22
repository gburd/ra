# DataFusion and Apache Calcite Optimizer Rules: Comparative Analysis

**Source:** DataFusion source code (github.com/apache/datafusion), Calcite source code (github.com/apache/calcite)
**Date:** 2026-03-22
**Topic:** Production optimizer rule sets compared against Ra
**Context:** CMU 15-721 Spring 2024 references both Calcite (Lecture 13) and DataFusion

## Key Points

DataFusion and Calcite represent two mature, widely-deployed optimizer rule sets.
Comparing them against Ra's rules identifies consensus optimization techniques
that Ra should implement.

### DataFusion Optimizer Rules (Complete List)

From `datafusion/optimizer/src/`:

| Rule | Category | Ra Status |
|------|----------|-----------|
| `common_subexpr_eliminate` | Expression | Has `common-subexpression-elimination.rra` |
| `decorrelate` | Subquery | Has `correlated-subquery-decorrelation.rra` |
| `decorrelate_lateral_join` | Subquery | Has `lateral-join-decorrelation.rra` |
| `decorrelate_predicate_subquery` | Subquery | Has `in-subquery-to-semi-join.rra` etc |
| `eliminate_cross_join` | Join | Has `cross-join-elimination.rra` |
| `eliminate_duplicated_expr` | Expression | Likely covered by simplification rules |
| `eliminate_filter` | Filter | Has `starburst-contradiction-detection.rra` |
| `eliminate_group_by_constant` | Aggregate | Has `aggregate-with-constant-elimination.rra` |
| `eliminate_join` | Join | Has multiple join elimination rules |
| `eliminate_limit` | Limit | Verify in limit-pushdown |
| `eliminate_outer_join` | Join | Has `outer-join-to-inner.rra` |
| `extract_equijoin_predicate` | Join | **MISSING** - separate = from non-= in joins |
| `filter_null_join_keys` | Join | **MISSING** - add IS NOT NULL for join keys |
| `optimize_unions` | Set Ops | Has `union-merge.rra`, `union-eliminator.rra` |
| `propagate_empty_relation` | Execution | **MISSING** - short-circuit empty inputs |
| `push_down_filter` | Filter | Has extensive predicate pushdown |
| `push_down_limit` | Limit | Has limit pushdown rules |
| `replace_distinct_aggregate` | Aggregate | Has `distinct-to-group-by.rra` |
| `rewrite_set_comparison` | Expression | Verify coverage |
| `scalar_subquery_to_join` | Subquery | Has `scalar-subquery-to-join.rra` |
| `single_distinct_to_groupby` | Aggregate | **MISSING** - COUNT(DISTINCT x) rewrite |

### Apache Calcite Optimizer Rules (Selected Highlights)

From `org.apache.calcite.rel.rules` (145 rules total):

**Rules Ra has:**
- `FilterJoinRule` (predicate pushdown through joins)
- `JoinCommuteRule`, `JoinAssociateRule` (join reordering)
- `AggregateJoinTransposeRule` (eager aggregation)
- `ProjectMergeRule`, `ProjectRemoveRule` (projection optimization)
- `FilterMergeRule` (filter combination)
- `SemiJoinRule` (semi-join optimization)

**Rules Ra may be missing:**

1. `JoinPushTransitivePredicatesRule` - Derive new predicates from transitive closure
   of join equalities. If A.x = B.y and B.y = C.z, derive A.x = C.z.
   **Impact:** Enables predicate pushdown to tables not directly connected in joins.

2. `AggregateExpandDistinctAggregatesRule` - Expand queries with multiple DISTINCT
   aggregates into UNION ALL of per-group queries.
   Ra has `aggregate-expand-distinct-aggregates.rra` - verify completeness.

3. `AggregatePullUpConstantsRule` - Pull constant expressions out of aggregates.
   Ra has `aggregate-project-pull-up-constants.rra` - verify.

4. `FilterTableScanRule` - Push filters into table scans (different from general pushdown).
   Ra likely has this via access path selection.

5. `IntersectToSemiJoinRule` - Convert INTERSECT to semi-join.
   Ra has `intersect-to-semi-join.rra`.

6. `MinusToAntiJoinRule` - Convert EXCEPT to anti-join.
   Ra has `minus-to-anti-join.rra`.

7. `ValuesReduceRule` - Reduce expressions over VALUES clauses at compile time.
   Ra has `aggregate-values.rra` - verify coverage.

8. `SortRemoveRule` - Remove Sort when input is already sorted.
   Ra has `sort-elimination-by-index.rra` - verify it also handles sorted inputs
   from other sources (merge join output, sorted scan).

### BusTub Teaching Optimizer Rules

BusTub (CMU 15-445 teaching database) implements the minimum viable rule set:

1. `column_pruning` - Ra has
2. `eliminate_true_filter` - Ra has via contradiction detection
3. `merge_filter_nlj` - Ra has via filter pushdown through joins
4. `merge_filter_scan` - Ra has via predicate pushdown
5. `merge_projection` - Ra has via projection merging
6. `nlj_as_hash_join` - Ra has
7. `nlj_as_index_join` - Ra has
8. `order_by_index_scan` - Ra has
9. `seqscan_as_indexscan` - Ra has
10. `sort_limit_as_topn` - Ra has `sort-limit-heap.rra`

Ra covers all BusTub rules (as expected).

## Definitive Missing Rules (Cross-Referenced Across Systems)

Rules that are implemented by both DataFusion AND Calcite but missing from Ra:

1. **extract-equijoin-predicate** - Separate equality predicates from non-equality
   predicates in join conditions. Equality predicates enable hash join and merge join;
   non-equality predicates must be applied as post-join filters.
   - DataFusion: `extract_equijoin_predicate.rs`
   - Calcite: `JoinExtractFilterRule`
   - Ra: Not found

2. **filter-null-join-keys** - Add IS NOT NULL filter on join key columns before
   join execution. NULL keys never match in equi-joins, so filtering them early
   reduces join build and probe size.
   - DataFusion: `filter_null_join_keys.rs`
   - Calcite: Implicit in join processing
   - Ra: Not found

3. **propagate-empty-relation** - When any operator has a provably empty input,
   propagate the empty result upward. Examples: contradictory WHERE clause,
   LIMIT 0, empty values clause.
   - DataFusion: `propagate_empty_relation.rs`
   - Calcite: `PruneEmptyRules`
   - Ra: Not found

4. **single-distinct-to-groupby** - Rewrite `SELECT COUNT(DISTINCT col) FROM t`
   to `SELECT COUNT(*) FROM (SELECT col FROM t GROUP BY col)`.
   - DataFusion: `single_distinct_to_groupby.rs`
   - Calcite: `AggregateExpandDistinctAggregatesRule`
   - Ra: Not found as a separate rule (verify `aggregate-distinct-optimization.rra`)

5. **eliminate-limit** - Remove redundant LIMIT clauses (LIMIT on already-limited
   input, LIMIT larger than child's max rows, LIMIT 0 -> empty).
   - DataFusion: `eliminate_limit.rs`
   - Calcite: `SortRemoveRule` (partial)
   - Ra: Verify in limit-pushdown rules

## Relevance to Ra

**Priority:** High for rules 1-4 above. These are consensus optimizations
implemented by every production optimizer. Their absence in Ra represents
genuine functional gaps.

**Recommended actions:**
1. Implement `extract-equijoin-predicate` (enables correct hash/merge join selection)
2. Implement `filter-null-join-keys` (reduces join input size, low complexity)
3. Implement `propagate-empty-relation` (eliminates unnecessary computation)
4. Verify `single-distinct-to-groupby` coverage in existing aggregate rules
5. Verify `eliminate-limit` coverage in existing limit pushdown rules
