# RFC 0043: GroupJoin - Eager Aggregation Before Join

- Start Date: 2026-03-22
- Author: RA Contributors
- Status: Draft
- Tracking Issue: TBD

## Summary

Optimize aggregate-after-join queries by aggregating one input relation before the join (eager aggregation) instead of aggregating after the join (lazy aggregation). This reduces intermediate result size and significantly improves performance for OLAP queries with decomposable aggregate functions.

## Motivation

A common OLAP query pattern aggregates facts after joining with dimensions:

```sql
SELECT products.category, SUM(sales.amount)
FROM sales JOIN products ON sales.product_id = products.id
GROUP BY products.category
```

**Current execution** (lazy aggregation):
1. Join sales $\bowtie$ products: 10M rows
2. Aggregate: 10M rows -> 5 categories

**Problem**: Join produces 10M intermediate rows, most of which get aggregated away.

**Optimal execution** (eager aggregation):
1. Pre-aggregate sales: 10M rows -> 1000 products
2. Join aggregated_sales $\bowtie$ products: 1000 rows
3. Final aggregate: 1000 rows -> 5 categories

**Benefit**: 10,000x fewer rows in join intermediate result.

### Use Cases

1. **Sales Analytics**: "Total revenue per product category"
2. **Event Analytics**: "Count events per user region" (join events -> users -> regions)
3. **Supply Chain**: "Total inventory cost per warehouse region"
4. **Ad Analytics**: "Total clicks per advertiser country" (clicks -> ads -> advertisers -> countries)
5. **IoT Metrics**: "Average sensor value per device manufacturer"

All follow the pattern: **Aggregate fact table -> Join dimension -> Final aggregate**

## Guide-level explanation

GroupJoin is an optimization that moves aggregation before a join when the aggregate function is decomposable and the grouping keys include the join keys.

### Before GroupJoin (Lazy Aggregation)

```sql
SELECT products.category, SUM(sales.amount) AS total_revenue
FROM sales
JOIN products ON sales.product_id = products.id
GROUP BY products.category;
```

**Execution Plan**:
```
Aggregate(category, SUM(amount))
  |--- HashJoin(sales.product_id = products.id)
      |--- Scan(sales)        -- 10M rows
      |--- Scan(products)     -- 1K rows
```

**Cost**: 10M row join, then aggregate

### After GroupJoin (Eager Aggregation)

```sql
-- Optimizer rewrites to:
WITH aggregated_sales AS (
  SELECT product_id, SUM(amount) AS amount_sum
  FROM sales
  GROUP BY product_id
)
SELECT products.category, SUM(amount_sum) AS total_revenue
FROM aggregated_sales
JOIN products ON aggregated_sales.product_id = products.id
GROUP BY products.category;
```

**Execution Plan**:
```
Aggregate(category, SUM(amount_sum))
  |--- HashJoin(aggregated_sales.product_id = products.id)
      |--- Aggregate(product_id, SUM(amount) AS amount_sum)
      |   |--- Scan(sales)   -- 10M rows -> 1K groups
      |--- Scan(products)    -- 1K rows
```

**Cost**: 1K row join, much cheaper!

### Decomposable Aggregates

GroupJoin only works for **decomposable** aggregate functions:
- `SUM(x)`: `SUM(SUM(x))` = `SUM(x)` [x]
- `COUNT(*)`: `SUM(COUNT(*))` = `COUNT(*)` [x]
- `MIN(x)`: `MIN(MIN(x))` = `MIN(x)` [x]
- `MAX(x)`: `MAX(MAX(x))` = `MAX(x)` [x]
- `AVG(x)`: `SUM(sum_x) / SUM(count_x)` = `AVG(x)` [x] (requires rewriting)
- `COUNT(DISTINCT x)`: [FAIL] NOT decomposable (requires full data)
- `MEDIAN(x)`: [FAIL] NOT decomposable

### Preconditions

GroupJoin applies when:
1. **Aggregate function is decomposable**: SUM, COUNT, MIN, MAX, AVG
2. **Grouping keys include join keys**: `GROUP BY product_id, category` (includes product_id)
3. **Join is many-to-one or one-to-one**: sales -> products (many sales per product)
4. **Pre-aggregation reduces cardinality**: 10M sales -> 1K products

