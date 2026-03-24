# RFC 0054: Streaming Plan Adjustments for Pre-compiled Plans

- Start Date: 2026-03-22
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: N/A

## Summary

Enable dynamic adjustment of pre-compiled execution plans as statistics and database facts change, without requiring expensive full re-optimization. This provides a lightweight, incremental replanning mechanism that keeps cached plans effective as data evolves. The mechanism is particularly valuable for stored procedures (RFC 0053), plan cache entries (RFC 0060), and any scenario where a `RelExpr` plan is reused across changing data distributions.

## Motivation

Pre-compiled plans (stored procedures, prepared statements, plan caches) become **stale** over time as:

1. **Table cardinalities change**: Data grows or shrinks significantly
2. **Data distributions shift**: `ColumnStats.distinct_count` or histogram shapes change
3. **Indexes are added or dropped**: `IndexStats` entries appear or disappear
4. **Statistics are refreshed**: `ANALYZE` reveals new patterns not reflected in cached `Statistics`

### The cost of staleness

Consider a plan compiled when `orders` had `row_count = 10_000` and `Statistics.indexes` contained `idx_orders_status`. The optimizer chose a nested loop join with an index scan. Over time:

- `orders` grows to `row_count = 1_000_000` -- nested loop becomes a sequential scan over the full table.
- `idx_orders_status` is dropped -- the index scan silently degrades to a sequential scan in the backend.
- `ColumnStats.distinct_count` for `orders.status` changes from 5 to 500 -- equality selectivity shifts from 0.2 to 0.002.

**Full re-optimization is expensive**: Complex queries with multiple joins take seconds to re-optimize through equality saturation. For OLTP workloads, this latency is unacceptable.

**Automatic re-compilation is disruptive**: Unpredictable latency spikes during execution degrade tail latency.

**Manual invalidation is error-prone**: DBAs must remember to flush specific plan cache entries when statistics change.

### What this RFC proposes

- **Plan fingerprinting**: Record which `Statistics` values (cardinalities, `distinct_count`, index availability) influenced each plan decision, extending the existing `QueryFingerprint` from RFC 0060.
- **Threshold monitoring**: Detect when current statistics diverge from fingerprinted values by more than a configurable factor (default: 2x for cardinalities).
- **Incremental replanning**: Modify only affected subtrees of the `RelExpr` plan (join method, scan type, filter ordering) in milliseconds instead of re-running the full optimizer.
- **Fallback to full re-optimization**: When incremental adjustment fails or has been applied too many times, evict the entry and trigger full optimization via RFC 0052's progressive re-optimization.

## Guide-level explanation

### What plan staleness means

A cached plan becomes stale when the statistics that influenced its structure have changed enough that the plan is likely suboptimal. This is not a binary condition -- a 5% cardinality change probably does not matter, but a 10x change almost certainly does.

### When plans need adjustment

Ra checks staleness lazily -- only when a cached plan is accessed. The check compares fingerprinted statistics against current values:

```rust
// At query execution time, before using a cached plan:
let entry = plan_cache.lookup(&fingerprint)?;

match entry.staleness_check(&current_stats) {
    Staleness::Fresh => {
        // Use cached plan directly
        execute(entry.plan)
    }
    Staleness::Stale(divergences) => {
        // Try incremental adjustment (fast path)
        match entry.plan.adjust(&divergences, &current_stats) {
            Ok(adjusted) => {
                plan_cache.update(&fingerprint, adjusted);
                execute(adjusted)
            }
            Err(_) => {
                // Evict and re-optimize (slow path)
                plan_cache.evict(&fingerprint);
                let new_plan = optimizer.optimize(&query, &current_stats)?;
                plan_cache.insert(fingerprint, new_plan);
                execute(new_plan)
            }
        }
    }
}
```

### How streaming adjustments work

The adjustment algorithm walks the `RelExpr` tree and applies local transformations based on which statistics diverged:

| Divergence | Adjustment |
|---|---|
| Table cardinality increased >10x | Switch join from nested loop to hash join |
| Table cardinality decreased >10x | Switch join from hash join to nested loop |
| New index added | Replace sequential scan with index scan |
| Index dropped | Replace index scan with sequential scan |
| `distinct_count` changed >5x | Reorder filter predicates by new selectivity |
| Join selectivity changed >3x | Reorder join inputs (swap build/probe sides) |

### Example: before and after adjustment

