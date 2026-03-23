# RFC 0050: Decorrelation Improvements

- Start Date: 2026-03-22
- Author: RA Team
- Status: Proposed
- Tracking Issue: #TBD

## Summary

Extend the RA optimizer's subquery decorrelation capabilities to handle nested aggregates, lateral joins, multi-level correlated subqueries, and correlated subqueries in HAVING clauses. The current decorrelation support covers basic EXISTS/IN patterns and semi-join conversion but lacks the systematic decorrelation framework needed for complex analytical queries.

## Motivation

### Current State

The RA optimizer has limited subquery decorrelation:

- **`ra-engine/src/semi_join.rs`**: Converts EXISTS to semi-join, NOT EXISTS to anti-join, IN to semi-join, NOT IN to anti-join, scalar subqueries to left join + aggregate. Helper condition functions (`is_correlated_exists`, `is_single_column_subquery`, etc.) return `false` -- the rules exist structurally but the analysis to drive them is unimplemented.

- **`ra-engine/src/rewrite.rs`**: `subquery_optimization_rules()` contains two rules: merging a filter into a semi-join condition, and merging a filter into an anti-join condition. These are post-decorrelation simplifications, not decorrelation itself.

- **Tests in `logical_subquery_unnesting_test.rs`**: Test cases for subquery unnesting patterns, suggesting the intent exists but full implementation is incomplete.

### The Gap