## Reference-level explanation

### Algorithm

#### Phase 1: Pattern Recognition

Detect the pattern:
```
Aggregate(G, F)
  |--- Join(R $\bowtie$ S on R.k = S.k)
```

Where:
- `G` = grouping columns
- `F` = aggregate functions
- `R.k` $\in$ G (join key is in grouping columns)

#### Phase 2: Decomposability Check

For each aggregate function `F`, verify it's decomposable:
- `SUM(expr)` -> `SUM(SUM(expr))`
- `COUNT(*)` -> `SUM(COUNT(*))`
- `MIN(expr)` -> `MIN(MIN(expr))`
- `MAX(expr)` -> `MAX(MAX(expr))`
- `AVG(expr)` -> rewrite to `SUM(expr) / COUNT(expr)`, then apply SUM/COUNT rules

Reject if:
- `COUNT(DISTINCT ...)`: Not decomposable
- `MEDIAN(...)`, `PERCENTILE_CONT(...)`: Not decomposable
- User-defined aggregates: Unknown decomposability (conservative: reject)

#### Phase 3: Cardinality Estimation

Estimate benefit:
1. **Without GroupJoin**: Join cardinality $\times$ aggregate cost
2. **With GroupJoin**: Pre-aggregate cardinality $\times$ join cardinality $\times$ final aggregate cost

Apply transformation only if **benefit > overhead**.

**Example**:
- Sales: 10M rows, 1K distinct product_ids
- Products: 1K rows
- Join output: 10M rows

**Cost without GroupJoin**:
- Join: 10M $\times$ 1K (hash table size) = 10B cost units
- Aggregate: 10M rows -> 5 categories = 10M cost units
- **Total**: 10.01B cost units

**Cost with GroupJoin**:
- Pre-aggregate: 10M rows -> 1K groups = 10M cost units
- Join: 1K $\times$ 1K = 1M cost units
- Final aggregate: 1K rows -> 5 categories = 1K cost units
- **Total**: 11M cost units

**Speedup**: ~1000x

#### Phase 4: Transformation

Rewrite the query:

**Input**:
```
Aggregate(category, SUM(amount))
  |--- Join(sales.product_id = products.id)
      |--- Scan(sales)
      |--- Scan(products)
```

**Output**:
```
Aggregate(category, SUM(partial_sum))
  |--- Join(agg_sales.product_id = products.id)
      |--- Aggregate(product_id, SUM(amount) AS partial_sum)
      |   |--- Scan(sales)
      |--- Scan(products)
```

**Steps**:
1. Identify fact table (R) and dimension table (S)
2. Create pre-aggregation on fact table: `Aggregate(R.k, partial_F)`
3. Replace fact table in join with pre-aggregated version
4. Adjust outer aggregate to combine partial results

### Implementation Details

**Data Structures**:

```rust
pub struct GroupJoinOptimizer {
    /// Cost model for benefit estimation
    cost_model: Box<dyn CostModel>,
    /// Statistics for cardinality estimation
    stats_provider: Arc<dyn StatisticsProvider>,
}

pub struct GroupJoinCandidate {
    /// Outer aggregate operator
    aggregate: Aggregate,
    /// Join operator
    join: Join,
    /// Fact table to pre-aggregate
    fact_table: RelExpr,
    /// Dimension table (remains unchanged)
    dimension_table: RelExpr,
    /// Join key columns
    join_keys: Vec<String>,
    /// Decomposable aggregate functions
    decomposable_aggs: Vec<DecomposedAggregate>,
}

pub struct DecomposedAggregate {
    /// Original aggregate function (SUM, AVG, etc.)
    original: AggregateFunction,
    /// Partial aggregate (SUM -> SUM, AVG -> SUM + COUNT)
    partial: Vec<AggregateFunction>,
    /// Combiner aggregate (SUM -> SUM, AVG -> SUM/SUM)
    combiner: AggregateFunction,
}
```

**Transformation Steps**:

