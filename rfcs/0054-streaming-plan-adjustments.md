# RFC 0054: Streaming Plan Adjustments for Pre-compiled Plans

- Start Date: 2026-03-22
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: N/A

## Summary

Enable dynamic adjustment of pre-compiled execution plans as statistics and database facts change, without requiring expensive full re-optimization. This provides a lightweight, incremental replanning mechanism that keeps cached plans effective as data evolves, particularly valuable for stored procedures and prepared statements.

## Motivation

Pre-compiled plans (stored procedures, prepared statements, plan caches) become **stale** over time as:

1. **Table cardinalities change**: Data grows or shrinks significantly
2. **Data distributions shift**: Skew changes, new hotspots emerge
3. **Indexes are added or dropped**: Available access paths change
4. **Statistics are updated**: ANALYZE reveals new patterns

**Problem:**

- **Full re-optimization is expensive**: Complex queries take seconds to re-optimize
- **Automatic re-compilation is disruptive**: Unpredictable latency spikes during execution
- **Manual invalidation is error-prone**: DBAs must remember to invalidate plans

**Current workarounds:**

- PostgreSQL: Manual plan cache invalidation via `pg_stat_statements_reset()`
- Oracle: Automatic plan baselines (but heavyweight, requires tuning)
- SQL Server: Forced recompilation with `OPTION (RECOMPILE)` (full re-optimization)

**This RFC proposes:**

- **Lightweight plan adjustment**: Modify only affected parts of the plan
- **Threshold-based triggering**: Detect when statistics diverge beyond acceptable bounds
- **Incremental replanning**: Fast adjustments (milliseconds) instead of full optimization (seconds)
- **Plan fingerprinting**: Track which statistics influenced original plan choices

## Guide-level explanation

Imagine you have a stored procedure optimized for a `customers` table with 10,000 rows. The optimizer chose a nested loop join. Over time, the table grows to 1,000,000 rows, making the nested loop inefficient, but the plan cache still uses the old plan.

**Without streaming adjustments:**

```sql
-- Plan cached when customers had 10,000 rows
-- Still using nested loop join after customers grows to 1M rows
-- Performance degrades 100x
SELECT * FROM customers c JOIN orders o ON c.id = o.customer_id WHERE c.status = 'active';
```

**With streaming adjustments:**

```rust
// Ra detects cardinality divergence
let plan = cache.get_plan(query_id)?;

if plan.is_stale(&current_stats) {
    // Fast incremental adjustment (milliseconds)
    let adjusted_plan = plan.adjust_for_new_stats(&current_stats)?;
    cache.update_plan(query_id, adjusted_plan);
}
```

The adjusted plan switches from nested loop to hash join without re-running the full optimizer.

### Example Usage

**1. Plan Creation with Fingerprint:**

```rust
use ra_core::plan_cache::{PlanCache, PlanFingerprint};

let plan = optimizer.optimize(&query)?;

// Record which statistics influenced this plan
let fingerprint = PlanFingerprint {
    table_cardinalities: vec![
        ("customers".to_string(), 10_000),
        ("orders".to_string(), 50_000),
    ],
    column_ndistinct: vec![
        ("customers.status".to_string(), 5),
        ("orders.status".to_string(), 8),
    ],
    index_list: vec![
        "idx_customers_status".to_string(),
        "idx_orders_customer_id".to_string(),
    ],
    join_selectivities: vec![
        (("customers".to_string(), "orders".to_string()), 0.1),
    ],
};

plan_cache.store(query_id, plan, fingerprint);
```

**2. Staleness Detection:**

```rust
let cached_entry = plan_cache.get(query_id)?;

if cached_entry.fingerprint.is_stale(&current_stats) {
    println!("Plan is stale:");
    println!("  customers: {} -> {}",
        cached_entry.fingerprint.table_cardinalities["customers"],
        current_stats.table_cardinality("customers"));

    // Trigger streaming adjustment
    let adjusted_plan = cached_entry.plan.adjust(
        &cached_entry.fingerprint,
        &current_stats,
        AdjustmentStrategy::Conservative,
    )?;

    plan_cache.update(query_id, adjusted_plan);
}
```

**3. Incremental Adjustment:**

