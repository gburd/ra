# RFC 0047: Semi-Join Reduction

- Start Date: 2026-03-22
- Author: RA Team
- Status: Proposed
- Tracking Issue: #TBD

## Summary

Implement semi-join reduction as a first-class optimization strategy for distributed and local query execution. Semi-join reduction transmits only the join keys from one relation to filter the other before the actual join, dramatically reducing data transfer in distributed settings and I/O in local settings when join selectivity is low.

## Motivation

The RA optimizer currently has basic semi-join support (`crates/ra-engine/src/semi_join.rs`) that converts EXISTS/IN subqueries to semi-joins and performs filter pushdown through semi-joins. However, it lacks the critical *semi-join reduction* optimization where a semi-join is used as a pre-filtering step before a regular join to reduce the size of one or both inputs.

This matters because:

1. **Distributed queries**: In distributed execution, shuffling data across the network is the dominant cost. Semi-join reduction sends only join key values (a compact Bloom filter or sorted key list) from one node to another, filtering rows before they are transferred. This can reduce network traffic by 90%+ when join selectivity is low.

2. **Star-schema queries**: TPC-H and TPC-DS queries frequently join a large fact table against multiple dimension tables. Semi-join reduction filters the fact table using dimension predicates before the expensive fact-dimension join.

3. **Multi-way joins**: For queries with 3+ tables, semi-join programs (sequences of semi-joins) can dramatically reduce intermediate result sizes before the actual joins execute.