1. **Detect Pattern**:
   ```rust
   fn detect_groupjoin_pattern(expr: &RelExpr) -> Option<GroupJoinCandidate> {
       if let RelExpr::Aggregate { input, group_by, aggregates, .. } = expr {
           if let RelExpr::Join { left, right, on, .. } = input.as_ref() {
               // Check if join keys are in group_by
               // Identify fact vs dimension table
               // Return candidate
           }
       }
       None
   }
   ```

2. **Check Decomposability**:
   ```rust
   fn is_decomposable(agg: &AggregateFunction) -> bool {
       matches!(agg,
           AggregateFunction::Sum(_) |
           AggregateFunction::Count(_) |
           AggregateFunction::Min(_) |
           AggregateFunction::Max(_) |
           AggregateFunction::Avg(_)  // Requires rewriting
       )
   }
   ```

3. **Estimate Benefit**:
   ```rust
   fn estimate_benefit(candidate: &GroupJoinCandidate, stats: &Statistics) -> f64 {
       let fact_card = stats.row_count(&candidate.fact_table);
       let agg_card = stats.distinct_count(&candidate.fact_table, &candidate.join_keys);
       let dim_card = stats.row_count(&candidate.dimension_table);

       let cost_without = fact_card * dim_card;  // Join cost
       let cost_with = fact_card + (agg_card * dim_card);  // Pre-agg + Join cost

       cost_without / cost_with  // Speedup factor
   }
   ```

4. **Apply Transformation**:
   ```rust
   fn apply_groupjoin(candidate: GroupJoinCandidate) -> RelExpr {
       // Create pre-aggregation
       let pre_agg = Aggregate {
           input: candidate.fact_table,
           group_by: candidate.join_keys.clone(),
           aggregates: candidate.decomposable_aggs.iter()
               .map(|da| da.partial.clone())
               .flatten()
               .collect(),
       };

       // Update join
       let new_join = Join {
           left: Box::new(pre_agg),
           right: candidate.dimension_table,
           on: candidate.join.on,
           ..candidate.join
       };

       // Update outer aggregate
       Aggregate {
           input: Box::new(new_join),
           group_by: candidate.aggregate.group_by,
           aggregates: candidate.decomposable_aggs.iter()
               .map(|da| da.combiner.clone())
               .collect(),
       }
   }
   ```

### Integration Points

**Interactions with**:
- **Join Ordering**: GroupJoin affects join cardinality estimates
- **Aggregate Pushdown**: Complementary optimizations (GroupJoin vs filter pushdown)
- **Multi-Way Joins**: Can apply GroupJoin at multiple levels
- **Parallel Execution**: Pre-aggregation can be parallelized

**Rule Categories**:
- `rules/logical/groupjoin-eager-aggregation.rra`
- `rules/physical/groupjoin-partial-aggregate.rra`
- `rules/cost-models/groupjoin-benefit-estimation.rra`

### Error Handling

**Transformation fails if**:
1. Aggregate function is not decomposable (MEDIAN, COUNT DISTINCT)
2. Join key not in grouping columns
3. Cost model predicts no benefit (e.g., fact table already has low cardinality)
4. Join is many-to-many (cannot determine which side to pre-aggregate)

**Fallback**: Use original lazy aggregation plan.

### Performance Considerations

**Expected Speedup**:
- **High reduction factor** (10M -> 1K): 100x-1000x faster
- **Medium reduction factor** (10M -> 100K): 10x-50x faster
- **Low reduction factor** (10M -> 5M): 2x-5x faster
- **No reduction** (already unique on join key): Slight overhead