The gap analysis (CHORES.md, Gap #10) identifies decorrelation improvements as needed. Specific missing capabilities:

1. **Nested correlated aggregates**: Subqueries that compute aggregates over correlated values:
   ```sql
   SELECT * FROM orders o
   WHERE o.amount > (
       SELECT AVG(amount)
       FROM orders o2
       WHERE o2.customer_id = o.customer_id
   );
   ```
   This requires decorrelating the scalar subquery into a grouped aggregation joined back to the outer query.

2. **Multi-level correlation**: Subqueries correlated to grandparent queries:
   ```sql
   SELECT * FROM departments d
   WHERE EXISTS (
       SELECT 1 FROM employees e
       WHERE e.dept_id = d.id
       AND e.salary > (
           SELECT AVG(salary) FROM employees e2
           WHERE e2.dept_id = d.id
       )
   );
   ```

3. **Correlated subqueries in HAVING**:
   ```sql
   SELECT customer_id, COUNT(*)
   FROM orders
   GROUP BY customer_id
   HAVING COUNT(*) > (
       SELECT AVG(order_count) FROM (
           SELECT COUNT(*) AS order_count
           FROM orders GROUP BY customer_id
       ) t
   );
   ```

4. **Lateral joins (LATERAL / CROSS APPLY)**: SQL:1999 lateral joins are syntactic sugar for correlated subqueries in FROM:
   ```sql
   SELECT d.name, top3.product_name
   FROM departments d
   CROSS JOIN LATERAL (
       SELECT product_name FROM sales
       WHERE sales.dept_id = d.id
       ORDER BY revenue DESC LIMIT 3
   ) top3;
   ```

5. **Correlated LIMIT/OFFSET**: Subqueries with LIMIT that reference outer columns (common in "top-N per group" patterns).

## Guide-level explanation

Decorrelation transforms correlated subqueries (which execute once per outer row) into joins or semi-joins (which execute once total). This can improve performance by orders of magnitude.

### Before decorrelation

```sql
-- Executes the subquery once per order row (~10M executions)
SELECT * FROM orders o
WHERE o.amount > (
    SELECT AVG(amount) FROM orders o2
    WHERE o2.customer_id = o.customer_id
);
```

Execution: For each of the 10M orders, compute the average amount for that customer. This is O(n^2).

### After decorrelation

```sql
-- Equivalent query with no correlated subquery
SELECT o.*
FROM orders o
JOIN (
    SELECT customer_id, AVG(amount) AS avg_amount
    FROM orders
    GROUP BY customer_id
) avg_by_cust ON o.customer_id = avg_by_cust.customer_id
WHERE o.amount > avg_by_cust.avg_amount;
```

Execution: Compute the average per customer once (single pass), then join. This is O(n).

### API Usage

```rust
use ra_engine::decorrelation::{
    Decorrelator, DecorrelationConfig,
};

let config = DecorrelationConfig {
    max_correlation_depth: 3,
    enable_lateral_unnesting: true,
    enable_having_decorrelation: true,
    ..Default::default()
};

let decorrelator = Decorrelator::new(config);
let decorrelated_plan = decorrelator.decorrelate(plan)?;
```

## Reference-level explanation

### Implementation Details

#### Decorrelation Framework

The decorrelation framework uses the "apply" operator (also called "dependent join" or "correlated join") as an intermediate representation:

```
CorrelatedSubquery(outer, inner, correlation)

=>  (step 1: make correlation explicit)

Apply(outer, inner[outer_cols -> params], correlation)

=>  (step 2: decorrelate by pushing Apply down)

Join(outer, inner_without_correlation, derived_join_condition)
```

The `Apply` operator evaluates `inner` once for each row of `outer`, binding correlated column references to the current outer row values. Decorrelation transforms `Apply` into a standard join by:

1. Identifying correlated references in `inner`
2. Adding the correlated columns to the inner query's input via a join
3. Replacing correlated references with direct column references
4. Converting the Apply to a Join/SemiJoin/AntiJoin/LeftJoin

#### Apply Operator

```rust
/// The Apply (dependent join) operator. Evaluates the inner
/// expression once per row of the outer expression, binding
/// correlated column references.
pub enum ApplyType {
    /// Cross apply: every outer row paired with inner result.
    /// Used for scalar subqueries and lateral joins.
    Cross,
    /// Semi apply: outer rows with non-empty inner result.
    /// Used for EXISTS subqueries.
    Semi,
    /// Anti apply: outer rows with empty inner result.
    /// Used for NOT EXISTS subqueries.
    Anti,
    /// Left apply: like cross, but preserves outer rows
    /// with empty inner result (using NULLs).
    /// Used for optional scalar subqueries.
    Left,
}

pub struct Apply {
    pub apply_type: ApplyType,
    pub outer: Box<RelExpr>,
    pub inner: Box<RelExpr>,
    /// Columns from outer referenced in inner.
    pub correlation: Vec<ColumnRef>,
}
```

#### Decorrelation Rules

**Rule 1: Correlated Filter to Join**

```
Apply(Cross, R, Filter(corr_pred, S))
  where corr_pred references columns from R
=>
Join(Inner, corr_pred, R, S)
```

**Rule 2: Correlated Aggregate to GroupBy + Join**

```
Apply(Cross, R, Aggregate([], [agg(x)], Filter(corr_pred, S)))
  where corr_pred is "S.key = R.key"
=>
LeftJoin(R.key = derived.key, R,
    Aggregate([key], [agg(x)], S) AS derived)
```

This is the key rule for nested aggregate decorrelation. The correlated filter becomes a group-by key in the decorrelated aggregate.

**Rule 3: Correlated EXISTS to Semi-Join**

```
Filter(EXISTS(Apply(Cross, R, Filter(corr_pred, S))), R)
=>
SemiJoin(corr_pred, R, S)
```

This rule exists in `semi_join.rs` but needs the analysis functions implemented.

**Rule 4: Multi-Level Decorrelation**

For multi-level correlation, decorrelation proceeds inside-out:

```
Apply(Semi, R,
    Filter(EXISTS(
        Apply(Cross, S, Filter(S.x = R.y AND S.z > T.w, T))
    ), S)
    WHERE S.key = R.key)

=>  (decorrelate inner Apply first)

Apply(Semi, R,
    Filter(EXISTS(
        SemiJoin(S.z > T.w, S, T)  -- inner decorrelated
    ), S)
    WHERE S.key = R.key)

=>  (decorrelate outer Apply)

SemiJoin(S.key = R.key,
    R,
    SemiJoin(EXISTS condition, S, T))
```

**Rule 5: LATERAL Join Unnesting**

```
Apply(Cross, R, TopN(n, sort_key, Filter(S.key = R.key, S)))
=>
WindowJoin(R, S,
    ROW_NUMBER() OVER (PARTITION BY S.key ORDER BY sort_key) <= n)
```

Or equivalently:

```
LeftJoin(R.key = ranked.key,
    R,
    Filter(rn <= n,
        Window(ROW_NUMBER() OVER (PARTITION BY key ORDER BY sort_key) AS rn, S)
    ) AS ranked)
```

**Rule 6: HAVING Clause Decorrelation**

```
Filter(HAVING_pred(Apply(Cross, grouped_R, subquery)), grouped_R)
=>
Join(derived_pred, grouped_R, decorrelated_subquery)
```

#### Analysis Infrastructure

The current helper functions in `semi_join.rs` all return `false`. This RFC implements proper analysis:

```rust
/// Analyze a subquery expression for correlation patterns.
pub struct CorrelationAnalysis {
    /// Columns from outer scope referenced in the subquery.
    pub correlated_columns: Vec<ColumnRef>,
    /// Depth of correlation (1 = parent, 2 = grandparent, etc.).
    pub correlation_depth: u32,
    /// Type of correlation pattern detected.
    pub pattern: CorrelationPattern,
}

pub enum CorrelationPattern {
    /// Simple equi-correlation: inner.col = outer.col
    EquiCorrelation {
        inner_cols: Vec<ColumnRef>,
        outer_cols: Vec<ColumnRef>,
    },
    /// Range correlation: inner.col > outer.col
    RangeCorrelation {
        inner_col: ColumnRef,
        outer_col: ColumnRef,
        op: BinOp,
    },
    /// Aggregate over correlated group
    CorrelatedAggregate {
        group_correlation: Vec<(ColumnRef, ColumnRef)>,
        aggregate: AggregateExpr,
    },
    /// Not decorrelatable (must use nested loops)
    NonDecorrelatable,
}

/// Walk a RelExpr tree and identify all correlated references
/// relative to a given outer scope.
pub fn analyze_correlation(
    inner: &RelExpr,
    outer_schema: &[ColumnRef],
) -> CorrelationAnalysis {
    // Walk the expression tree, collecting references
    // to columns in outer_schema
    todo!()
}
```

#### E-graph Integration

```rust
// Decorrelate scalar subquery with aggregate
rewrite!("decorrelate-scalar-aggregate";
    "(apply cross ?outer
        (aggregate nil ?aggs
            (filter (eq ?inner_col ?outer_col) ?source)))"
    =>
    "(join left-outer (eq ?outer_col ?group_col)
        ?outer
        (aggregate (list ?inner_col as ?group_col) ?aggs ?source))"
    if is_correlated_equi_join(?inner_col, ?outer_col, ?outer)
),

// Decorrelate EXISTS with correlated filter
rewrite!("decorrelate-exists-filter";
    "(filter (exists (apply cross ?outer
        (filter ?corr_pred ?source))) ?outer)"
    =>
    "(join semi ?corr_pred ?outer ?source)"
    if all_correlations_are_equi(?corr_pred, ?outer)
),

// Decorrelate NOT EXISTS
rewrite!("decorrelate-not-exists-filter";
    "(filter (not (exists (apply cross ?outer
        (filter ?corr_pred ?source)))) ?outer)"
    =>
    "(join anti ?corr_pred ?outer ?source)"
    if all_correlations_are_equi(?corr_pred, ?outer)
),

// Decorrelate lateral with LIMIT (top-N per group)
rewrite!("decorrelate-lateral-topn";
    "(apply cross ?outer
        (limit ?n ?off (sort ?key
            (filter (eq ?inner_col ?outer_col) ?source))))"
    =>
    "(join left-outer (eq ?outer_col ?partition_col)
        ?outer
        (filter (le ?rn ?n)
            (window (row-number (partition-by ?inner_col)
                                (order-by ?key)) as ?rn
                ?source)))"
    if is_simple_correlation(?inner_col, ?outer_col, ?outer)
),
```

### Integration Points

- **Semi-join rules** (`ra-engine/src/semi_join.rs`): The existing semi-join conversion rules are a subset of decorrelation. This RFC implements the analysis functions they depend on and adds additional rules.
- **Subquery unnesting tests** (`logical_subquery_unnesting_test.rs`): Existing test cases provide validation targets.
- **Parser** (`ra-parser`): The parser must identify correlated subqueries and represent them as `Apply` nodes in the logical plan.
- **Cost model** (`ra-engine/src/cost.rs`): Add cost estimates for `Apply` (nested loop execution) to ensure the optimizer prefers decorrelated plans.
- **Window functions**: LATERAL decorrelation may produce window function nodes (ROW_NUMBER for top-N per group). Requires window function support in the execution engine.

### Error Handling

- **Non-decorrelatable subqueries**: Some subqueries cannot be decorrelated (e.g., correlated subqueries with non-equi predicates and aggregates over the correlation). These remain as `Apply` operators and execute via nested loops. The optimizer logs a warning.
- **Incorrect correlation depth**: Multi-level decorrelation must track correlation depth correctly. An off-by-one error would produce incorrect results. The framework validates correlation depth at each transformation step.
- **NULL handling**: Decorrelation changes evaluation semantics around NULLs. NOT IN with NULLs has particularly tricky semantics (SQL three-valued logic). The transformation must preserve correct NULL behavior.

### Performance Considerations

- **Decorrelation itself is cheap**: The analysis and transformation are linear in plan size. No combinatorial explosion.
- **Benefit is massive**: Converting O(n*m) nested loop execution to O(n+m) join execution is typically 100-10000x faster for large inputs.
- **Statistics impact**: Decorrelated plans produce better cardinality estimates because joins have well-understood selectivity models, while correlated subqueries are opaque to the optimizer.
- **Plan cache friendliness**: Decorrelated plans are more stable (less sensitive to parameter values) than correlated plans.

## Drawbacks

- **Complexity**: The decorrelation framework is one of the most complex parts of a query optimizer. The Apply operator, correlation analysis, and inside-out decorrelation logic add significant implementation complexity.
- **Correctness risk**: Decorrelation transformations must preserve SQL semantics exactly, including NULL handling, duplicate handling, and empty set behavior. Bugs here produce silent wrong results.
- **Regression for small inputs**: For very small outer relations (e.g., 10 rows), the correlated nested loop may be faster than the decorrelated join due to lower startup cost.
- **Window function dependency**: LATERAL/top-N decorrelation introduces window functions, which may not be fully supported in all execution backends.

## Rationale and alternatives

### Why This Design?

The Apply-based decorrelation framework is the standard approach used by SQL Server, PostgreSQL, Calcite, and academic systems. It provides a systematic way to handle all types of correlated subqueries through a uniform intermediate representation (Apply) and a small set of transformation rules.

The inside-out decorrelation strategy (decorrelate innermost first) ensures each transformation step deals with a single level of correlation.

### Alternative Approaches

1. **Magic decorrelation (Seshadri et al., 1996)**: Uses "magic sets" to compute only the relevant subset of the inner query. More complex than Apply-based decorrelation and harder to implement in an e-graph framework.

2. **Unnesting via window functions only**: Convert all correlated subqueries to window functions. This works for some patterns but not all (e.g., correlated aggregates over different tables).

3. **Runtime optimization only**: Detect correlated patterns at runtime and batch subquery evaluations. This is what some systems do as a fallback but doesn't help the optimizer choose good plans.

### Impact of Not Doing This

Without decorrelation improvements:
- Queries with correlated subqueries execute via nested loops, which is O(n*m) vs. O(n+m) for decorrelated joins.
- The semi-join conversion rules in `semi_join.rs` remain non-functional (analysis helpers return `false`).
- TPC-H queries Q2, Q4, Q17, Q20, Q21, Q22 which contain correlated subqueries cannot be optimized effectively.
- User queries from BI tools (which generate correlated subqueries) perform poorly.

## Prior art

### Academic Research

- **Kim, W. (1982)**: "On Optimizing an SQL-like Nested Query." One of the earliest papers on subquery decorrelation. Identified the key transformations for converting correlated subqueries to joins.

- **Seshadri, P., Pirahesh, H., and Leung, T.Y.C. (1996)**: "Complex Query Decorrelation." The foundational paper from IBM Research. Introduced the Apply operator and the systematic decorrelation framework used by most modern optimizers.

- **Galindo-Legaria, C. and Joshi, M. (2001)**: "Orthogonal Optimization of Subqueries and Aggregation." Describes SQL Server's decorrelation framework, which handles aggregation and subqueries uniformly.

- **Neumann, T. and Kemper, A. (2015)**: "Unnesting Arbitrary Queries." From the HyPer system. Describes a general decorrelation algorithm that handles arbitrary queries including LATERAL and correlated aggregates.

- **Abo Khamis, M., et al. (2024)**: "Optimizing Nested Queries in Modern Database Systems." Survey of decorrelation techniques and their implementation in production systems.

### Industry Solutions

- **PostgreSQL**: Has basic decorrelation for simple EXISTS/IN patterns (converts to semi-join/anti-join). Does not decorrelate correlated aggregates or lateral subqueries with aggregates. The pull-up-sublinks pass handles simple cases; complex cases remain as SubPlan nodes with nested loop execution.

- **SQL Server**: Has the most sophisticated decorrelation implementation, based on the Seshadri et al. paper. Uses the Apply operator internally and systematically decorrelates through a series of algebraic transformations.

- **MySQL**: Limited decorrelation. Converts some IN subqueries to semi-joins (since MySQL 5.6). Does not handle correlated aggregates.

- **Apache Calcite**: Has `SubQueryRemoveRule` and `RelDecorrelator` which implement Apply-based decorrelation. Handles EXISTS, IN, scalar subqueries, and some lateral patterns.

- **DuckDB**: Implements "unnesting" for correlated subqueries using the "dependent join" (Apply) concept. Handles correlated aggregates and nested correlations.

- **CockroachDB**: Implements systematic decorrelation based on the same Apply framework, documented in their architecture docs.

### What We Can Learn

SQL Server and DuckDB demonstrate that the Apply-based framework is both general and practical. The key insight is that any correlated subquery can be represented as an Apply, and decorrelation is a series of algebraic transformations that push the Apply down through the inner plan until it becomes a standard join. CockroachDB's open-source implementation provides a readable reference.

## Unresolved questions

- **Correlation depth limit**: What is the maximum correlation depth we should support? Most practical queries have depth 1-2, but some generated queries (from BI tools) may have depth 3+.

- **Non-equi correlation**: How should we handle correlated subqueries with non-equality predicates (e.g., `WHERE inner.date > outer.date`)? These cannot be decorrelated into equi-joins but might use range joins or lateral joins.

- **Apply operator representation**: Should we add `Apply` as a first-class variant of `RelExpr`, or represent it implicitly through annotations on existing join nodes? A first-class variant is cleaner but requires changes throughout the codebase.

- **Interaction with semi-join reduction (RFC 0047)**: After decorrelation produces semi-joins, should semi-join reduction be applied? This seems natural but needs validation.

## Future possibilities

### Natural Extensions

- **Incremental decorrelation**: For materialized views with correlated subqueries, incrementally maintain the decorrelated result as base tables change.

- **Parameterized plan caching**: For subqueries that cannot be fully decorrelated, cache and reuse subquery results for repeated parameter values (similar to RFC 0032 Memoize).

- **Cost-based decorrelation**: Sometimes the correlated plan is faster (small outer, large inner). Add cost-based decision making to choose between correlated and decorrelated plans.

### Long-term Vision

Complete decorrelation support allows RA to handle the full range of SQL queries that users and BI tools generate. Combined with semi-join reduction (RFC 0047) for the resulting semi-joins and partial aggregation (RFC 0049) for the resulting aggregations, this completes the subquery optimization pipeline: parse correlated subquery -> decorrelate to join/semi-join -> optimize the resulting plan.

## Implementation Plan

### Phase 1: Correlation Analysis (Week 1)
- Implement `CorrelationAnalysis` and `analyze_correlation()`
- Implement the helper functions in `semi_join.rs` (`is_correlated_exists`, `is_single_column_subquery`, etc.)
- Add Apply operator to RelExpr (or as annotation)
- Test with simple EXISTS/IN decorrelation

### Phase 2: Aggregate and Scalar Subquery Decorrelation (Week 2)
- Implement Rule 2 (correlated aggregate to GroupBy + Join)
- Implement scalar subquery decorrelation improvements
- Handle mixed correlated + non-correlated predicates
- Test with TPC-H Q2, Q4, Q17

### Phase 3: Advanced Patterns (Week 3)
- Implement multi-level decorrelation (inside-out strategy)
- Implement LATERAL/top-N per group decorrelation
- Implement HAVING clause decorrelation
- Test with TPC-H Q20, Q21, Q22

### Phase 4: Hardening (Week 4)
- NULL semantics validation (NOT IN with NULLs, NOT EXISTS behavior)
- Edge cases: empty outer, empty inner, all NULLs
- Integration testing with TPC-DS correlated queries

### Estimated Effort: 3-4 weeks
