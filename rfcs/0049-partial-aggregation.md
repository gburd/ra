# RFC 0049: Partial Aggregation (Two-Phase)

- Start Date: 2026-03-22
- Author: RA Team
- Status: Proposed
- Tracking Issue: #TBD

## Summary

Implement two-phase partial aggregation as a local (non-distributed) optimization that inserts a pre-aggregation step before expensive operations like joins, sorts, and exchanges. While the RA codebase already has distributed two-phase aggregation infrastructure in `ra-core/src/distributed_agg.rs`, this RFC addresses the complementary local optimization: splitting a single aggregation into partial (pre-aggregate) and final (merge-aggregate) phases to reduce intermediate data volumes within a single node or execution pipeline.

## Motivation

### Current State

The RA codebase has substantial distributed aggregation infrastructure:

- `ra-core/src/distributed_agg.rs`: Defines `AggregationStrategy` with `TwoPhase`, `ThreePhase`, `MapReduce`, and `SkewAware` variants. Provides decomposition of aggregate functions into local/global phases.
- `ra-engine/src/distributed_optimizer.rs`: Selects aggregation strategies based on data distribution, skew, and cluster topology.
- `ra-engine/tests/aggregation_distribution_integration.rs`: Tests for distributed aggregation scenarios.

However, this infrastructure is exclusively designed for *distributed* execution -- splitting aggregation across network-connected nodes. The RA optimizer lacks *local* partial aggregation, which is valuable even on a single node.

### The Gap

Local partial aggregation inserts a pre-aggregation operator at strategic points in the query plan to reduce the number of rows flowing through subsequent operators. This is distinct from distributed aggregation in several ways:

1. **No network transfer**: The partial aggregate runs in the same process, targeting CPU and memory reduction rather than network traffic.
2. **Pipeline integration**: Partial aggregation fits into the operator pipeline between a scan and a join, or between a scan and a sort.
3. **Adaptive behavior**: Local partial aggregation can be abandoned at runtime if the reduction ratio is poor (too many groups relative to input).
4. **Hash table spilling**: For large aggregations, partial aggregation reduces peak memory by aggregating batches before they enter the final hash table.

### Specific scenarios

1. **Pre-join aggregation**: When an aggregation follows a join, pushing a partial aggregate below the join reduces the join input size:

```sql
SELECT customer.name, SUM(orders.amount)
FROM orders JOIN customer ON orders.cust_id = customer.id
GROUP BY customer.name;
```

With partial aggregation on `orders` grouped by `cust_id` before the join, duplicate `cust_id` values are combined, reducing join input rows.

2. **Pre-sort aggregation**: Before a sort for ORDER BY or window functions, partial aggregation reduces the number of rows to sort.

3. **Pre-exchange aggregation** (pipeline context): In a parallel pipeline, partial aggregation per worker thread reduces data before the exchange/repartition step -- this overlaps with the distributed case but applies to thread-level parallelism.

4. **Memory-constrained aggregation**: For aggregations that exceed available memory, partial aggregation in batches reduces peak hash table size, delaying or avoiding spill to disk.

## Guide-level explanation

The optimizer inserts partial aggregation operators at strategic points in query plans to reduce intermediate row counts. This optimization is transparent to users -- it produces the same results with lower resource consumption.

### How it works

Given a query plan:

```
Aggregate(GROUP BY region, SUM(amount))
    HashJoin(orders.cust_id = customer.id)
        SeqScan(orders)       -- 10M rows
        SeqScan(customer)     -- 100K rows
```

The optimizer inserts a partial aggregation below the join:

```
FinalAggregate(GROUP BY region, SUM(partial_sum))
    HashJoin(orders.cust_id = customer.id)
        PartialAggregate(GROUP BY cust_id, SUM(amount) AS partial_sum)
            SeqScan(orders)   -- 10M rows -> ~1M groups
        SeqScan(customer)     -- 100K rows
```

The partial aggregate reduces `orders` from 10M rows to ~1M rows (one per customer), reducing join input by 10x.

### API Usage

```rust
use ra_engine::partial_agg::{
    PartialAggPlanner, PartialAggConfig,
};

let config = PartialAggConfig {
    // Minimum reduction ratio to justify partial aggregation
    min_reduction_ratio: 0.5,
    // Maximum hash table size (rows) for partial aggregation
    max_partial_groups: 1_000_000,
    // Enable adaptive partial aggregation (abandon if not reducing)
    adaptive: true,
    // Threshold for adaptive abandonment
    adaptive_check_interval: 10_000,
    ..Default::default()
};

let planner = PartialAggPlanner::new(config, &statistics);
let optimized = planner.insert_partial_aggregation(plan)?;
```