**Space Overhead**:
- Pre-aggregation hash table: O(distinct join keys)
- For high-cardinality keys: May spill to disk
- Worst case: Pre-aggregation cost > join cost (don't apply)

**Trade-offs**:
- **Best case**: Join key has low cardinality, aggregate reduces 100x+
- **Worst case**: Join key is already unique, aggregation overhead wasted
- **Solution**: Cost-based decision using NDV (number of distinct values) statistics

## Drawbacks

### Complexity Cost
- Adds ~800 lines of code for pattern detection and rewriting
- Requires decomposability analysis for aggregate functions
- AVG rewriting is non-trivial (SUM/COUNT decomposition)

### Maintenance Burden
- New aggregate functions must be marked decomposable or not
- User-defined aggregates: Default to non-decomposable (conservative)
- Interaction with other aggregate optimizations (e.g., hash vs sort aggregate)

### Incorrect Transformations
- If cardinality estimates are wrong, may apply GroupJoin when it's not beneficial
- Example: Statistics say 1K distinct products, reality is 10M (groupjoin makes it slower)
- Mitigation: Adaptive execution (detect and revert)

### AVG Rewriting Complexity
- AVG(x) must be rewritten as SUM(sum_x) / SUM(count_x)
- Requires tracking two intermediate values
- More complex than other decomposable aggregates

## Rationale and alternatives

### Why This Design?

**GroupJoin is proven**:
- Academic foundation: Yan & Larson (1995), Chaudhuri & Shim (1994)
- Production use: SQL Server, DB2, Teradata
- Core technique in columnar databases (Vertica, Redshift)

**Significant performance gains**:
- 100x-1000x speedup for common OLAP queries
- Reduces memory pressure (smaller intermediate results)
- Enables queries that would otherwise OOM

**Cost-based applicability**:
- Can estimate benefit using NDV statistics
- Only applies when beneficial

### Alternative Approaches

#### 1. Always Apply (No Cost Model)

**Approach**: Apply GroupJoin whenever pattern matches, regardless of cost

**Pros**: Simpler implementation
**Cons**:
- May apply when not beneficial (join key already unique)
- Wastes CPU on unnecessary aggregation

#### 2. Lazy Aggregation Pushdown

**Approach**: Push aggregation below join, but don't change aggregate semantics

**Pros**: Less invasive transformation
**Cons**:
- Doesn't achieve full benefit of pre-aggregation
- Still produces large intermediate result

#### 3. Runtime Adaptive

**Approach**: Start with lazy aggregation, switch to eager if intermediate result is large

**Pros**: Adaptive to data distribution
**Cons**:
- Requires executor changes (not just optimizer)
- Overhead from runtime monitoring
- Lost work if switching mid-execution

### Impact of Not Doing This

**Without GroupJoin**:
- OLAP queries 100x-1000x slower
- Memory pressure from large intermediate results
- Queries may OOM and fail
- Competitive disadvantage vs SQL Server, Teradata, Redshift

## Prior art

### Academic Research

**"Eager Aggregation and Lazy Aggregation" (Yan & Larson, 1995)**
- Introduced GroupJoin optimization
- Showed conditions for correctness (decomposability + join key in grouping)
- Benchmarks: 10x-1000x speedup on TPC-D queries

**"Optimization of Queries with Aggregates" (Chaudhuri & Shim, 1994)**
- Generalized aggregate pushdown techniques
- Cost models for aggregate placement
- Integration with join ordering

**"Cost-Based Optimization for Magic Sets" (Seshadri et al., 1996)**
- Combined Magic Sets + GroupJoin
- Recursive queries with aggregation

### Industry Solutions

**SQL Server**:
- **Full support**: GroupJoin implemented since SQL Server 2000
- **Cost-based**: Uses cardinality estimates to decide
- **SHOWPLAN**: Shows "Stream Aggregate" before join in execution plan

**IBM DB2**:
- **"Star Join" optimization**: Applies GroupJoin to fact-dimension joins
- **Statistics-driven**: Requires accurate NDV statistics
- **Bitmap filtering**: Combines GroupJoin with bloom filters

**Teradata**:
- **AMP-local aggregation**: Pre-aggregates on each node before redistribution
- **Two-phase aggregation**: Local + global (similar to GroupJoin)
- **Critical for performance**: Star schema queries rely on this

**PostgreSQL**:
- **Limited support**: Aggregate pushdown exists but not full GroupJoin
- **Issue**: Cannot push aggregation below all join types
- **Workaround**: Users manually rewrite queries with CTEs

**DuckDB**:
- **Partial support**: Pushes aggregation in some cases
- **Columnar execution**: Benefits less from GroupJoin (already efficient)
- **Roadmap**: Full GroupJoin listed as enhancement

**Apache Calcite**:
- **AggregateJoinTransposeRule**: Implements GroupJoin transformation
- **Widely used**: Adopted by Drill, Flink, Hive, Spark
- **Configurable**: Can enable/disable via planner rules

### What We Can Learn

**Key insights**:
1. **Cost model is critical**: Don't apply blindly, use NDV statistics
2. **AVG is complex**: Requires SUM+COUNT decomposition and careful rewriting
3. **Multi-way joins**: Can apply GroupJoin at multiple levels (cascade)
4. **Explain plan clarity**: Users should see "Pre-Aggregation" in plan
5. **Statistics quality matters**: Bad NDV estimates -> wrong decision

**Calcite lesson**: Implement as modular rule (easy to enable/disable).

## Unresolved questions

**Design Questions**:
1. **Cost threshold**: What speedup factor justifies transformation? 2x? 5x? 10x?
2. **AVG decomposition**: Always decompose AVG or only when beneficial?
3. **User control**: Should users be able to force/disable GroupJoin via hint?

**Implementation Strategy**:
1. Where in optimizer pipeline? After join reordering or before?
2. Interaction with aggregate splitting (partial vs final)?
3. Should we support cascading GroupJoins (multiple fact tables)?

**Integration Questions**:
1. Interaction with parallel aggregation: Two-phase + GroupJoin?
2. Distributed execution: Where to do pre-aggregation (coordinator vs workers)?
3. EXPLAIN output: How to clearly show pre-aggregation was applied?

**Out of Scope** (for initial RFC):
- Non-decomposable aggregates (MEDIAN, MODE, PERCENTILE)
- GroupJoin for many-to-many joins
- User-defined aggregate decomposability annotations
- Cascading GroupJoins (multiple levels)

## Future possibilities

### Natural Extensions

#### 1. Multi-Level GroupJoin
- Apply GroupJoin to multi-way joins
- Example: sales -> products -> categories
- Pre-aggregate at each level

#### 2. GroupJoin + Semi-Join Reduction
- Combine with bloom filter pushdown
- Pre-aggregate + filter in single pass
- Critical for distributed star schema queries

#### 3. User-Defined Aggregate Decomposition
- Allow users to annotate UDAs as decomposable
- Provide decomposition rules (partial + combiner)
- Enables GroupJoin for custom aggregates

#### 4. Adaptive GroupJoin
- Decide at runtime based on actual cardinality
- Start with lazy aggregation
- Switch to eager if intermediate result is large
- Requires executor support

### Long-term Vision

**Intelligent Aggregate Placement**:
- Cost-based optimization across aggregate, join, and filter placement
- Consider all permutations, pick lowest cost
- Machine learning to learn optimal placement heuristics

**OLAP Query Acceleration**:
- GroupJoin as foundation for OLAP optimizations
- Combine with: columnar execution, vectorization, late materialization
- Target: Competitive with Redshift, Snowflake, BigQuery

**Materialized Aggregate Views**:
- Pre-compute common pre-aggregations
- Query rewriter: Use materialized aggregate instead of base table
- GroupJoin + view matching = very fast OLAP

---

## Implementation Roadmap

### Phase 1: Basic GroupJoin (SUM, COUNT, MIN, MAX)
- Pattern detection: Aggregate(Join(...))
- Decomposability check
- Cost-based applicability
- ~600 LOC
- **Benefit**: 100x+ speedup for simple aggregates

### Phase 2: AVG Support
- Rewrite AVG to SUM/COUNT
- Track intermediate values
- Final combiner
- ~200 LOC
- **Benefit**: Covers 90% of OLAP queries

### Phase 3: Multi-Level GroupJoin
- Cascade through multi-way joins
- ~300 LOC
- **Benefit**: 1000x+ speedup for deep star schemas

**Total effort**: 3-4 weeks for full implementation

---

## References

- Yan, W. P., & Larson, P.-A. (1995). *Eager aggregation and lazy aggregation*. VLDB '95.
- Chaudhuri, S., & Shim, K. (1994). *Including group-by in query optimization*. VLDB '94.
- Apache Calcite: [AggregateJoinTransposeRule](https://calcite.apache.org/javadocAggregate/org/apache/calcite/rel/rules/AggregateJoinTransposeRule.html)
- SQL Server: [Execution Plan Basics](https://docs.microsoft.com/en-us/sql/relational-databases/performance/execution-plans)