```rust
impl ExecutionPlan {
    pub fn adjust(
        &self,
        old_fingerprint: &PlanFingerprint,
        new_stats: &Statistics,
        strategy: AdjustmentStrategy,
    ) -> Result<ExecutionPlan> {
        let mut adjusted = self.clone();

        // Identify divergent statistics
        let changes = old_fingerprint.diff(new_stats);

        for change in changes {
            match change {
                StatChange::CardinalityIncrease { table, old, new } if new > old * 10 => {
                    // Significant cardinality increase
                    adjusted = adjusted.replace_join_method(table, JoinMethod::HashJoin)?;
                }
                StatChange::IndexAdded { index_name } => {
                    // New index available
                    adjusted = adjusted.add_index_scan(index_name)?;
                }
                StatChange::SelectivityChange { predicate, old_sel, new_sel } => {
                    // Predicate selectivity changed
                    adjusted = adjusted.reorder_filters(predicate, new_sel)?;
                }
                _ => {}
            }
        }

        adjusted.recalculate_costs(new_stats)?;
        Ok(adjusted)
    }
}
```

## Reference-level explanation

### Implementation Details

**Plan Fingerprint Structure:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanFingerprint {
    /// Table cardinalities when plan was created
    pub table_cardinalities: HashMap<String, usize>,

    /// Column distinct value counts
    pub column_ndistinct: HashMap<String, usize>,

    /// List of available indexes
    pub index_list: HashSet<String>,

    /// Join selectivities for pairs of tables
    pub join_selectivities: HashMap<(String, String), f64>,

    /// Predicate selectivities
    pub predicate_selectivities: HashMap<String, f64>,

    /// Optimization parameters used
    pub optimizer_config: OptimizerConfig,

    /// Timestamp when plan was created
    pub created_at: Instant,
}

impl PlanFingerprint {
    /// Check if current statistics have diverged significantly
    pub fn is_stale(&self, current_stats: &Statistics) -> bool {
        // Threshold: 2x cardinality change triggers staleness
        for (table, old_card) in &self.table_cardinalities {
            let current_card = current_stats.table_cardinality(table);
            if current_card > old_card * 2 || current_card < old_card / 2 {
                return true;
            }
        }

        // Check for added/removed indexes
        let current_indexes = current_stats.available_indexes();
        if !self.index_list.is_subset(&current_indexes) {
            // Index was dropped
            return true;
        }
        if current_indexes.difference(&self.index_list).count() > 0 {
            // New indexes available
            return true;
        }

        false
    }

    /// Compute difference between old and new statistics
    pub fn diff(&self, new_stats: &Statistics) -> Vec<StatChange> {
        let mut changes = Vec::new();

        // Cardinality changes
        for (table, old_card) in &self.table_cardinalities {
            let new_card = new_stats.table_cardinality(table);
            if new_card != *old_card {
                changes.push(StatChange::CardinalityChange {
                    table: table.clone(),
                    old: *old_card,
                    new: new_card,
                });
            }
        }

        // Index changes
        let current_indexes = new_stats.available_indexes();
        for added_index in current_indexes.difference(&self.index_list) {
            changes.push(StatChange::IndexAdded {
                index_name: added_index.clone(),
            });
        }
        for removed_index in self.index_list.difference(&current_indexes) {
            changes.push(StatChange::IndexRemoved {
                index_name: removed_index.clone(),
            });
        }

        changes
    }
}

#[derive(Debug, Clone)]
pub enum StatChange {
    CardinalityChange { table: String, old: usize, new: usize },
    IndexAdded { index_name: String },
    IndexRemoved { index_name: String },
    SelectivityChange { predicate: String, old_sel: f64, new_sel: f64 },
    NDistinctChange { column: String, old: usize, new: usize },
}
```

**Adjustment Strategies:**

```rust
pub enum AdjustmentStrategy {
    /// Only adjust if change is >10x
    Conservative,

    /// Adjust if change is >2x
    Moderate,

    /// Adjust on any statistical change
    Aggressive,