## Reference-level explanation

### Implementation Details

#### Operator Model

Two new logical operators are introduced:

```rust
/// Partial (pre-) aggregation. Produces partial aggregate results
/// that must be finalized by a FinalAggregate operator.
pub struct PartialAggregate {
    /// Group-by columns for the partial aggregation.
    /// May differ from the final group-by if pushed below a join.
    pub group_by: Vec<Expr>,
    /// Aggregate expressions producing partial results.
    pub aggregates: Vec<PartialAggExpr>,
    /// Input relation.
    pub input: Box<RelExpr>,
}

/// Final (merge) aggregation that consumes partial aggregate results.
pub struct FinalAggregate {
    /// Final group-by columns.
    pub group_by: Vec<Expr>,
    /// Aggregate expressions that merge partial results.
    pub aggregates: Vec<FinalAggExpr>,
    /// Input (output of PartialAggregate or exchange).
    pub input: Box<RelExpr>,
}
```

These compose with the existing `AggDecomposition` from `distributed_agg.rs`:

```rust
pub struct PartialAggExpr {
    /// The decomposed local function (e.g., SUM for SUM, COUNT for COUNT).
    pub function: AggregateFunction,
    /// Input expression.
    pub arg: Option<Expr>,
    /// Output column name.
    pub alias: String,
}

pub struct FinalAggExpr {
    /// The decomposed global function (e.g., SUM for local SUM,
    /// SUM for local COUNT).
    pub function: AggregateFunction,
    /// Input column (output of partial aggregate).
    pub partial_col: ColumnRef,
    /// Output column name.
    pub alias: String,
}
```

#### Placement Heuristics

The optimizer considers inserting partial aggregation at these points:

1. **Below joins**: When a join input has a group-by key that is a subset of or functionally determines the join key, partial aggregation below the join reduces join input size.

2. **Below sorts**: When a sort precedes an aggregation, partial aggregation before the sort reduces sort input. This is especially valuable for external sorts.

3. **At pipeline boundaries**: In parallel execution, partial aggregation per worker reduces the data volume at exchange points.

4. **Above scans with high duplication**: When a table scan produces many duplicate group-by key values, partial aggregation immediately after the scan reduces downstream processing.

The placement decision uses:

$$\text{benefit} = (\text{input\_rows} - \text{estimated\_groups}) \times \text{row\_size} \times C_{\text{downstream}}$$
$$\text{cost} = \text{input\_rows} \times C_{\text{hash\_probe}} + \text{estimated\_groups} \times C_{\text{hash\_build}}$$

Partial aggregation is inserted when benefit > cost.

#### Aggregate Function Decomposition

The existing `AggregationStrategy::decompose_aggregate` in `distributed_agg.rs` is reused. This already handles:

| Function | Partial | Final |
|----------|---------|-------|
| COUNT | COUNT | SUM |
| SUM | SUM | SUM |
| MIN | MIN | MIN |
| MAX | MAX | MAX |
| AVG | SUM + COUNT | SUM/SUM |

Functions that are not decomposable (StdDev, Variance, StringAgg, ArrayAgg) cannot use partial aggregation through simple decomposition. For Variance/StdDev, the three-statistic decomposition (`VarianceDecomposition`) from `distributed_agg.rs` applies.

#### Adaptive Partial Aggregation

When the number of groups is uncertain, the optimizer inserts an *adaptive* partial aggregate that monitors its own effectiveness at runtime:

```rust
pub struct AdaptivePartialAggregate {
    /// The partial aggregation to attempt.
    pub partial: PartialAggregate,
    /// Check reduction ratio every N input rows.
    pub check_interval: u64,
    /// Abandon partial aggregation if reduction ratio
    /// falls below this threshold.
    pub min_reduction: f64,
    /// Fallback: pass rows through unmodified.
    pub passthrough_on_abandon: bool,
}
```

The adaptive aggregate works as follows:
1. Process the first `check_interval` rows normally (build hash table, emit partial results).
2. After `check_interval` rows, compute `reduction = 1 - groups / rows_seen`.
3. If `reduction < min_reduction`, switch to passthrough mode (stop aggregating, forward rows directly to the final aggregate).
4. Otherwise, continue partial aggregation and re-check periodically.

This is modeled after Spark's adaptive partial aggregation and Presto's partial aggregation with "split" fallback.

#### E-graph Integration