**Original plan** (compiled when `orders.row_count = 10_000`):

```
Project [c.name, o.total]
  NestedLoopJoin (c.id = o.customer_id)
    IndexScan(customers, idx_customers_pk)
    Filter(o.status = 'active')
      SeqScan(orders)
```

**After adjustment** (detected `orders.row_count = 1_000_000`):

```
Project [c.name, o.total]
  HashJoin (c.id = o.customer_id)     -- switched from nested loop
    SeqScan(customers)                 -- build side
    Filter(o.status = 'active')
      SeqScan(orders)                  -- probe side
```

Only the join method changed. The filter, projection, and scan operators were left intact. No equality saturation pass was needed.

## Reference-level explanation

### Plan statistics fingerprint

A `PlanStatFingerprint` captures the statistics that influenced plan decisions. It extends (but does not replace) the structural `QueryFingerprint` from RFC 0060, which is used for cache key lookup.

```rust
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Records the statistics values that influenced a plan's structure.
/// Stored alongside each `CacheEntry` in the plan cache.
#[derive(Debug, Clone)]
pub struct PlanStatFingerprint {
    /// Table cardinalities (row_count) at plan creation time.
    /// Key: table name; value: Statistics.row_count.
    pub table_cardinalities: HashMap<String, f64>,

    /// Column distinct counts at plan creation time.
    /// Key: "table.column"; value: ColumnStats.distinct_count.
    pub column_ndistinct: HashMap<String, f64>,

    /// Set of index names that existed when the plan was created.
    /// Derived from Statistics.indexes keys.
    pub available_indexes: HashSet<String>,

    /// Estimated join selectivities used by the cost model.
    /// Key: (left_table, right_table); value: selectivity in [0.0, 1.0].
    pub join_selectivities: HashMap<(String, String), f64>,

    /// Predicate selectivities used for filter ordering.
    /// Key: predicate string repr; value: selectivity.
    pub predicate_selectivities: HashMap<String, f64>,

    /// When this fingerprint was created.
    pub created_at: Instant,
}
```

### Staleness detection

The fingerprint provides a `check` method that compares against current `Statistics`:

```rust
/// Divergence types detected during staleness check.
#[derive(Debug, Clone)]
pub enum StatDivergence {
    CardinalityChange {
        table: String,
        old: f64,
        new: f64,
        ratio: f64,  // max(new/old, old/new)
    },
    NDistinctChange {
        column: String,
        old: f64,
        new: f64,
        ratio: f64,
    },
    IndexAdded {
        index_name: String,
        table: String,
        columns: Vec<String>,
    },
    IndexDropped {
        index_name: String,
    },
    SelectivityChange {
        predicate: String,
        old: f64,
        new: f64,
    },
}

pub enum Staleness {
    Fresh,
    Stale(Vec<StatDivergence>),
}

impl PlanStatFingerprint {
    /// Compare fingerprinted values against current statistics.
    /// Returns `Stale` if any value diverges beyond thresholds.
    pub fn check(
        &self,
        current: &HashMap<String, Statistics>,
        thresholds: &StalenessThresholds,
    ) -> Staleness {
        let mut divergences = Vec::new();

        for (table, old_card) in &self.table_cardinalities {
            if let Some(stats) = current.get(table) {
                let ratio = if stats.row_count > *old_card {
                    stats.row_count / old_card
                } else {
                    old_card / stats.row_count
                };
                if ratio >= thresholds.cardinality_ratio {
                    divergences.push(StatDivergence::CardinalityChange {
                        table: table.clone(),
                        old: *old_card,
                        new: stats.row_count,
                        ratio,
                    });
                }

                // Check distinct counts for columns in this table
                for (col_key, old_ndv) in &self.column_ndistinct {
                    if !col_key.starts_with(table.as_str()) {
                        continue;
                    }
                    let col_name = col_key
                        .strip_prefix(&format!("{table}."))
                        .unwrap_or(col_key);
                    if let Some(col_stats) = stats.columns.get(col_name) {
                        let ndv_ratio = if col_stats.distinct_count > *old_ndv {
                            col_stats.distinct_count / old_ndv
                        } else {
                            old_ndv / col_stats.distinct_count
                        };
                        if ndv_ratio >= thresholds.ndistinct_ratio {
                            divergences.push(StatDivergence::NDistinctChange {
                                column: col_key.clone(),
                                old: *old_ndv,
                                new: col_stats.distinct_count,
                                ratio: ndv_ratio,
                            });
                        }
                    }
                }

                // Check for added indexes
                for (idx_name, idx_stats) in &stats.indexes {
                    if !self.available_indexes.contains(idx_name) {
                        divergences.push(StatDivergence::IndexAdded {
                            index_name: idx_name.clone(),
                            table: table.clone(),
                            columns: idx_stats.columns.clone(),
                        });
                    }
                }
            }
        }

        // Check for dropped indexes
        let current_indexes: HashSet<String> = current
            .values()
            .flat_map(|s| s.indexes.keys().cloned())
            .collect();
        for idx in &self.available_indexes {
            if !current_indexes.contains(idx) {
                divergences.push(StatDivergence::IndexDropped {
                    index_name: idx.clone(),
                });
            }
        }

        if divergences.is_empty() {
            Staleness::Fresh
        } else {
            Staleness::Stale(divergences)
        }
    }
}
```