    /// Custom thresholds
    Custom { cardinality_threshold: f64, selectivity_threshold: f64 },
}
```

**Plan Adjustment Algorithm:**

```rust
impl ExecutionPlan {
    pub fn adjust(
        &self,
        old_fingerprint: &PlanFingerprint,
        new_stats: &Statistics,
        strategy: AdjustmentStrategy,
    ) -> Result<ExecutionPlan> {
        let changes = old_fingerprint.diff(new_stats);
        let mut adjusted = self.clone();

        for change in changes {
            match (change, &strategy) {
                // Large table growth: switch to hash join
                (StatChange::CardinalityChange { table, old, new }, _)
                    if new > old * strategy.cardinality_threshold() => {
                    adjusted = self.replace_join_method(&table, JoinMethod::HashJoin)?;
                }

                // Table shrinkage: consider nested loop
                (StatChange::CardinalityChange { table, old, new }, _)
                    if new < old / strategy.cardinality_threshold() => {
                    adjusted = self.replace_join_method(&table, JoinMethod::NestedLoop)?;
                }

                // New index: add index scan path
                (StatChange::IndexAdded { index_name }, _) => {
                    if let Some(better_plan) = self.try_add_index_scan(&index_name, new_stats)? {
                        adjusted = better_plan;
                    }
                }

                // Index removed: fall back to seq scan
                (StatChange::IndexRemoved { index_name }, _) => {
                    adjusted = self.replace_index_scan_with_seq_scan(&index_name)?;
                }

                _ => {}
            }
        }

        // Recalculate costs with new statistics
        adjusted.recalculate_costs(new_stats)?;

        // Verify adjusted plan is better than original
        if adjusted.total_cost() < self.total_cost() * 1.1 {
            Ok(adjusted)
        } else {
            // Adjustment didn't help, trigger full re-optimization
            Err(PlanAdjustmentError::NoImprovement)
        }
    }

    fn replace_join_method(&self, table: &str, new_method: JoinMethod) -> Result<ExecutionPlan> {
        // Traverse plan tree, find joins involving 'table', replace join method
        // This is a localized change, doesn't require full optimization
        todo!()
    }

    fn try_add_index_scan(&self, index_name: &str, stats: &Statistics) -> Result<Option<ExecutionPlan>> {
        // Check if the new index can improve a table scan
        // Replace SeqScan with IndexScan if beneficial
        todo!()
    }
}
```

**Plan Cache Integration:**

```rust
pub struct PlanCache {
    plans: HashMap<u64, CachedPlanEntry>,
    adjustment_strategy: AdjustmentStrategy,
}

pub struct CachedPlanEntry {
    pub plan: ExecutionPlan,
    pub fingerprint: PlanFingerprint,
    pub last_adjusted: Instant,
    pub adjustment_count: usize,
}

impl PlanCache {
    pub fn get_plan(&mut self, query_hash: u64, current_stats: &Statistics) -> Result<ExecutionPlan> {
        let entry = self.plans.get_mut(&query_hash)?;

        // Check staleness
        if entry.fingerprint.is_stale(current_stats) {
            // Try incremental adjustment
            match entry.plan.adjust(&entry.fingerprint, current_stats, &self.adjustment_strategy) {
                Ok(adjusted_plan) => {
                    entry.plan = adjusted_plan;
                    entry.last_adjusted = Instant::now();
                    entry.adjustment_count += 1;

                    // After N adjustments, force full re-optimization
                    if entry.adjustment_count > 5 {
                        return Err(PlanCacheError::TooManyAdjustments);
                    }
                }
                Err(PlanAdjustmentError::NoImprovement) => {
                    // Adjustment didn't help, evict and trigger full re-optimization
                    self.plans.remove(&query_hash);
                    return Err(PlanCacheError::StalePlan);
                }
            }
        }

        Ok(entry.plan.clone())
    }
}
```

### Integration Points

**1. RFC 0052 (Progressive Re-Optimization):**

- **Streaming adjustments** = fast, lightweight changes
- **Progressive re-optimization** = full re-optimization in background
- Combined: Try streaming adjustment first, fall back to progressive re-opt if adjustment fails

**2. Plan Cache:**

- Store `PlanFingerprint` alongside each cached plan
- Check staleness on every cache access
- Evict plans that fail adjustment after N attempts

**3. Statistics Subsystem:**

- Statistics module emits events on UPDATE
- Plan cache listens for events, marks affected plans as potentially stale
- Lazy evaluation: check staleness only when plan is accessed

**4. Stored Procedures (RFC 0053):**

- Procedures with embedded queries benefit from streaming adjustments
- Each query in procedure has its own fingerprint
- Adjust queries independently as statistics change

### Error Handling

**Adjustment Failures:**

```rust
#[derive(Debug, Error)]
pub enum PlanAdjustmentError {
    #[error("Adjustment did not improve plan cost")]
    NoImprovement,

