# TiDB Rule Extraction Progress

Tracking completion of 12 TiDB optimization rules for Task #5.

## Completed Rules (2/12)

1. ✅ **aggregation-pushdown-decomposable.rra** - Decomposable aggregation pushdown
   - Source: `rule_aggregation_push_down.go:44-59`
   - Functions: MAX, MIN, SUM, COUNT (without DISTINCT)

2. ✅ **coprocessor-predicate-pushdown.rra** - Filter pushdown to TiKV
   - Source: `find_best_task.go`, `exhaust_physical_plans.go`
   - Pushes filters to storage layer for early filtering

## Remaining Rules (10/12)

### Aggregation Optimization (2 remaining)
3. 📋 **aggregation-elimination.rra**
   - Source: `rule_aggregation_push_down.go` (aggregationEliminateChecker)
   - Remove redundant GROUP BY when all rows guaranteed unique

4. 📋 **aggregation-merge.rra**
   - Source: Pattern from consecutive aggregations
   - Merge multiple aggregation layers when possible

### Coprocessor Pushdown (3 remaining)
5. 📋 **coprocessor-projection-pushdown.rra**
   - Source: `find_best_task.go` (buildCopTask)
   - Push column projections to TiKV

6. 📋 **coprocessor-limit-pushdown.rra**
   - Source: `rule_push_down_sequence.go`
   - Push LIMIT to storage layer

7. 📋 **coprocessor-topn-pushdown.rra**
   - Source: `exhaust_physical_plans.go` (getTaskPlan)
   - Push ORDER BY + LIMIT (TOP-N) to TiKV

### Join Optimization (3 remaining)
8. 📋 **join-reorder-dp.rra**
   - Source: `rule_join_reorder_dp.go`
   - Dynamic programming for optimal join order

9. 📋 **semi-join-rewrite.rra**
   - Source: `rule_semi_join_rewrite.go`
   - Transform semi-join to inner join when beneficial

10. 📋 **outer-join-elimination.rra**
    - Source: `rule_outer_join_elimination.go`
    - Simplify outer joins to inner joins using constraints

### Distributed Execution (2 remaining)
11. 📋 **partition-pruning.rra**
    - Source: `partition_pruning.go`
    - Eliminate unnecessary partitions in range/hash partitioned tables

12. 📋 **index-merge.rra**
    - Source: `exhaust_physical_plans.go` (getIndexMergeTask)
    - Merge multiple index accesses (OR conditions)

## Implementation Strategy

Each rule will include:
- Complete relational algebra transformation
- Rust/egg implementation snippets
- Source file and function references
- Cost model with complexity analysis
- Positive/negative test cases
- TiDB documentation links

## Source Repository

- Repository: https://github.com/pingcap/tidb
- Branch: main (shallow clone)
- Primary directory: `pkg/planner/core/`
- Cloned at: `/tmp/claude/repo-clones/tidb/`