### Staleness thresholds

```rust
/// Configurable thresholds that control how aggressively
/// plans are considered stale.
#[derive(Debug, Clone)]
pub struct StalenessThresholds {
    /// Cardinality must change by this factor to trigger
    /// staleness. Default: 2.0 (2x growth or 2x shrinkage).
    pub cardinality_ratio: f64,

    /// Distinct count must change by this factor.
    /// Default: 5.0.
    pub ndistinct_ratio: f64,

    /// Any index change (add/drop) triggers staleness.
    /// Default: true.
    pub index_changes_trigger: bool,

    /// Maximum age before a plan is considered stale
    /// regardless of statistics. None = no age limit.
    pub max_age: Option<std::time::Duration>,
}

impl Default for StalenessThresholds {
    fn default() -> Self {
        Self {
            cardinality_ratio: 2.0,
            ndistinct_ratio: 5.0,
            index_changes_trigger: true,
            max_age: None,
        }
    }
}
```

### Plan adjustment algorithm

The adjustment operates on a `RelExpr` tree, applying local transformations:

```rust
use ra_core::algebra::{RelExpr, JoinType};

/// Errors from the incremental adjustment process.
#[derive(Debug)]
pub enum AdjustmentError {
    /// The adjustment did not reduce estimated cost.
    NoImprovement,
    /// The plan structure cannot be adjusted incrementally
    /// (e.g., complex CTEs, recursive queries).
    UnsupportedStructure,
    /// Too many incremental adjustments have been applied;
    /// full re-optimization is needed.
    AdjustmentLimitReached,
}

/// Apply incremental adjustments to a RelExpr plan based on
/// detected divergences. Returns the adjusted plan or an error
/// if adjustment is not possible.
pub fn adjust_plan(
    plan: &RelExpr,
    divergences: &[StatDivergence],
    current_stats: &HashMap<String, Statistics>,
) -> Result<RelExpr, AdjustmentError> {
    let mut adjusted = plan.clone();

    for divergence in divergences {
        adjusted = match divergence {
            // Large cardinality increase: switch small-to-large
            // join methods to hash join
            StatDivergence::CardinalityChange {
                table, ratio, new, ..
            } if *ratio >= 10.0 && *new > 100_000.0 => {
                replace_join_for_table(
                    &adjusted,
                    table,
                    JoinMethod::HashJoin,
                )
            }

            // Large cardinality decrease: nested loop may be
            // cheaper for small tables
            StatDivergence::CardinalityChange {
                table, ratio, new, ..
            } if *ratio >= 10.0 && *new < 1_000.0 => {
                replace_join_for_table(
                    &adjusted,
                    table,
                    JoinMethod::NestedLoop,
                )
            }

            // New index: try replacing seq scans on the indexed
            // columns with index scans
            StatDivergence::IndexAdded {
                table, columns, index_name,
            } => {
                try_add_index_scan(
                    &adjusted,
                    table,
                    index_name,
                    columns,
                    current_stats,
                )
            }

            // Dropped index: replace index scans that reference
            // the dropped index with seq scans
            StatDivergence::IndexDropped { index_name } => {
                replace_index_with_seq_scan(&adjusted, index_name)
            }

            // NDV change: reorder filter predicates by updated
            // selectivity
            StatDivergence::NDistinctChange { column, .. } => {
                reorder_filters_for_column(
                    &adjusted,
                    column,
                    current_stats,
                )
            }

            _ => adjusted,
        };
    }

    // Recalculate cost estimates with new statistics.
    // If the adjusted plan is not at least 10% cheaper than
    // the original, the adjustment is not worth the churn.
    let original_cost = estimate_cost(plan, current_stats);
    let adjusted_cost = estimate_cost(&adjusted, current_stats);

    if adjusted_cost < original_cost * 0.9 {
        Ok(adjusted)
    } else if adjusted == *plan {
        // No changes were made (divergences didn't affect plan)
        Ok(adjusted)
    } else {
        Err(AdjustmentError::NoImprovement)
    }
}
```