    #[error("Cannot adjust plan due to missing statistics")]
    MissingStatistics,

    #[error("Plan structure does not support incremental adjustment")]
    UnsupportedPlanStructure,

    #[error("Too many adjustments, full re-optimization required")]
    TooManyAdjustments,
}
```

**Fallback:**

- If adjustment fails, evict plan from cache
- Next access triggers full re-optimization
- Emit warning to user/logs

### Performance Considerations

**Adjustment Speed:**

- Target: <10ms for plan adjustment (vs. seconds for full optimization)
- Achieved by:
  - No search (just local modifications)
  - No cost-based enumeration
  - Direct replacements based on statistical thresholds

**Staleness Check Overhead:**

- Checking `is_stale()` requires comparing current stats to fingerprint
- Optimize by caching statistics snapshot
- Only check staleness every N seconds (configurable)

**Memory:**

- Each cached plan stores a `PlanFingerprint` (few KB)
- Acceptable overhead for plan cache

## Drawbacks

**Approximate Optimization:**

- Adjustments are heuristic-based, may not find optimal plan
- Trade-off: speed (milliseconds) vs. optimality
- Risk: Adjusted plan is worse than original plan

**Complexity:**

- Adds fingerprinting logic to optimizer
- Plan cache becomes more complex
- Testing requires simulating statistics changes

**Not Always Applicable:**

- Some plan structures are hard to adjust (complex subqueries, CTEs)
- May need full re-optimization anyway
- User confusion: "Why did my plan change?"

**Maintenance:**

- Need to track which statistics influence which plan decisions
- Must update fingerprinting logic as optimizer evolves

## Rationale and alternatives

### Why This Design?

**Incremental > Full:**

- Full re-optimization is expensive (seconds for complex queries)
- Incremental adjustment is fast (milliseconds)
- Most statistics changes don't require full re-optimization

**Fingerprinting:**

- Makes staleness detection precise
- Avoids unnecessary adjustments
- Provides auditability (why did plan change?)

**Threshold-Based:**

- Avoids constant plan churn from minor statistical noise
- 2x cardinality change is a reasonable default
- Configurable for different workloads

### Alternative Approaches

**1. Always Re-Optimize:**

- On every statistics update, invalidate all plans
- **Rejected**: Too expensive, causes latency spikes

**2. Time-Based Expiration:**

- Evict plans after N seconds
- **Rejected**: Arbitrary, doesn't correlate with actual staleness

**3. Feedback-Driven:**

- Monitor query performance, evict if slower than expected
- **Rejected**: Requires runtime monitoring, reactive not proactive

**4. Plan Baselines (Oracle-style):**

- Store multiple plans, pick best based on current stats
- **Rejected**: High memory cost, complex to implement

**5. Per-Query Hints:**

- User specifies when to adjust plans
- **Rejected**: Manual, error-prone, defeats purpose of optimizer

### Impact of Not Doing This

**Without streaming adjustments:**

- Pre-compiled plans degrade over time
- Users must manually invalidate caches
- Performance cliffs when statistics change
- Full re-optimization is the only option (slow)

**Workaround:**

- Periodic cache invalidation (conservative, wastes work)
- Runtime query hints to force re-compilation (manual)
- Accept degraded performance until next ANALYZE

## Prior art

### Academic Research

**Adaptive Query Processing:**

- [Eddies: Continuously Adaptive Query Processing](https://dl.acm.org/doi/10.1145/335191.335420) - Dynamic plan reordering at runtime
- [Progressive Optimization in a Shared-Nothing Parallel Database](https://dl.acm.org/doi/10.1145/170036.170077) - Plan refinement during execution

**Plan Stability:**

- [Parametric Query Optimization](http://www.vldb.org/conf/1992/P451.PDF) - Plans that adapt to parameter values
- [Robust Query Processing through Progressive Optimization](https://dl.acm.org/doi/10.1145/1007568.1007642) - Plan re-optimization strategies

### Industry Solutions

**PostgreSQL:**

- **Plan cache invalidation**: Automatic on schema changes, manual on statistics updates
- **`pg_plan_cache_stats`**: View showing cache hit rates
- **No streaming adjustment**: Full re-optimization only
- **`auto_explain`**: Logs slow plans, manual review required

**Oracle:**

- **SQL Plan Management (SPM)**: Stores plan baselines, picks best plan
- **Adaptive Plans**: Switch between hash join and nested loop at runtime
- **Statistics History**: Tracks statistics over time, can revert
- **Complex**: Requires DBA tuning, heavy memory footprint

**SQL Server:**

- **Query Store**: Tracks plan performance, detects regressions
- **Automatic Plan Correction**: Reverts to old plan if new one is slower
- **Adaptive Joins**: Switch join method at runtime based on row counts
- **Reactive**: Adjusts after detecting slow execution

**MySQL:**

- **Query cache**: Deprecated in MySQL 8.0 due to scalability issues
- **Prepared statements**: Cached plans, no automatic adjustment
- **Manual invalidation**: `RESET QUERY CACHE`, `FLUSH TABLES`

**DuckDB:**

- **Statistics-aware optimizer**: Re-optimizes if statistics change significantly
- **Lightweight**: Small plan cache, fast re-optimization
- **No streaming adjustment**: Full re-optimization is fast enough (single-threaded)

**What We Can Learn:**

- Oracle's adaptive joins show runtime adjustment is valuable
- PostgreSQL's manual invalidation is painful for users
- SQL Server's Query Store demonstrates value of tracking plan performance
- DuckDB's fast re-optimization suggests streaming adjustment may not be needed for small databases
- **Key insight**: Need lightweight, automatic adjustment with fallback to full re-optimization

## Unresolved questions

**Design Questions:**

1. What is the right staleness threshold? (2x cardinality change? 5x? 10x?)
2. Should adjustment be automatic or require user opt-in?
3. How to communicate plan changes to users? (EXPLAIN output, logs?)

**Implementation Questions:**

1. Which plan modifications are safe to do incrementally? (Join method change? Yes. Projection pushdown? Unclear.)
2. How to test streaming adjustments? (Need infrastructure to simulate statistics changes)
3. Should adjustments be logged for debugging?

**Integration Questions:**

1. How to integrate with progressive re-optimization (RFC 0052)? (Try adjustment first, fall back to progressive?)
2. Should materialized view matching (RFC 0051) trigger plan adjustment?
3. How to expose adjustment settings in PostgreSQL extension?

**Out of Scope:**

- **Runtime adaptive query processing**: This RFC is for pre-compiled plans, not runtime adaptation
- **Machine learning-based adjustment**: Future work, requires training data
- **Cross-query optimization**: Adjusting multiple plans together

## Future possibilities

### Natural Extensions

**1. Cost-Based Adjustment:**

- Currently: Threshold-based heuristics (2x cardinality)
- Future: Cost-based decisions (only adjust if cost improves >10%)

**2. Multi-Plan Caching:**

- Store multiple plans for different cardinality ranges
- Pick best plan based on current statistics
- Inspired by Oracle's SQL Plan Management

**3. Learning-Based Adjustment:**

- Track which adjustments improve performance
- Learn workload-specific adjustment policies
- Avoid adjustments that historically didn't help

**4. Runtime Feedback:**

- Monitor actual cardinalities during execution
- Trigger mid-execution re-optimization if estimates are way off
- Combine with RFC 0052 (Progressive Re-Optimization)

**5. Global Plan Adjustment:**

- Adjust multiple related plans together
- Example: If `customers` cardinality changes, adjust all queries using `customers`
- Batch adjustments for efficiency

### Long-term Vision

Ra becomes a **fully adaptive optimizer** with multiple layers:

1. **Streaming adjustment** (this RFC): Fast, heuristic-based, milliseconds
2. **Progressive re-optimization** (RFC 0052): Background, full optimization, seconds
3. **Runtime adaptation**: Mid-execution plan changes based on actual data
4. **Learned policies**: ML-based adjustment strategies trained on workload

Integration with other RFCs:

- **RFC 0051 (Materialized Views)**: Adjust plans when new materialized views are created
- **RFC 0053 (Stored Procedures)**: Adjust embedded queries in procedures
- **RFC 0055-0057 (Type-Specific Optimizations)**: Adjust when TOAST thresholds change, JSONB indexes added

This RFC provides the foundation for keeping pre-compiled plans effective as databases evolve, without expensive full re-optimization.
