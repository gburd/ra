# CMU Seminar: Apache DataFusion Query Engine

**Source:** CMU Database Seminar Fall 2024
**Speaker:** Andrew Lamb
**Topic:** Apache Arrow DataFusion - Modular Analytic Query Engine

## Key Concepts

### DataFusion Optimizer Architecture
- Three-phase optimizer: analysis, logical optimization, physical planning
- Rule-based logical optimizer with ordered rule passes
- Physical optimizer selects implementations and inserts enforcers
- Designed for extensibility: users can add custom rules

### Logical Optimization Rules (Full List)
1. **SimplifyExpressions**: Constant folding, boolean simplification
2. **EliminateDuplicatedExpr**: Remove duplicate expressions in projections
3. **CommonSubexprEliminate**: Compute shared subexpressions once
4. **DecorrelatePredicateSubquery**: IN/EXISTS -> SEMI/ANTI JOIN
5. **ScalarSubqueryToJoin**: Scalar subquery in SELECT -> LEFT JOIN
6. **PullUpCorrelatedExpr**: Decorrelate correlated subqueries
7. **DecorrelateLateralJoin**: Handle LATERAL join decorrelation
8. **EliminateCrossJoin**: CROSS JOIN + WHERE -> INNER JOIN
9. **EliminateOuterJoin**: Outer -> Inner when WHERE rejects nulls
10. **EliminateJoin**: Remove joins with constant true/false conditions
11. **ExtractEquijoinPredicate**: Identify = predicates in join conditions
12. **FilterNullJoinKeys**: Add IS NOT NULL filter for non-nullable joins
13. **PushDownFilter**: Move filters as close to scans as possible
14. **EliminateFilter**: Remove always-true filters, replace always-false with Empty
15. **EliminateGroupByConstant**: Remove constants from GROUP BY list
16. **ReplaceDistinctWithAggregate**: DISTINCT -> GROUP BY
17. **SingleDistinctToGroupBy**: AGG(DISTINCT col) -> GROUP BY col, AGG(col)
18. **PushDownLimit**: Push LIMIT below certain operators
19. **EliminateLimit**: Remove LIMIT 0 (empty result) or redundant limits
20. **PropagateEmptyRelation**: Short-circuit when input is empty
21. **OptimizeProjections**: Eliminate unused column computations
22. **OptimizeUnions**: Flatten nested UNIONs, eliminate redundant branches

### Physical Optimization
- Join selection: HashJoin, SortMergeJoin, NestedLoopJoin, CrossJoin
- Aggregate selection: HashAggregate, GroupedAggregate
- Sort enforcement: when merge join or ORDER BY requires sorted input
- Partition enforcement: redistribute data for hash join or aggregate

### Key Design Decisions
- Separate logical and physical plans (clean abstraction)
- Rules applied in fixed order, not iterative
- No cost-based join ordering (uses heuristics)
- Physical properties not tracked through logical plan

## Applicable to Ra

### New Rule Ideas
1. **Filter Null Join Keys**: For inner joins, add IS NOT NULL filter on
   join key columns. Removes null rows early, reducing join work.
2. **Single Distinct to GroupBy**: Convert `SELECT COUNT(DISTINCT a) FROM t`
   to `SELECT COUNT(*) FROM (SELECT a FROM t GROUP BY a)`.
3. **Eliminate GroupBy Constant**: Remove constant expressions from GROUP BY
   keys (they contribute nothing to grouping).
4. **Propagate Empty Relation**: When any branch of JOIN/UNION is provably
   empty, simplify the entire subtree.
5. **Flatten Nested Unions**: UNION ALL(UNION ALL(A, B), C) -> UNION ALL(A, B, C).
6. **Extract Equijoin Predicate**: Separate equality predicates from
   non-equality predicates in join conditions for join method selection.

### Gap Analysis
- Ra has most of the basic logical rules
- Missing: filter null join keys
- Missing: single distinct -> group by conversion
- Missing: empty relation propagation
- Missing: group by constant elimination
- Missing: nested union flattening