### Subtree replacement helpers

These functions traverse the `RelExpr` tree and apply local modifications:

```rust
/// Walk the RelExpr tree. When we find a Join whose inputs
/// reference `table`, replace the join method.
fn replace_join_for_table(
    plan: &RelExpr,
    table: &str,
    new_method: JoinMethod,
) -> RelExpr {
    match plan {
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let left_refs_table = references_table(left, table);
            let right_refs_table = references_table(right, table);

            if left_refs_table || right_refs_table {
                // Apply physical join method hint.
                // In Ra's current model, join_type is logical
                // (Inner, Left, etc.). The physical method
                // (hash, merge, nested loop) is chosen by the
                // cost model. We annotate the plan with a hint.
                let mut new_join = RelExpr::Join {
                    join_type: *join_type,
                    condition: condition.clone(),
                    left: Box::new(
                        replace_join_for_table(left, table, new_method),
                    ),
                    right: Box::new(
                        replace_join_for_table(right, table, new_method),
                    ),
                };
                annotate_physical_method(&mut new_join, new_method);
                new_join
            } else {
                RelExpr::Join {
                    join_type: *join_type,
                    condition: condition.clone(),
                    left: Box::new(
                        replace_join_for_table(left, table, new_method),
                    ),
                    right: Box::new(
                        replace_join_for_table(right, table, new_method),
                    ),
                }
            }
        }
        // Recursively handle other RelExpr variants...
        RelExpr::Filter { predicate, input } => RelExpr::Filter {
            predicate: predicate.clone(),
            input: Box::new(
                replace_join_for_table(input, table, new_method),
            ),
        },
        RelExpr::Project { columns, input } => RelExpr::Project {
            columns: columns.clone(),
            input: Box::new(
                replace_join_for_table(input, table, new_method),
            ),
        },
        other => other.clone(),
    }
}

/// Check whether a RelExpr subtree references a given table.
fn references_table(plan: &RelExpr, table: &str) -> bool {
    match plan {
        RelExpr::Scan { table: t, .. } => t == table,
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. } => {
            references_table(input, table)
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            references_table(left, table)
                || references_table(right, table)
        }
        RelExpr::Aggregate { input, .. } => {
            references_table(input, table)
        }
        _ => false,
    }
}
```

### Plan cache integration

The existing `PlanCache` (crate `ra-engine`, module `plan_cache`) stores entries keyed by `QueryFingerprint`. Streaming adjustments extend each `CacheEntry`:

```rust
/// Extended cache entry with statistics fingerprint.
struct CacheEntry {
    /// The structural query fingerprint (RFC 0060).
    fingerprint: QueryFingerprint,
    /// The optimized plan.
    plan: RelExpr,
    /// Statistics that influenced this plan's structure.
    stat_fingerprint: PlanStatFingerprint,
    /// LRU tracking counter.
    last_access: u64,
    /// Hit count.
    hit_count: u64,
    /// Number of incremental adjustments applied to this entry.
    adjustment_count: u32,
}

impl PlanCache {
    /// Look up a plan, checking staleness and adjusting if needed.
    pub fn get_or_adjust(
        &mut self,
        key: &QueryFingerprint,
        current_stats: &HashMap<String, Statistics>,
        thresholds: &StalenessThresholds,
    ) -> Result<CacheLookupResult, PlanCacheError> {
        let entry = self.entries.get_mut(key)
            .ok_or(PlanCacheError::NotFound)?;

        entry.last_access = self.access_counter;
        self.access_counter += 1;

        match entry.stat_fingerprint.check(current_stats, thresholds) {
            Staleness::Fresh => Ok(CacheLookupResult {
                plan: entry.plan.clone(),
                match_type: CacheMatchType::Exact,
                similarity: 1.0,
            }),
            Staleness::Stale(divergences) => {
                // Limit incremental adjustments before forcing
                // full re-optimization.
                if entry.adjustment_count >= 5 {
                    self.entries.remove(key);
                    return Err(PlanCacheError::TooManyAdjustments);
                }

                match adjust_plan(
                    &entry.plan,
                    &divergences,
                    current_stats,
                ) {
                    Ok(adjusted) => {
                        entry.plan = adjusted.clone();
                        entry.stat_fingerprint =
                            build_fingerprint(&adjusted, current_stats);
                        entry.adjustment_count += 1;
                        Ok(CacheLookupResult {
                            plan: adjusted,
                            match_type: CacheMatchType::Exact,
                            similarity: 1.0,
                        })
                    }
                    Err(_) => {
                        // Adjustment failed; evict entry
                        self.entries.remove(key);
                        Err(PlanCacheError::StalePlan)
                    }
                }
            }
        }
    }
}
```