```rust
// Split aggregate into partial + final
rewrite!("two-phase-aggregate";
    "(aggregate ?groups ?aggs ?input)" =>
    "(final-aggregate ?groups (merge-aggs ?aggs)
        (partial-aggregate ?groups (partial-aggs ?aggs) ?input))"
    if aggregates_decomposable(?aggs)
    if reduction_ratio_sufficient(?groups, ?input)
),

// Push partial aggregate below join (left side)
rewrite!("partial-agg-below-join-left";
    "(aggregate ?groups ?aggs (join inner ?cond ?left ?right))" =>
    "(final-aggregate ?groups (merge-aggs ?aggs)
        (join inner ?cond
            (partial-aggregate (join-key-groups ?groups ?cond left)
                               (partial-aggs ?aggs) ?left)
            ?right))"
    if can_push_partial_agg_through_join(?groups, ?cond, ?aggs)
),

// Push partial aggregate below sort
rewrite!("partial-agg-below-sort";
    "(aggregate ?groups ?aggs (sort ?keys ?input))" =>
    "(final-aggregate ?groups (merge-aggs ?aggs)
        (sort ?keys
            (partial-aggregate ?groups (partial-aggs ?aggs) ?input)))"
    if aggregates_decomposable(?aggs)
),
```

### Integration Points

- **Distributed aggregation** (`ra-core/src/distributed_agg.rs`): Reuse `AggDecomposition`, `AvgDecomposition`, and `VarianceDecomposition` for function splitting. The local partial aggregation uses the same decomposition logic but at a different level of the plan.
- **Cost model** (`ra-engine/src/cost.rs`): Add cost functions for `PartialAggregate` and `FinalAggregate` operators. The partial aggregate cost is dominated by hash table operations.
- **Parallel execution** (RFC 0020): Partial aggregation per parallel worker reduces exchange volume, complementing the parallel aggregation infrastructure.
- **Aggregate pushdown rules** (`ra-engine`): The existing aggregate-through-join rules should compose with partial aggregation. Partial aggregation is a weaker form of full pushdown -- it applies when full pushdown is not possible.
- **Eager aggregation** (RFC 0043): GroupJoin/eager aggregation is a related optimization that pushes full (not partial) aggregation below joins. Partial aggregation is the fallback when eager aggregation conditions are not met.

### Error Handling

- **Non-decomposable aggregates**: If the aggregate list contains functions that cannot be decomposed, skip partial aggregation for the entire node. Do not partially decompose (this avoids correctness bugs from mixing partial and non-partial aggregates).
- **Group explosion**: If the partial aggregation produces more groups than expected (detected by adaptive monitoring), abandon and log a warning.
- **Memory limits**: If the partial hash table exceeds a memory budget, flush partial results and start a new batch (mini-batch partial aggregation).

### Performance Considerations

- **Best case**: Input with many duplicates per group. Partial aggregation reduces rows by 90%+, saving join/sort/exchange costs.
- **Worst case**: Input with unique group keys (no duplicates). Partial aggregation adds overhead (hash probe per row) with no reduction. The adaptive mechanism detects this and abandons.
- **Memory**: A partial aggregation hash table for 1M groups with 8-byte key + 16-byte accumulators uses ~24MB. This is typically acceptable.
- **Break-even**: Partial aggregation is beneficial when the reduction ratio exceeds ~50% and the downstream operator cost is significant (join, sort, or network exchange).

## Drawbacks

- **Plan complexity**: Adding partial/final aggregate pairs increases plan size and complexity, making plan debugging harder.
- **Adaptive overhead**: The adaptive check adds branching overhead per row, though this is minimal compared to hash table operations.
- **Interaction with other aggregation optimizations**: Partial aggregation, eager aggregation (RFC 0043), and distinct aggregation rewrite (RFC 0048) all transform aggregation nodes. The optimizer must carefully order these transformations or handle their interactions in the e-graph.
- **Over-eagerness**: Without good statistics, the optimizer may insert partial aggregation where it is not beneficial, adding unnecessary overhead.

## Rationale and alternatives

### Why This Design?

Separating partial aggregation from distributed aggregation allows the optimization to apply in both local and distributed contexts. The adaptive approach handles the common case where cardinality estimates are uncertain.

Reusing the decomposition infrastructure from `distributed_agg.rs` avoids duplicating the aggregate function splitting logic and ensures consistency between local and distributed partial aggregation.

### Alternative Approaches

1. **Only distributed partial aggregation**: Keep partial aggregation as a distributed-only optimization (current state). This misses significant local optimization opportunities, especially for pre-join aggregation.

2. **Full eager aggregation only (RFC 0043)**: Only push complete aggregations below joins (GroupJoin). This is more restrictive -- it requires the aggregation to be fully computable below the join, which is not always possible.

3. **Sort-based partial aggregation**: Instead of hash-based, use a sorted intermediate to combine adjacent groups. This avoids the hash table overhead but requires sorted input, limiting placement flexibility.