4. **Gap in current coverage**: The gap analysis (CHORES.md, Gap #2) explicitly identifies semi-join reduction as missing. While RFC 0027 (Runtime Filters) and RFC 0045 (Runtime Filter Pushdown) cover runtime Bloom filters for local execution, neither addresses the systematic semi-join reduction planning that selects which semi-joins to introduce and in what order.

## Guide-level explanation

Semi-join reduction adds a planning phase that examines join graphs and inserts semi-join operations to pre-filter large relations before they participate in joins.

### Example

Consider TPC-H Query 5 (Local Supplier Volume):

```sql
SELECT n.n_name, SUM(l.l_extendedprice * (1 - l.l_discount)) AS revenue
FROM customer c, orders o, lineitem l, supplier s, nation n, region r
WHERE c.c_custkey = o.o_custkey
  AND l.l_orderkey = o.o_orderkey
  AND l.l_suppkey = s.s_suppkey
  AND c.c_nationkey = s.s_nationkey
  AND s.s_nationkey = n.n_nationkey
  AND n.n_regionkey = r.r_regionkey
  AND r.r_name = 'ASIA'
  AND o.o_orderdate >= '1994-01-01'
  AND o.o_orderdate < '1995-01-01'
GROUP BY n.n_name
ORDER BY revenue DESC;
```

Without semi-join reduction, the optimizer must join all six tables, potentially shuffling the entire `lineitem` table (6M+ rows in SF1).

With semi-join reduction, the optimizer produces a semi-join program:

1. Filter `region` by `r_name = 'ASIA'` (1 row)
2. Semi-join `nation` with `region` on `n_regionkey` (5 rows)
3. Semi-join `supplier` with filtered `nation` on `s_nationkey` (~2K rows)
4. Semi-join `customer` with filtered `nation` on `c_nationkey` (~30K rows)
5. Semi-join `orders` with filtered `customer` on `o_custkey` (~150K rows after date filter)
6. Semi-join `lineitem` with filtered `orders` on `l_orderkey` (~600K rows)

Now the actual join operates on substantially reduced inputs.

### API Usage

```rust
use ra_engine::semi_join_reduction::{
    SemiJoinReducer, SemiJoinReductionConfig,
};

let config = SemiJoinReductionConfig {
    min_selectivity_gain: 0.3,  // At least 30% row reduction
    max_semi_joins: 10,          // Limit program length
    prefer_bloom_filter: true,   // Use Bloom filters for distributed
    ..Default::default()
};

let reducer = SemiJoinReducer::new(config, &statistics);
let optimized_plan = reducer.reduce(logical_plan)?;
```

## Reference-level explanation

### Implementation Details

#### Semi-Join Reduction Strategy Selection

The optimizer evaluates whether to introduce semi-join reduction for each join in the plan by computing the *semi-join reduction ratio*:

$$\text{ratio}(R \ltimes S) = 1 - \frac{|R \ltimes S|}{|R|}$$

A high ratio means the semi-join eliminates many rows from R, making it worthwhile.

The decision factors are:
- **Selectivity**: The estimated fraction of R that survives the semi-join
- **Cost**: The cost of computing the semi-join vs. the savings from reduced join input
- **Distribution**: Whether R and S are co-located, and the cost of shipping semi-join keys

#### Semi-Join Program Generation

For multi-way joins, the optimizer generates a *semi-join program* -- an ordered sequence of semi-join operations that progressively reduces relation sizes before the actual joins. The algorithm uses a greedy heuristic based on Bernstein and Goodman (1981):

```
function generate_semi_join_program(join_graph, statistics):
    program = []
    changed = true
    while changed:
        changed = false
        best_reduction = 0
        best_semi_join = null
        for each edge (R, S, condition) in join_graph:
            for direction in [R $\ltimes$ S, S $\ltimes$ R]:
                reduction = estimate_reduction(direction, statistics)
                cost = estimate_semi_join_cost(direction)
                benefit = reduction * estimated_row_size - cost
                if benefit > best_reduction:
                    best_reduction = benefit
                    best_semi_join = direction
        if best_semi_join and best_reduction > threshold:
            program.append(best_semi_join)
            update_statistics(best_semi_join)
            changed = true
    return program
```

#### Data Structures

```rust
/// A semi-join reduction operation.
pub struct SemiJoinReduction {
    /// The relation being filtered.
    pub target: RelExpr,
    /// The relation providing filter keys.
    pub source: RelExpr,
    /// Join condition (equi-join keys).
    pub condition: Vec<(ColumnRef, ColumnRef)>,
    /// Estimated selectivity (fraction of target rows surviving).
    pub selectivity: f64,
    /// Transfer mechanism for distributed execution.
    pub transfer: SemiJoinTransfer,
}

/// How semi-join keys are transferred between nodes.
pub enum SemiJoinTransfer {
    /// Send raw key values (small sets).
    KeyList,
    /// Send a Bloom filter (larger sets, allows false positives).
    BloomFilter {
        expected_elements: u64,
        false_positive_rate: f64,
    },
    /// Send a range (min, max) for range-partitioned data.
    Range,
    /// No transfer needed (co-located data).
    Local,
}

/// Configuration for semi-join reduction.
pub struct SemiJoinReductionConfig {
    /// Minimum reduction ratio to introduce a semi-join.
    pub min_selectivity_gain: f64,
    /// Maximum number of semi-joins in a program.
    pub max_semi_joins: usize,
    /// Whether to prefer Bloom filters over key lists.
    pub prefer_bloom_filter: bool,
    /// Bloom filter false positive rate.
    pub bloom_fpr: f64,
    /// Maximum Bloom filter size in bytes.
    pub max_bloom_size: usize,
    /// Whether to consider bidirectional semi-joins.
    pub bidirectional: bool,
}
```

#### Cost Model Integration

Semi-join reduction cost is modeled as:

$$C_{\text{semi-join}} = C_{\text{build}} + C_{\text{transfer}} + C_{\text{probe}}$$

Where:
- $C_{\text{build}}$: Cost of building the key list or Bloom filter from the source relation
- $C_{\text{transfer}}$: Network transfer cost (0 if local)
- $C_{\text{probe}}$: Cost of probing the filter against the target relation

The benefit is:

$$B = (1 - \text{selectivity}) \times |R| \times \text{row\_size} \times C_{\text{IO}}$$

A semi-join is introduced when $B > C_{\text{semi-join}}$.

#### E-graph Integration

Semi-join reduction rules are added to the egg-based rewrite engine:

```rust
// Introduce semi-join reduction before expensive join
rewrite!("semi-join-reduction-left";
    "(join inner ?cond ?left ?right)" =>
    "(join inner ?cond (join semi ?cond ?left ?right) ?right)"
    if semi_join_beneficial_left(?cond, ?left, ?right)
),

// Introduce semi-join reduction on right side
rewrite!("semi-join-reduction-right";
    "(join inner ?cond ?left ?right)" =>
    "(join inner ?cond ?left (join semi (flip ?cond) ?right ?left))"
    if semi_join_beneficial_right(?cond, ?left, ?right)
),

// Bloom filter variant for distributed execution
rewrite!("bloom-semi-join-reduction";
    "(exchange ?dist (join inner ?cond ?left ?right))" =>
    "(exchange ?dist (join inner ?cond
        (bloom-filter ?cond ?left ?right) ?right))"
    if distributed_semi_join_beneficial(?cond, ?left, ?right)
),
```

### Integration Points

- **Cost model** (`ra-engine/src/cost.rs`): Add cost functions for semi-join operations including Bloom filter construction and probing.
- **Distributed optimizer** (`ra-engine/src/distributed_optimizer.rs`): Semi-join reduction is most impactful in distributed settings where it reduces network transfer.
- **Runtime filters** (RFC 0027, RFC 0045): Semi-join reduction at the plan level complements runtime Bloom filter pushdown. The planner decides where to place semi-joins; the runtime system builds the actual Bloom filters.
- **Statistics** (`ra-stats`): Requires accurate cardinality estimates and distinct value counts for join key columns.
- **Existing semi-join rules** (`ra-engine/src/semi_join.rs`): The existing rules handle subquery-to-semi-join conversion. This RFC adds the orthogonal optimization of introducing semi-joins to reduce join input sizes.

### Error Handling

- **Missing statistics**: Fall back to conservative estimates (assume 50% selectivity). Log a warning recommending ANALYZE.
- **Bloom filter sizing**: If the estimated key count exceeds `max_bloom_size`, fall back to a key list or skip the semi-join.
- **Infinite loops**: The semi-join program generator uses a fixed iteration limit (`max_semi_joins`) and requires strictly positive benefit to continue.

### Performance Considerations

- **Overhead**: Each semi-join adds a pass over the source relation to build the filter and a probe pass over the target. This overhead is only justified when selectivity is significantly less than 1.0.
- **Bloom filter false positives**: A 1% FPR Bloom filter uses ~9.6 bits per element. For 1M keys, this is ~1.2MB -- far less than transferring millions of full rows.
- **Diminishing returns**: After a few semi-joins, remaining relations are already small. The greedy algorithm naturally stops when benefit drops below threshold.

## Drawbacks

- **Planning overhead**: Evaluating semi-join reduction for all join pairs adds O(n^2) cost per join graph with n relations. For large join graphs (>15 tables), this could slow planning.
- **Statistics sensitivity**: Semi-join reduction heavily depends on accurate selectivity estimates. Poor cardinality estimates can cause the optimizer to introduce semi-joins that actually increase cost.
- **Bloom filter memory**: Large Bloom filters consume memory at the operator level. Multiple concurrent queries each building Bloom filters could cause memory pressure.
- **Interaction with join reordering**: Semi-join reduction constrains join order. The optimizer must consider semi-join placement jointly with join enumeration, increasing search space.

## Rationale and alternatives

### Why This Design?

The greedy semi-join program approach is well-studied (Bernstein & Goodman, 1981) and used in production systems (Oracle, SQL Server, Spark). It balances effectiveness with planning efficiency. The e-graph integration ensures semi-join reduction is considered alongside other optimizations without requiring a fixed optimization phase order.

### Alternative Approaches

1. **Full semi-join optimization (Bernstein & Chiu, 1981)**: Computes the theoretical optimal semi-join program. This is NP-hard in general and impractical for real-time query optimization.

2. **Runtime-only Bloom filters (current approach via RFC 0027/0045)**: Builds Bloom filters at runtime without planner involvement. This misses opportunities where the planner could choose better join orders knowing that semi-join reduction will be applied.

3. **Materialized semi-join views**: Pre-compute semi-join results for common join patterns. Adds storage overhead and staleness concerns.

### Impact of Not Doing This

Without semi-join reduction, distributed queries on star schemas will transfer significantly more data than necessary. TPC-H queries Q5, Q7, Q8, Q9 are particularly affected, with potential 5-10x performance gaps compared to systems that implement this optimization.

## Prior art

### Academic Research

- **Bernstein, P.A. and Goodman, N. (1981)**: "Power of Natural Semijoins." Established the theoretical foundation for semi-join reduction in distributed databases. Proved that finding the optimal semi-join program is NP-hard but greedy heuristics work well in practice.

- **Bernstein, P.A. and Chiu, D.W. (1981)**: "Using Semi-Joins to Solve Relational Queries." Extended the theory to full query programs with multiple semi-joins.

- **Yu, C.T. and Ozsoyoglu, M.Z. (1979)**: "An Algorithm for Tree-Query Membership of a Distributed Query." Early work on semi-join optimization for tree-structured queries.

- **Ramakrishnan, R. and Gehrke, J. (2003)**: "Database Management Systems", Chapter 22. Textbook coverage of semi-join reduction in distributed query processing.

### Industry Solutions

- **PostgreSQL**: Does not perform explicit semi-join reduction for regular joins. Has semi-join/anti-join node types for EXISTS/IN. The upcoming asynchronous execution in PG17+ could benefit from semi-join reduction.

- **Oracle**: Uses semi-join reduction extensively in RAC (Real Application Clusters) and Exadata. The "SEMIJOIN" hint forces the optimizer to use semi-join reduction. Oracle's Bloom filter pruning in partition-wise joins is a form of semi-join reduction.

- **Apache Spark**: Implements dynamic partition pruning (DPP) which is a specialized form of semi-join reduction for partitioned tables. Spark 3.0+ builds Bloom filters from dimension table predicates and pushes them to fact table scans.

- **Apache Calcite**: Has `SemiJoinRule` that converts joins to semi-joins when only left-side columns are needed, but lacks the more aggressive reduction optimization.

- **Presto/Trino**: Implements dynamic filtering where the coordinator builds Bloom filters from small-side scan results and pushes them to large-side scans. This is semi-join reduction implemented at the execution layer.

- **DuckDB**: Implements perfect hash join with early filtering, which achieves similar effects to semi-join reduction for in-memory execution.

### What We Can Learn

Oracle and Spark demonstrate that semi-join reduction is most impactful in distributed and partitioned settings. The key insight is that the planner should reason about which semi-joins to introduce (a planning decision) separately from how to execute them (runtime Bloom filters, key lists, etc.).

## Unresolved questions

- **Join order interaction**: Should semi-join reduction be a separate optimization phase that runs after join enumeration, or should it be integrated into the join enumeration search space? The e-graph approach naturally handles this, but may increase e-graph size significantly.

- **Bloom filter sizing**: How should we size Bloom filters when cardinality estimates are uncertain? Oversizing wastes memory; undersizing increases false positive rates.

- **Multi-column joins**: How should semi-join reduction handle composite join keys? Build separate Bloom filters per column or a single filter on the concatenated key?

- **Cyclic join graphs**: The greedy algorithm assumes acyclic join graphs. How should we handle cycles (e.g., self-joins, triangles)?

## Future possibilities

### Natural Extensions

- **Adaptive semi-join reduction**: Monitor actual selectivity at runtime and disable semi-joins that don't meet expected reduction ratios.
- **Semi-join materialization**: For frequently executed queries, cache semi-join results (dimension key Bloom filters) and reuse them across queries.
- **Cross-query semi-join sharing**: Multiple concurrent queries on the same star schema could share dimension Bloom filters.

### Long-term Vision

Semi-join reduction is a key building block for efficient distributed query execution. Combined with the existing runtime filter infrastructure (RFCs 0027, 0045), it forms a complete pipeline: the planner decides where to place semi-joins, and the runtime system executes them efficiently using Bloom filters or key lists. This positions RA for competitive performance on distributed benchmarks (TPC-H, TPC-DS distributed variants).

## Implementation Plan

### Phase 1: Core Semi-Join Reduction (Week 1)
- Implement `SemiJoinReductionConfig` and cost model extensions
- Implement the greedy semi-join program generator
- Add e-graph rewrite rules for semi-join introduction

### Phase 2: Distributed Integration (Week 2)
- Integrate with distributed optimizer for Bloom filter transfer decisions
- Add network cost modeling for semi-join key transfer
- Implement Bloom filter sizing heuristics

### Phase 3: Testing and Tuning (Week 3)
- Test with TPC-H queries (especially Q5, Q7, Q8, Q9)
- Benchmark distributed execution with and without semi-join reduction
- Tune thresholds based on empirical results

### Estimated Effort: 2-3 weeks