### Integration with RFC 0052 (Progressive Re-Optimization)

Streaming adjustments and progressive re-optimization form a two-tier adaptive system:

| Layer | Trigger | Cost | Scope |
|---|---|---|---|
| **Streaming adjustment** (this RFC) | Statistics diverge from fingerprint | Microseconds to milliseconds | Local subtree changes |
| **Progressive re-optimization** (RFC 0052) | Adjustment fails or limit reached | Seconds (background) | Full plan via equality saturation |

The interaction:

1. **Plan accessed**: Check `PlanStatFingerprint` against current stats.
2. **If stale**: Try `adjust_plan()` (fast path, this RFC).
3. **If adjustment fails** (`NoImprovement` or `AdjustmentLimitReached`): Evict from cache and request full re-optimization through RFC 0052's `StitchCoordinator`.
4. **If running**: RFC 0052's stitch points can also trigger streaming adjustment mid-execution when actual cardinalities diverge from estimates. The `DivergenceInfo` from a stitch point feeds directly into `StatDivergence::CardinalityChange`.

```rust
// In RFC 0052's stitch point handler:
fn on_stitch_divergence(info: &DivergenceInfo, plan: &RelExpr) {
    let divergence = StatDivergence::CardinalityChange {
        table: info.operator.clone(),
        old: info.estimated_cardinality as f64,
        new: info.actual_cardinality as f64,
        ratio: info.divergence_factor,
    };

    // Try streaming adjustment first (fast)
    match adjust_plan(plan, &[divergence], &runtime_stats) {
        Ok(adjusted) => switch_to_plan(adjusted),
        Err(_) => {
            // Fall back to full re-optimization (RFC 0052)
            reoptimize_at_stitch_point(query, plan, stitch_info);
        }
    }
}
```

### Integration with RFC 0051 (Materialized View Matching)

When new materialized views are created or existing ones are refreshed, this triggers plan staleness. The `IndexAdded` divergence type generalizes to cover MVs:

```rust
/// Extended divergence type for materialized view changes.
pub enum StatDivergence {
    // ... existing variants ...

    /// A new materialized view is available that covers
    /// tables/columns used by the cached plan.
    MaterializedViewAdded {
        mv_name: String,
        base_tables: Vec<String>,
    },

    /// A materialized view used by the plan was dropped
    /// or became too stale.
    MaterializedViewInvalidated {
        mv_name: String,
    },
}
```

When a `MaterializedViewAdded` divergence is detected, the adjustment algorithm can replace a join subtree with an MV scan:

```rust
// Adjustment for new MV: replace join subtree with MV scan
StatDivergence::MaterializedViewAdded {
    mv_name, base_tables,
} => {
    if plan_covers_tables(plan, &base_tables) {
        replace_subtree_with_mv_scan(plan, &mv_name, &base_tables)
    } else {
        plan.clone()
    }
}
```

### Integration with RFC 0053 (Stored Procedures)

Each SQL statement within a stored procedure has its own `PlanStatFingerprint`. When the procedure is invoked:

1. Each statement's fingerprint is checked against current statistics.
2. Stale statements are adjusted independently.
3. The procedure's compiled representation stores per-statement adjustment counts.
4. If any statement exceeds the adjustment limit, only that statement is re-optimized -- not the entire procedure.

### Error handling