### Impact of Not Doing This

Without local partial aggregation:
- Join inputs remain unreduced even when the aggregation could eliminate duplicates early
- Sort operations process more rows than necessary
- Memory consumption for large aggregations is higher than optimal
- The gap between RA and production systems (Spark, Presto, DuckDB) that implement this optimization remains

## Prior art

### Academic Research

- **Yan, W.P. and Larson, P. (1995)**: "Eager Aggregation and Lazy Aggregation." Introduced the formal framework for pushing aggregations through joins, distinguishing between eager (full) and lazy (partial) aggregation. Partial aggregation is the "lazy" case.

- **Chaudhuri, S. and Shim, K. (1994)**: "Including Group-By in Query Optimization." Extended query optimization to consider group-by placement, including partial aggregation at intermediate points.

- **Neumann, T. and Moerkotte, G. (2006)**: "An Efficient Framework for Order Optimization." Discusses how partial aggregation interacts with sort order and interesting orderings.

### Industry Solutions

- **PostgreSQL**: Has `Partial HashAggregate` and `Finalize HashAggregate` operators for parallel aggregation (since PG 9.6). The partial aggregate runs per-worker in a parallel scan, and the finalize aggregate merges results. PG does not insert partial aggregation below joins.

- **Apache Spark**: Implements partial aggregation in the `HashAggregateExec` operator with the `partial` and `final` modes. Spark uses adaptive partial aggregation that falls back to sort-based aggregation when the hash table exceeds memory. Spark also pushes partial aggregation below shuffles in the `EnsureRequirements` rule.

- **Presto/Trino**: Has `PARTIAL`, `INTERMEDIATE`, and `FINAL` aggregation modes. Partial aggregation runs per-split with adaptive abandonment when effectiveness is low. The `AddExchanges` optimizer rule inserts partial aggregation before exchanges.

- **DuckDB**: Uses streaming partial aggregation with adaptive fallback. When the hash table grows too large, DuckDB switches to sorting the input and doing a merge-based aggregation.

- **Apache Calcite**: Has `AggregatePartialRule` that splits aggregation into partial and final phases. Combined with `AggregateReduceFunctionsRule` for function decomposition.

### What We Can Learn

Spark and Presto demonstrate that adaptive partial aggregation is essential for robustness. Without it, partial aggregation on high-cardinality groups wastes memory and CPU. PostgreSQL's parallel partial aggregation shows that the concept extends naturally to thread-level parallelism.

## Unresolved questions

- **Ordering with RFC 0043**: How should partial aggregation interact with the GroupJoin/eager aggregation optimization from RFC 0043? Should partial aggregation be tried only when eager aggregation fails, or should both be explored in the e-graph?

- **Multi-level partial aggregation**: Can we benefit from more than two levels (partial-intermediate-final)? The distributed `ThreePhase` strategy suggests yes for skewed data, but the local case is less clear.

- **Batch size for adaptive checks**: What is the right `check_interval` for the adaptive mechanism? Too small adds overhead; too large delays the abandonment decision.

## Future possibilities

### Natural Extensions

- **Partial aggregation with spilling**: Extend the partial aggregate operator to spill to disk when the hash table exceeds memory, combining partial results from multiple spill passes.

- **Vectorized partial aggregation**: Implement partial aggregation over columnar batches for better cache utilization.

- **Cost-based partial aggregation placement**: Integrate partial aggregation placement into the main cost-based optimizer rather than using heuristic rules.

### Long-term Vision

Local partial aggregation completes the aggregation optimization story alongside distributed partial aggregation (`distributed_agg.rs`), eager aggregation (RFC 0043), and distinct aggregation rewrite (RFC 0048). Together, these ensure RA can generate competitive aggregation plans across all execution contexts -- single-node, parallel, and distributed.

## Implementation Plan

### Phase 1: Core Partial Aggregation (Week 1)
- Define `PartialAggregate` and `FinalAggregate` RelExpr variants
- Implement aggregate decomposition reuse from `distributed_agg.rs`
- Add e-graph rules for splitting aggregation into partial/final
- Add cost model entries for partial/final aggregation
- Test with simple GROUP BY queries

### Phase 2: Placement and Pushdown (Week 2)
- Implement partial aggregation placement below joins
- Implement partial aggregation placement below sorts
- Add placement heuristics using statistics
- Test with TPC-H Q1, Q3, Q5, Q10

### Phase 3: Adaptive Mechanism (Week 3)
- Implement `AdaptivePartialAggregate` operator
- Add runtime monitoring of reduction ratio
- Implement passthrough fallback
- Stress test with varying data distributions

### Estimated Effort: 2-3 weeks