```rust
#[derive(Debug)]
pub enum PlanCacheError {
    /// No entry found for this fingerprint.
    NotFound,
    /// Plan is stale and adjustment failed; caller should
    /// re-optimize from scratch.
    StalePlan,
    /// Too many incremental adjustments; full re-optimization
    /// is needed to avoid drift.
    TooManyAdjustments,
}

#[derive(Debug)]
pub enum AdjustmentError {
    /// Adjusted plan is not cheaper than original.
    NoImprovement,
    /// Plan structure does not support incremental adjustment
    /// (recursive CTEs, complex subqueries).
    UnsupportedStructure,
    /// Adjustment limit reached.
    AdjustmentLimitReached,
}
```

When adjustment fails:
1. The cache entry is evicted.
2. The next access triggers a full optimization pass.
3. A diagnostic event is emitted (table, divergence type, ratio) for observability.

### Performance considerations

**Adjustment speed target**: <1ms for single-divergence adjustments on plans with <50 operators. The algorithm does a single tree traversal per divergence, with O(n) complexity in the number of `RelExpr` nodes.

**Staleness check overhead**: Comparing a fingerprint against current statistics is O(t + c + i) where t = tables, c = columns, i = indexes in the fingerprint. For typical OLTP queries (2-5 tables, 10-20 columns, 5-10 indexes), this is <100 microseconds.

**Memory overhead**: Each `PlanStatFingerprint` adds roughly 200-500 bytes per cache entry (proportional to number of tables and columns). For a 1024-entry plan cache, this is ~500KB total.

**Adjustment count limit**: After 5 incremental adjustments, the plan is evicted and fully re-optimized. This prevents accumulated drift from small adjustments producing a globally suboptimal plan.

## Drawbacks

**Approximate optimization**: Adjustments are heuristic-based (threshold comparisons and local substitutions). They may not find the globally optimal plan that a full equality saturation pass would produce. This is an intentional tradeoff: millisecond adjustments versus second-scale re-optimization.

**Drift risk**: Multiple incremental adjustments can accumulate into a plan that no single optimization pass would produce. The adjustment count limit (default: 5) mitigates this, but a plan after 4 adjustments may still be suboptimal compared to a fresh optimization.

**Complexity**: Adds a new fingerprinting subsystem alongside the existing `QueryFingerprint`. The plan cache API grows more complex with the `get_or_adjust` method. Testing requires infrastructure to simulate statistics changes between cache accesses.

**Limited scope**: Some plan structures cannot be adjusted incrementally:
- Recursive CTEs (changing the recursion strategy requires full re-optimization)
- Complex subqueries with correlated predicates
- Window function partitioning (changing partition strategy affects correctness)

For these structures, the adjustment returns `UnsupportedStructure` and falls through to full re-optimization.

**Maintenance burden**: As new `RelExpr` variants are added, the adjustment traversal must be updated to handle them. Forgetting a variant means that subtree is silently skipped.

## Rationale and alternatives

### Why incremental adjustment over full re-optimization?

Full re-optimization through equality saturation is Ra's strength, but it has a cost: complex queries with 5+ joins can take seconds to optimize. For cached plans accessed thousands of times per second, even occasional re-optimization creates latency spikes. Incremental adjustment fills the gap: it handles the 80% case (join method switch, scan type change) in microseconds, reserving the full optimizer for structural changes.

### Why threshold-based triggering?

Continuous monitoring would catch divergence earlier but adds per-query overhead even when plans are fresh. Threshold-based checking is O(1) per cache access and avoids plan churn from minor statistical noise. The 2x cardinality threshold was chosen because:
- Below 2x, the cost model's estimates are usually still within the same order of magnitude.
- Above 2x, join method and scan type choices are likely affected.
- Configurable per workload via `StalenessThresholds`.

### Why track statistics in the fingerprint (not just timestamps)?

Timestamp-based expiration (evict plans older than N minutes) is simple but coarse: it evicts fresh plans on stable tables and keeps stale plans on volatile tables. By tracking the actual statistics values, staleness detection is precise -- a plan is only considered stale when the specific values that influenced it have changed.

### Alternative: always re-optimize

Invalidate all cached plans whenever any statistics update occurs. **Rejected**: Too expensive. A single `ANALYZE` on a large table would flush the entire plan cache, causing a thundering herd of re-optimization requests.

### Alternative: time-based expiration

Evict plans after a fixed TTL (e.g., 5 minutes). **Rejected**: Does not correlate with actual staleness. A plan for a static lookup table never needs eviction; a plan for a rapidly-growing staging table needs adjustment within seconds.

### Alternative: runtime feedback only

Monitor actual vs estimated cardinalities during execution (as RFC 0052 does), and adjust plans only when execution proves them wrong. **Rejected**: Reactive, not proactive. By the time a plan executes slowly, the user has already experienced the latency. Streaming adjustments catch staleness before execution begins.

### Alternative: plan baselines (Oracle SPM style)

Store multiple plan variants and select the best one based on current statistics. **Rejected**: High memory cost (N plans per query), complex selection logic, and the combinatorial explosion for queries with many parameters. Streaming adjustment achieves a similar result with a single plan and local modifications.

## Prior art

### PostgreSQL

PostgreSQL's `plan_cache_mode` GUC controls plan caching behavior for prepared statements:
- `auto` (default): Uses a generic plan after 5 executions if its cost is not much worse than a custom plan. Compares generic plan cost to average custom plan cost.
- `force_custom_plan`: Always re-plans (equivalent to disabling the cache).
- `force_generic_plan`: Always uses the cached plan (no staleness handling).

PostgreSQL has no incremental adjustment mechanism. When a generic plan becomes stale, the only option is `DEALLOCATE` followed by a new `PREPARE`, which triggers full re-planning. Schema changes (DDL) automatically invalidate cached plans, but statistics updates (`ANALYZE`) do not.

**Lesson**: PostgreSQL's coarse-grained approach (all-or-nothing plan caching) leaves a gap that streaming adjustments fill.

### SQL Server

SQL Server's approach is multi-layered:
- **Parameter sniffing**: Plans are optimized for the first set of parameter values, then reused. When parameters change significantly, plans become suboptimal ("parameter sniffing problem").
- **Automatic plan correction** (SQL Server 2017+): Detects plan regressions via Query Store, automatically reverts to the previous plan.
- **Adaptive joins** (SQL Server 2017+): Defers the hash-join vs nested-loop decision to runtime based on actual build-side row count. This is the closest industry analog to streaming adjustments, but it operates at the single-operator level rather than adjusting the full plan.
- **Forced recompilation**: `OPTION(RECOMPILE)` hint forces full re-optimization on every execution.

**Lesson**: SQL Server's adaptive joins validate the approach of deferring physical operator decisions based on actual statistics. Streaming adjustments generalize this to the full plan level.

### Oracle

Oracle's adaptive query processing (12c+) includes:
- **Adaptive plans**: The optimizer creates a plan with decision points. At runtime, the engine chooses between alternatives (e.g., nested loop vs hash join) based on actual row counts. Similar to SQL Server's adaptive joins.
- **SQL Plan Management (SPM)**: Stores plan baselines (known-good plans) and evolves them by testing new plans against baselines in a controlled manner.
- **Statistics feedback**: After the first execution, actual cardinalities are fed back to the optimizer for the next compilation. Not incremental -- it triggers a full re-optimization.
- **Automatic reoptimization**: Plans marked for reoptimization are fully re-optimized on the next execution.

**Lesson**: Oracle's adaptive plans show that runtime decision deferral works in production. SPM's plan evolution model is more heavyweight than streaming adjustments but provides stronger correctness guarantees.

### MySQL

MySQL 8.0+ handles plan caching for prepared statements:
- Cached plans are invalidated on schema changes and `ANALYZE TABLE`.
- No adaptive execution or incremental adjustment.
- The query cache (which cached result sets, not plans) was removed in MySQL 8.0 due to scalability issues with invalidation.

**Lesson**: MySQL's experience with the query cache shows that invalidation is the hard problem. Streaming adjustments address this by adjusting rather than invalidating.

### DuckDB

DuckDB re-optimizes on every execution because its optimizer is fast enough (single-digit milliseconds for most queries). Plan caching is not a priority because:
- Analytical queries are typically executed once.
- The optimizer is lightweight (no equality saturation).

**Lesson**: For analytical workloads, fast re-optimization may be preferable to incremental adjustment. Streaming adjustments are most valuable for OLTP-style workloads where the same prepared statements execute millions of times.

### Academic research

- **Eddies** (Avnur & Hellerstein, SIGMOD 2000): Continuously adaptive query processing that routes tuples through operators dynamically. Demonstrates the value of runtime adaptation but has high per-tuple overhead.
- **Parametric Query Optimization** (Ioannidis et al., VLDB 1992): Pre-computes optimal plans for different parameter ranges. Similar in spirit to plan baselines but with explicit parameter space partitioning.
- **Progressive Optimization** (Markl et al., SIGMOD 2004): Re-optimizes mid-execution when cardinality estimates prove wrong. Direct inspiration for RFC 0052; streaming adjustments complement this by handling pre-execution staleness.
- **Plan Bouquets** (Dutt & Haritsa, SIGMOD 2014): Creates a set of plans that collectively cover the parameter space with near-optimal performance. Reduces to a small number of plans through geometric analysis.

## Unresolved questions

### Design questions

1. **Default threshold**: Is 2x cardinality change the right default? Production workloads may need different defaults for OLTP (more sensitive, 1.5x) vs OLAP (less sensitive, 5x).

2. **Adjustment count limit**: Is 5 the right limit before forcing full re-optimization? Too low wastes optimization budget; too high allows drift.

3. **Opt-in vs opt-out**: Should streaming adjustments be on by default, or require explicit opt-in? On by default is more useful, but may surprise users who expect plan stability.

### Implementation questions

1. **Which `RelExpr` variants support adjustment?** Joins and scans are straightforward. Aggregates (switching hash-aggregate to sort-aggregate) and sorts (changing sort order) are more complex. Window functions and recursive CTEs probably cannot be adjusted incrementally.

2. **How to test?** Need infrastructure to:
   - Create a plan with specific statistics
   - Modify statistics between accesses
   - Verify the adjustment produces the expected plan change
   - Verify the adjusted plan produces correct results

3. **How to observe?** Should adjustment events be exposed through:
   - `EXPLAIN` output (show fingerprint and divergences)
   - Plan cache statistics (`PlanCacheStats` from `plan_cache.rs`)
   - Structured logging / tracing events

### Integration questions

1. **RFC 0052 handoff**: When adjustment fails, how quickly should progressive re-optimization begin? Immediately (synchronous, blocking) or in the background (async, use stale plan for this execution)?

2. **RFC 0051 MV events**: Should MV creation/refresh be modeled as a `StatDivergence` variant, or as a separate invalidation signal?

3. **RFC 0060 fingerprint relationship**: `QueryFingerprint` identifies structurally equivalent queries. `PlanStatFingerprint` tracks statistics that influenced plan choices. Should these be unified or kept separate?

### Out of scope

- **Runtime adaptive execution**: Mid-execution plan changes based on actual row counts (covered by RFC 0052).
- **Machine learning**: Learning which adjustments historically improved performance.
- **Cross-plan optimization**: Adjusting multiple cached plans together when a shared table's statistics change (future RFC).

## Future possibilities

### Cost-based adjustment decisions

Currently, adjustments are threshold-based heuristics (e.g., "if cardinality grew >10x, switch to hash join"). A future enhancement could run a lightweight cost comparison before applying each adjustment, using the actual cost model rather than fixed thresholds. This bridges the gap between heuristic adjustment and full re-optimization.

### Multi-plan variants

Store 2-3 plan variants for different cardinality ranges (small/medium/large). When statistics change, select the appropriate variant instead of adjusting. This is similar to Oracle's SPM plan baselines but with explicit cardinality bucketing.

### Workload-aware thresholds

Learn from historical query executions which threshold values produce the best outcomes for specific workload patterns. A table with high insert rates might benefit from a lower cardinality threshold (1.5x), while a slowly-growing dimension table might use 10x.

### Batch adjustment

When a single `ANALYZE` updates statistics for many tables, adjust all affected cached plans in one batch pass rather than checking each plan individually on its next access. This amortizes the traversal cost.

### Integration with genetic tuning (RFC 0060)

The `GeneticTuner` from RFC 0060 could evolve `StalenessThresholds` alongside `OptimizerConfig` parameters, automatically finding the thresholds that produce the best balance between plan stability and freshness for the current workload.

### Long-term vision

Ra becomes a fully adaptive optimizer with layered adjustment:

1. **Streaming adjustment** (this RFC): Pre-execution, heuristic-based, microseconds
2. **Progressive re-optimization** (RFC 0052): Mid-execution, cost-based, seconds
3. **Genetic parameter tuning** (RFC 0060): Background, evolutionary, minutes
4. **MV matching** (RFC 0051): Pre-execution, structural, milliseconds

Each layer catches different kinds of staleness at different timescales. Streaming adjustments handle the fast, common case; progressive re-optimization handles the expensive, rare case; genetic tuning handles the slow, global case.
