# RFC 0059: Statistics-Based Plan Cache Invalidation

- Start Date: 2026-03-24
- Author: Ra Development Team
- Status: Draft
- Tracking Issue: TBD
- Related: RFC 0054 (Streaming Plan Adjustments), RFC 0060 (Plan Cache / Genetic Fingerprinting)

## Summary

Extend Ra's differential dataflow infrastructure to track statistics, index, and fact changes alongside rule changes, and use the resulting change propagation to invalidate affected cached plans. Today, the plan cache uses only LRU eviction -- it has no awareness of whether the underlying data has shifted enough to make a cached plan suboptimal. This RFC adds event-driven invalidation: when statistics change beyond configurable thresholds, the differential dataflow engine computes the set of affected plan fingerprints via a join between change events and tracked plan dependencies, and invalidates exactly those plans. Cache lookups remain O(1) with zero staleness-checking overhead; invalidation cost is O(affected plans) and occurs only when statistics actually change.

## Motivation

Cached query plans are a performance win until they go stale. The plan cache (RFC 0060) and its thread-safe counterpart in `ra-cache` avoid redundant equality saturation passes by reusing plans for structurally identical queries. But the plans themselves encode assumptions about the data:

- **Table cardinalities** determine join method selection (hash join vs nested loop).
- **Column distinct counts** determine predicate selectivity and filter ordering.
- **Available indexes** determine scan strategy (index scan vs sequential scan).
- **Histogram shapes** determine range selectivity estimates.

When these assumptions no longer hold, the cached plan can be arbitrarily worse than a freshly optimized one.

### Concrete scenario

A query plan for `SELECT * FROM orders JOIN customers ON orders.cust_id = customers.id WHERE orders.status = 'pending'` is cached when `orders` has `row_count = 10_000` and an index `idx_orders_status` exists. The optimizer chose a nested loop join with an index scan on `orders.status`.

Over time:

1. `orders` grows to `row_count = 1_000_000`. The nested loop join, which was cost-effective for 10K rows, now scans the entire large table per iteration.
2. `idx_orders_status` is dropped by a DBA. The plan references an index that no longer exists; the backend falls back to a sequential scan silently.
3. `ColumnStats.distinct_count` for `orders.status` shifts from 5 to 500 because new order statuses were added. Equality selectivity changes from 0.2 to 0.002, making a hash join on the smaller result set much cheaper.

### What exists today

- **`ra-engine::plan_cache::PlanCache`** (`crates/ra-engine/src/plan_cache.rs`): LRU eviction keyed by `QueryFingerprint`. No statistics awareness. Entries are evicted only when the cache is full.
- **`ra-cache::PlanCache`** (`crates/ra-cache/src/lib.rs`): Thread-safe cache with `CachedPlan` entries that store a `statistics_snapshot: HashMap<String, Statistics>`. The `validity` module (`crates/ra-cache/src/validity.rs`) implements `check_plan_drift()` using fractional row-count drift -- but only row counts, not column-level statistics, index changes, or histogram shifts.
- **`ra-adaptive::cache_adapter`** (`crates/ra-adaptive/src/cache_adapter.rs`): `StatisticsPoller` that periodically checks for drift and triggers reoptimization. Uses `ra-cache`'s drift detection, which is limited to row-count-only comparison. Polling is expensive: every cache access pays O(dependencies) comparison cost even when nothing has changed.
- **`ra-engine::differential`** (`crates/ra-engine/src/differential.rs`): `IncrementalOptimizer` already tracks rule changes via differential dataflow collections and computes affected queries via `compute_affected_queries()`. It maintains two collections -- rules and query-rule dependency edges -- and uses a join to find which queries are affected by rule changes. Statistics and index changes are not tracked.
- **RFC 0054**: Proposes streaming plan adjustments (incremental replanning) but does not define the invalidation signal that triggers them.

### What is missing

1. **Event-driven invalidation**: The current approach (polling on every cache access, or periodic background polling) pays per-access cost even when statistics have not changed. Ra already has a differential dataflow engine that propagates rule changes to affected queries -- it should propagate statistics changes the same way.
2. **Multi-dimensional staleness detection**: Row-count drift alone misses index changes, NDV shifts, and histogram drift.
3. **Dependency tracking per plan**: No record of which specific statistics resources (table cardinalities, column NDVs, indexes) influenced each plan's structure.
4. **Unified change infrastructure**: Rule changes, statistics changes, index changes, and fact changes should flow through the same differential dataflow pipeline rather than being handled by separate mechanisms.

### Why differential dataflow, not polling

The architecture document (`docs/architecture-differential-cache-invalidation.md`) makes the case quantitatively:

| Approach | Check frequency | Cost per access | Cost per change | Invalidation precision |
|---|---|---|---|---|
| None (current) | Never | O(1) | O(1) | N/A -- no invalidation |
| Polling on access | Every access | O(deps) | O(1) | High |
| Differential (this RFC) | On change | O(1) | O(affected) | Perfect |

For an OLTP workload with 1M queries/sec and 1 ANALYZE/hour, polling costs 10M dependency checks per second. Differential costs 100 invalidations per hour plus 1M O(1) lookups per second.

## Guide-level explanation

### What makes a plan stale

A cached plan becomes stale when the statistics that influenced its structure change enough that the plan is likely suboptimal. Staleness is not binary -- a 5% cardinality change rarely matters, but a 100x change almost certainly does.

Ra tracks staleness across four dimensions:

| Dimension | Default threshold | Rationale |
|---|---|---|
| Row count (cardinality) | 2x change (ratio) | Join method and scan type depend on cardinality magnitude |
| Column distinct count | 1.5x change (ratio) | Filter selectivity and predicate ordering are sensitive to NDV |
| Index availability | Any add/drop | Index scans become available or unavailable |
| Histogram distribution | KL-divergence > 0.5 | Range predicate selectivity depends on distribution shape |

### How invalidation works: the differential dataflow approach

Unlike a polling-based approach where every cache lookup checks whether statistics have changed, Ra uses **event-driven invalidation** through the same differential dataflow infrastructure already used for rule change propagation.

The flow has four stages:

```
1. TRACK: When a plan is cached, record its dependencies
   (which tables, columns, indexes influenced the plan)

2. DETECT: Statistics sources (ANALYZE, streaming pipeline,
   DDL hooks) detect when values cross thresholds

3. COMPUTE: Differential dataflow joins change events against
   plan dependencies to find affected fingerprints

4. INVALIDATE: Affected plans are evicted from the cache
```

The key property: **cache lookups do zero staleness work**. The `lookup()` method remains a pure hash table lookup at O(1). All invalidation happens asynchronously when change events arrive, not when plans are accessed.

```
            Differential Dataflow Collections
            ---------------------------------
Rule Changes        ->  Collection[RuleChange]          (existing)
Statistics Changes  ->  Collection[StatisticsChange]    (NEW)
Index Changes       ->  Collection[IndexChange]         (NEW)
Fact Changes        ->  Collection[FactChange]          (NEW)
                              |
                    Join with Plan Dependencies
                              |
                    Affected Plan Fingerprints
                              |
                    Plan Cache Invalidation
```

### When a plan is cached

When a plan is inserted into the cache, Ra extracts its **plan dependencies** -- the set of resources (table cardinalities, column NDVs, indexes, facts) that influenced the plan's structure. These dependencies are stored as edges in the differential dataflow dependency collection.

```rust
// At plan insertion time:
let plan = optimizer.optimize(&query, &stats)?;
let deps = PlanDependencies::from_plan_and_stats(&plan, &stats);

// Insert into cache (O(1) hash table insert)
plan_cache.insert(fingerprint, plan);

// Register dependencies in the differential dataflow graph
incremental.register_plan_dependencies(&fingerprint, &deps);
```

### When statistics change

Statistics sources (PostgreSQL `ANALYZE`, the streaming pipeline, DDL hooks) emit change events when values cross thresholds. These events flow into the differential dataflow engine, which computes affected plans via a join and invalidates them:

```rust
// When ANALYZE completes on the "orders" table:
let change = ChangeSource::Statistics(StatisticsChange::RowCount {
    table: "orders".into(),
    old_value: 1_000_000.0,
    new_value: 10_000_000.0,
    ratio: 10.0,
});

// Differential dataflow computes affected plans (O(affected))
let affected = incremental.compute_affected_plans(&[change]);

// Invalidate only those plans (O(affected))
plan_cache.invalidate(&affected);
```

### When a plan is looked up

Cache lookups are unchanged -- no staleness check, no fingerprint comparison, no generation counter. If a plan is in the cache, it is valid. If it was invalidated by a change event, it is gone and the lookup returns a miss.

```rust
match plan_cache.lookup(&fingerprint) {
    Some(result) => execute(result.plan),  // O(1), always fresh
    None => {
        // Cache miss: either never cached or invalidated by change event
        let plan = optimizer.optimize(&query, &stats)?;
        let deps = PlanDependencies::from_plan_and_stats(&plan, &stats);
        plan_cache.insert(fingerprint.clone(), plan.clone());
        incremental.register_plan_dependencies(&fingerprint, &deps);
        execute(plan)
    }
}
```

### Invalidation strategies

Two strategies are supported per cache entry:

- **Hard invalidation** (evict): Remove the cache entry. The next access triggers full re-optimization. Simple and correct. Default for entries with low hit counts.
- **Soft invalidation** (mark stale, attempt RFC 0054 adjustment): Flag the entry. On the next access, attempt an incremental streaming adjustment (RFC 0054). If adjustment succeeds, update the entry in place with a new `PlanDependencies`. If adjustment fails, evict. Preferred for hot entries (high hit count) where re-optimization latency matters.

The strategy is selected automatically based on `hit_count`: entries above a configurable threshold (default: 100 hits) use soft invalidation.

### Example: table growth scenario

**Initial state:**
```
Table: orders (row_count: 1,000,000, distinct status values: 5)

Cached Plan A (fingerprint_A):
  Query: SELECT * FROM orders WHERE status = ?
  Plan: Index scan on orders_status_idx
  Dependencies: {
      resources: [
          "orders.row_count",
          "orders.status.ndistinct",
          "orders.orders_status_idx",
      ]
  }

Cached Plan B (fingerprint_B):
  Query: SELECT * FROM users WHERE city = ?
  Plan: Index scan on users_city_idx
  Dependencies: {
      resources: ["users.row_count", "users.users_city_idx"]
  }
```

**After `ANALYZE orders` (now 10,000,000 rows):**
```
Step 1: Statistics provider detects threshold crossing
  orders.row_count: 1,000,000 -> 10,000,000 (10x, threshold: 2x)

Step 2: Emit change event into differential dataflow
  changes_coll = [("orders.row_count", +1)]

Step 3: Join with plan dependency collection
  deps_coll = [
      (fingerprint_A, "orders.row_count"),    // matches!
      (fingerprint_A, "orders.status.ndistinct"),
      (fingerprint_A, "orders.orders_status_idx"),
      (fingerprint_B, "users.row_count"),     // no match
      (fingerprint_B, "users.users_city_idx"),
  ]

  affected_plans = [fingerprint_A]
  // fingerprint_B NOT affected (depends on users, not orders)

Step 4: Invalidate
  plan_cache.invalidate([fingerprint_A])
  // Only 1 plan invalidated out of 1024 cached plans
```

**Next access to Plan A's query:**
```
plan_cache.lookup(fingerprint_A) -> None (invalidated)
  -> Full optimization with new statistics
  -> New plan: Sequential scan (10M rows, selectivity 0.2 = 2M rows)
     vs Index scan (2M random accesses) -> Sequential wins
  -> Cache new plan with new dependencies
```

Plan B remains cached and valid throughout -- it was never touched.

## Reference-level explanation

### Plan dependencies

When a plan is cached, its dependencies are extracted and recorded as `ResourceId` values that identify specific statistics resources. These serve as the join key between change events and cached plans.

```rust
/// Identifies a specific statistics resource that can change.
/// Used as the join key in the differential dataflow computation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceId {
    /// Table row count: "table_name.row_count"
    RowCount(String),
    /// Column distinct count: "table.column.ndistinct"
    NDistinct(String, String),
    /// Index existence: "table.index_name"
    Index(String, String),
    /// Column histogram: "table.column.histogram"
    Histogram(String, String),
    /// A database fact (e.g., constraint, FK relationship)
    Fact(String),
}

impl ResourceId {
    /// String key for differential dataflow collections.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::RowCount(t) => format!("{t}.row_count"),
            Self::NDistinct(t, c) => format!("{t}.{c}.ndistinct"),
            Self::Index(t, i) => format!("{t}.{i}"),
            Self::Histogram(t, c) => format!("{t}.{c}.histogram"),
            Self::Fact(f) => f.clone(),
        }
    }
}

/// Dependencies of a cached plan on statistics resources.
/// Stored per cache entry and registered as edges in the
/// differential dataflow dependency collection.
#[derive(Debug, Clone)]
pub struct PlanDependencies {
    /// Table cardinalities that influenced this plan.
    /// Key: table name; value: row_count at optimization time.
    pub table_cardinalities: HashMap<String, f64>,

    /// Indexes this plan uses or considered.
    /// Key: (table, index_name).
    pub indexes: HashSet<(String, String)>,

    /// Column distinct counts that affect selectivity.
    /// Key: (table, column); value: distinct_count at optimization time.
    pub distinct_counts: HashMap<(String, String), f64>,

    /// Column histograms that affect range selectivity.
    /// Key: (table, column); value: histogram digest at optimization time.
    pub histogram_digests: HashMap<(String, String), HistogramDigest>,

    /// Facts that enabled certain optimization rules.
    pub facts: HashSet<String>,
}

impl PlanDependencies {
    /// Build dependencies from the tables referenced by a plan.
    pub fn from_plan_and_stats(
        plan: &RelExpr,
        stats: &dyn StatisticsProvider,
    ) -> Self {
        let tables = collect_referenced_tables(plan);
        let mut deps = Self {
            table_cardinalities: HashMap::new(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };

        for table in &tables {
            if let Some(table_stats) = stats.get_statistics(table) {
                deps.table_cardinalities
                    .insert(table.clone(), table_stats.row_count);

                for (col, col_stats) in &table_stats.columns {
                    deps.distinct_counts.insert(
                        (table.clone(), col.clone()),
                        col_stats.distinct_count,
                    );
                    if let Some(hist) = &col_stats.histogram {
                        deps.histogram_digests.insert(
                            (table.clone(), col.clone()),
                            HistogramDigest::from(hist),
                        );
                    }
                }

                for idx_name in table_stats.indexes.keys() {
                    deps.indexes
                        .insert((table.clone(), idx_name.clone()));
                }
            }
        }

        deps
    }

    /// Enumerate all ResourceIds this plan depends on.
    pub fn all_resources(&self) -> Vec<ResourceId> {
        let mut resources = Vec::new();

        for table in self.table_cardinalities.keys() {
            resources.push(ResourceId::RowCount(table.clone()));
        }
        for (table, col) in self.distinct_counts.keys() {
            resources.push(
                ResourceId::NDistinct(table.clone(), col.clone()),
            );
        }
        for (table, idx) in &self.indexes {
            resources.push(
                ResourceId::Index(table.clone(), idx.clone()),
            );
        }
        for (table, col) in self.histogram_digests.keys() {
            resources.push(
                ResourceId::Histogram(table.clone(), col.clone()),
            );
        }
        for fact in &self.facts {
            resources.push(ResourceId::Fact(fact.clone()));
        }

        resources
    }
}
```

### Staleness thresholds

Thresholds determine when a statistics change is significant enough to emit a change event into the differential dataflow pipeline. Threshold checking happens at the **change source** (statistics provider, DDL hook), not at cache lookup time.

```rust
/// Configurable thresholds for change detection.
/// Applied at the statistics source, not at cache lookup.
#[derive(Debug, Clone)]
pub struct StalenessThresholds {
    /// Cardinality must change by this ratio to emit an event.
    /// Default: 2.0 (2x growth or shrinkage).
    /// Computed as max(new/old, old/new).
    pub cardinality_ratio: f64,

    /// Distinct count must change by this ratio.
    /// Default: 1.5. NDV affects selectivity estimation,
    /// which is sensitive to smaller changes than cardinality.
    pub ndistinct_ratio: f64,

    /// Whether any index add/drop emits a change event.
    /// Default: true. Index changes directly affect scan
    /// strategy selection.
    pub index_changes_trigger: bool,

    /// KL-divergence threshold for histogram comparison.
    /// Default: 0.5. Only applies to columns with histograms.
    pub histogram_kl_threshold: f64,

    /// Maximum plan age before forced invalidation,
    /// regardless of statistics. None = no age limit.
    /// Enforced by a periodic sweep, not per-access check.
    pub max_age: Option<std::time::Duration>,
}

impl Default for StalenessThresholds {
    fn default() -> Self {
        Self {
            cardinality_ratio: 2.0,
            ndistinct_ratio: 1.5,
            index_changes_trigger: true,
            histogram_kl_threshold: 0.5,
            max_age: None,
        }
    }
}
```

### Change sources

Change events are typed so the differential dataflow computation can distinguish them and the invalidation logic can pass divergence details to RFC 0054's streaming adjustment:

```rust
/// A detected change that may invalidate cached plans.
#[derive(Debug, Clone)]
pub enum ChangeSource {
    Statistics(StatisticsChange),
    Index(IndexChange),
    Fact(FactChange),
}

impl ChangeSource {
    /// The ResourceId affected by this change.
    pub fn resource_id(&self) -> ResourceId {
        match self {
            Self::Statistics(s) => s.resource_id(),
            Self::Index(i) => i.resource_id(),
            Self::Fact(f) => ResourceId::Fact(f.fact_name.clone()),
        }
    }
}

/// A statistics value that crossed its threshold.
#[derive(Debug, Clone)]
pub enum StatisticsChange {
    RowCount {
        table: String,
        old_value: f64,
        new_value: f64,
        ratio: f64,
    },
    DistinctCount {
        table: String,
        column: String,
        old_value: f64,
        new_value: f64,
        ratio: f64,
    },
    HistogramDrift {
        table: String,
        column: String,
        kl_divergence: f64,
    },
}

impl StatisticsChange {
    pub fn resource_id(&self) -> ResourceId {
        match self {
            Self::RowCount { table, .. } =>
                ResourceId::RowCount(table.clone()),
            Self::DistinctCount { table, column, .. } =>
                ResourceId::NDistinct(table.clone(), column.clone()),
            Self::HistogramDrift { table, column, .. } =>
                ResourceId::Histogram(table.clone(), column.clone()),
        }
    }
}

/// An index that was added or dropped.
#[derive(Debug, Clone)]
pub enum IndexChange {
    Added {
        table: String,
        index_name: String,
        columns: Vec<String>,
    },
    Dropped {
        table: String,
        index_name: String,
    },
}

impl IndexChange {
    pub fn resource_id(&self) -> ResourceId {
        match self {
            Self::Added { table, index_name, .. }
            | Self::Dropped { table, index_name } =>
                ResourceId::Index(table.clone(), index_name.clone()),
        }
    }
}

/// A database fact that changed.
#[derive(Debug, Clone)]
pub struct FactChange {
    pub fact_name: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}
```

### Histogram digest

Full histogram comparison is expensive. Instead, we store a compact digest and use KL-divergence for approximate comparison at the change source:

```rust
/// Compact summary of a histogram for drift comparison.
#[derive(Debug, Clone)]
pub struct HistogramDigest {
    /// Bucket boundary count.
    pub bucket_count: usize,
    /// Normalized frequency distribution (sums to 1.0).
    pub frequencies: Vec<f64>,
    /// Total row count across all buckets.
    pub total_rows: f64,
}

impl HistogramDigest {
    /// Compute symmetric KL-divergence between two digests.
    /// Returns 0.0 for identical distributions, higher for
    /// more divergent distributions.
    pub fn kl_divergence(&self, other: &HistogramDigest) -> f64 {
        if self.bucket_count != other.bucket_count {
            return f64::MAX;
        }

        let epsilon = 1e-10;
        let mut kl_pq = 0.0;
        let mut kl_qp = 0.0;

        for (p, q) in self.frequencies.iter()
            .zip(other.frequencies.iter())
        {
            let p_safe = p.max(epsilon);
            let q_safe = q.max(epsilon);
            kl_pq += p_safe * (p_safe / q_safe).ln();
            kl_qp += q_safe * (q_safe / p_safe).ln();
        }

        (kl_pq + kl_qp) / 2.0
    }
}
```

### Extending IncrementalOptimizer

The existing `IncrementalOptimizer` (`crates/ra-engine/src/differential.rs`) maintains two differential dataflow collections for rule changes. This RFC extends it with two additional collections for statistics-based invalidation:

```rust
/// Extended IncrementalOptimizer with plan dependency tracking.
pub struct IncrementalOptimizer {
    // --- Existing fields ---
    optimizer: Optimizer,
    memo: MemoTable,
    queries: HashMap<u64, RegisteredQuery>,
    active_rules: Vec<RuleId>,
    generation: u64,
    pending_changes: Vec<RuleChange>,
    stats: ComputationStats,
    next_query_id: u64,
    _timely_config: TimelyConfig,

    // --- New fields (this RFC) ---

    /// Plan dependencies: maps QueryFingerprint to the set
    /// of ResourceIds the plan depends on.
    plan_dependencies: HashMap<QueryFingerprint, PlanDependencies>,

    /// Staleness thresholds for change detection.
    thresholds: StalenessThresholds,
}
```

The core addition is `compute_affected_plans()`, which mirrors the existing `compute_affected_queries()` but operates on `ChangeSource` events instead of `RuleChange` events:

```rust
impl IncrementalOptimizer {
    /// Register a cached plan's dependencies for change tracking.
    pub fn register_plan_dependencies(
        &mut self,
        fingerprint: &QueryFingerprint,
        deps: &PlanDependencies,
    ) {
        self.plan_dependencies
            .insert(fingerprint.clone(), deps.clone());
    }

    /// Remove a plan's dependencies (called on cache eviction).
    pub fn unregister_plan_dependencies(
        &mut self,
        fingerprint: &QueryFingerprint,
    ) {
        self.plan_dependencies.remove(fingerprint);
    }

    /// Compute which cached plans are affected by a set of
    /// change events, using differential dataflow.
    ///
    /// This is the core invalidation computation. It builds
    /// two collections:
    /// 1. Changed resource IDs (from the change events)
    /// 2. (fingerprint, resource_id) dependency edges
    ///
    /// A join produces the set of affected fingerprints.
    pub fn compute_affected_plans(
        &self,
        changes: &[ChangeSource],
    ) -> Vec<QueryFingerprint> {
        if changes.is_empty() || self.plan_dependencies.is_empty() {
            return Vec::new();
        }

        use std::sync::{Arc, Mutex};
        use differential_dataflow::input::Input;
        use differential_dataflow::operators::Join;

        let change_keys: Vec<String> = changes
            .iter()
            .map(|c| c.resource_id().key())
            .collect();

        // Build (resource_key, fingerprint) edges
        let dep_edges: Vec<(String, QueryFingerprint)> =
            self.plan_dependencies
                .iter()
                .flat_map(|(fp, deps)| {
                    deps.all_resources()
                        .into_iter()
                        .map(move |r| (r.key(), fp.clone()))
                })
                .collect();

        let output_buf =
            Arc::new(Mutex::new(Vec::<QueryFingerprint>::new()));
        let buf_clone = Arc::clone(&output_buf);

        timely::execute_directly(move |worker| {
            worker.dataflow::<u64, _, _>(|scope| {
                let (mut changes_input, changes_coll) =
                    scope.new_collection::<String, isize>();

                let (mut deps_input, deps_coll) =
                    scope.new_collection::<(String, QueryFingerprint), isize>();

                // Join: changed resources x dependencies
                // -> affected fingerprints
                let affected = changes_coll
                    .map(|key| (key, ()))
                    .join(&deps_coll)
                    .map(|(_resource, ((), fp))| fp);

                let buf = Arc::clone(&buf_clone);
                affected.inspect(move |&(ref fp, _time, _diff)| {
                    if let Ok(mut v) = buf.lock() {
                        v.push(fp.clone());
                    }
                });

                for key in &change_keys {
                    changes_input.insert(key.clone());
                }
                for (resource_key, fp) in &dep_edges {
                    deps_input
                        .insert((resource_key.clone(), fp.clone()));
                }

                changes_input.advance_to(1);
                deps_input.advance_to(1);
                changes_input.flush();
                deps_input.flush();
            });

            worker.step();
            worker.step();
        });

        let mut unique = Arc::try_unwrap(output_buf)
            .expect("single owner after timely completes")
            .into_inner()
            .expect("mutex not poisoned");
        unique.sort();
        unique.dedup();
        unique
    }
}
```

### Extended PlanCache API

The `PlanCache` in `ra-engine::plan_cache` gains an `invalidate()` method but **does not** gain any staleness-checking on lookup. Lookups remain pure hash table operations.

```rust
/// Extended cache entry with dependency metadata.
struct CacheEntry {
    /// The structural query fingerprint (RFC 0060).
    fingerprint: QueryFingerprint,
    /// The optimized plan.
    plan: RelExpr,
    /// Monotonic counter for LRU tracking.
    last_access: u64,
    /// Number of cache hits for this entry.
    hit_count: u64,

    // --- New fields (this RFC) ---

    /// Statistics dependencies for this plan.
    /// Used to unregister from the differential dataflow graph
    /// on eviction.
    dependencies: Option<PlanDependencies>,
    /// Number of streaming adjustments applied (RFC 0054).
    adjustment_count: u32,
    /// Whether this entry has been soft-invalidated.
    /// If true, the next access will attempt RFC 0054 adjustment.
    stale: bool,
}

impl PlanCache {
    /// Insert a plan with its dependencies.
    /// The caller is responsible for also calling
    /// `incremental.register_plan_dependencies()`.
    pub fn insert_with_deps(
        &mut self,
        fingerprint: QueryFingerprint,
        plan: RelExpr,
        deps: PlanDependencies,
    ) {
        // ... same as insert() but stores dependencies
    }

    /// Invalidate specific plans by fingerprint.
    /// Called by the differential dataflow engine when
    /// change events affect these plans.
    ///
    /// Entries with high hit counts are soft-invalidated
    /// (marked stale for RFC 0054 adjustment on next access).
    /// Entries with low hit counts are hard-evicted.
    pub fn invalidate(&mut self, fingerprints: &[QueryFingerprint]) {
        let soft_threshold = self.config.soft_invalidation_hit_threshold;

        for fp in fingerprints {
            if let Some(&idx) = self.exact_index.get(fp) {
                if self.entries[idx].hit_count >= soft_threshold {
                    // Soft invalidation: mark stale
                    self.entries[idx].stale = true;
                    self.stats.soft_invalidations += 1;
                } else {
                    // Hard invalidation: evict
                    self.evict_entry(fp);
                    self.stats.hard_invalidations += 1;
                }
            }
        }
    }

    /// Invalidate all plans that depend on a specific table.
    /// Convenience method for DDL events that affect an
    /// entire table (DROP TABLE, major schema change).
    pub fn invalidate_for_table(&mut self, table: &str) {
        let affected: Vec<QueryFingerprint> = self.entries
            .iter()
            .filter(|e| {
                e.dependencies
                    .as_ref()
                    .map_or(false, |d| {
                        d.table_cardinalities.contains_key(table)
                    })
            })
            .map(|e| e.fingerprint.clone())
            .collect();

        self.invalidate(&affected);
    }

    /// Look up a plan. If the entry is soft-invalidated (stale),
    /// return it with a flag so the caller can attempt RFC 0054
    /// streaming adjustment.
    pub fn lookup(
        &mut self,
        fingerprint: &QueryFingerprint,
    ) -> Option<CacheLookupResult> {
        // ... existing exact + fuzzy lookup logic ...
        // Additionally: if entry.stale, set match_type to Stale
    }
}
```

### PlanCacheConfig extension

```rust
pub struct PlanCacheConfig {
    // ... existing fields ...

    /// Hit count threshold above which soft invalidation
    /// (mark stale) is preferred over hard eviction.
    /// Default: 100. Plans accessed fewer than this many
    /// times are evicted; plans above this threshold are
    /// marked stale for streaming adjustment.
    pub soft_invalidation_hit_threshold: u64,
}
```

### PlanCacheStats extension

```rust
pub struct PlanCacheStats {
    // ... existing fields ...

    /// Plans evicted by differential dataflow invalidation.
    pub hard_invalidations: u64,
    /// Plans marked stale (soft invalidation) for adjustment.
    pub soft_invalidations: u64,
    /// Successful RFC 0054 streaming adjustments on stale plans.
    pub streaming_adjustments: u64,
    /// Failed adjustments that fell through to re-optimization.
    pub adjustment_failures: u64,
}
```

### CacheMatchType extension

```rust
pub enum CacheMatchType {
    Exact,
    Fuzzy,
    /// Entry was found but is soft-invalidated.
    /// The caller should attempt streaming adjustment
    /// (RFC 0054) before using the plan.
    Stale,
}
```

### Integration with streaming statistics (ra-stats)

The `StreamingPipeline` in `ra-stats` gains a change callback that fires when metrics cross thresholds. This is the primary change source for continuous workloads:

```rust
// crates/ra-stats/src/streaming.rs
impl StreamingPipeline {
    /// Register a callback for significant statistics changes.
    /// The callback receives change events when any tracked
    /// metric crosses its threshold.
    pub fn on_significant_change<F>(&mut self, callback: F)
    where
        F: Fn(StatisticsChange) + Send + 'static,
    {
        self.change_callbacks.push(Box::new(callback));
    }

    /// Check all tracked metrics against their previous values
    /// and thresholds. Fires callbacks for any that crossed.
    fn check_thresholds(&mut self, thresholds: &StalenessThresholds) {
        for (table, old_stats, new_stats) in self.stats_deltas() {
            let card_ratio = ratio(
                old_stats.row_count,
                new_stats.row_count,
            );
            if card_ratio >= thresholds.cardinality_ratio {
                self.emit(StatisticsChange::RowCount {
                    table: table.clone(),
                    old_value: old_stats.row_count,
                    new_value: new_stats.row_count,
                    ratio: card_ratio,
                });
            }

            for (col, old_col) in &old_stats.columns {
                if let Some(new_col) = new_stats.columns.get(col) {
                    let ndv_ratio = ratio(
                        old_col.distinct_count,
                        new_col.distinct_count,
                    );
                    if ndv_ratio >= thresholds.ndistinct_ratio {
                        self.emit(StatisticsChange::DistinctCount {
                            table: table.clone(),
                            column: col.clone(),
                            old_value: old_col.distinct_count,
                            new_value: new_col.distinct_count,
                            ratio: ndv_ratio,
                        });
                    }

                    // Histogram drift
                    if let (Some(old_h), Some(new_h)) =
                        (&old_col.histogram, &new_col.histogram)
                    {
                        let old_digest = HistogramDigest::from(old_h);
                        let new_digest = HistogramDigest::from(new_h);
                        let kl = old_digest.kl_divergence(&new_digest);
                        if kl > thresholds.histogram_kl_threshold {
                            self.emit(StatisticsChange::HistogramDrift {
                                table: table.clone(),
                                column: col.clone(),
                                kl_divergence: kl,
                            });
                        }
                    }
                }
            }
        }
    }
}
```

### Integration with PostgreSQL extension

The `ra-pgrx` extension hooks into `ANALYZE` completion and DDL events, emitting change events that flow through the same differential dataflow pipeline:

```rust
// crates/ra-pg-extension/src/stats_bridge.rs
impl PostgresStatsBridge {
    /// Called by the pgrx post-ANALYZE hook.
    pub fn on_analyze_complete(&self, table: &str) {
        let old_stats = self.cached_stats.get(table);
        let new_stats = self.gather_table_stats(table);

        if let (Some(old), Some(new)) = (old_stats, new_stats) {
            let changes = detect_changes(
                table, old, new, &self.thresholds,
            );
            if !changes.is_empty() {
                let affected = self.incremental
                    .compute_affected_plans(&changes);
                self.plan_cache.invalidate(&affected);
            }
            self.cached_stats.insert(table.into(), new.clone());
        }
    }

    /// Called on CREATE INDEX / DROP INDEX.
    pub fn on_index_change(&self, change: IndexChange) {
        let affected = self.incremental.compute_affected_plans(
            &[ChangeSource::Index(change)],
        );
        self.plan_cache.invalidate(&affected);
    }
}
```

### Integration with differential dataflow for rules + statistics

The unified pipeline handles both rule changes and statistics changes through the same differential dataflow infrastructure. Rule changes continue to use `compute_affected_queries()` (reoptimizing registered queries). Statistics changes use `compute_affected_plans()` (invalidating cached plans). Both share the same timely runtime:

```rust
// Unified initialization:
let incremental = IncrementalOptimizer::with_config(
    optimizer_config,
    timely_config,
);

// Connect streaming statistics
stats_pipeline.on_significant_change(move |change| {
    let affected = incremental.compute_affected_plans(
        &[ChangeSource::Statistics(change)],
    );
    plan_cache.invalidate(&affected);
});

// Rule changes continue to work as before
incremental.add_rule(RuleId::new("new-optimization"));
incremental.apply_changes()?; // reoptimizes affected queries
```

### Lifecycle: from insertion to invalidation to re-optimization

The complete lifecycle of a cached plan under this design:

```
1. OPTIMIZE: optimizer.optimize(query, stats) -> plan
2. EXTRACT:  PlanDependencies::from_plan_and_stats(plan, stats) -> deps
3. CACHE:    plan_cache.insert_with_deps(fp, plan, deps)
4. REGISTER: incremental.register_plan_dependencies(fp, deps)
5. LOOKUP:   plan_cache.lookup(fp) -> Some(plan)   [O(1), no checking]
6. CHANGE:   ANALYZE detects orders.row_count 10x increase
7. EMIT:     StatisticsChange::RowCount { table: "orders", ... }
8. COMPUTE:  incremental.compute_affected_plans([change]) -> [fp]
9. INVALIDATE: plan_cache.invalidate([fp])
             - hot entry (hit_count >= 100): mark stale
             - cold entry (hit_count < 100): evict
10. NEXT ACCESS:
    - If evicted: cache miss -> goto step 1
    - If stale: attempt RFC 0054 adjustment -> if ok, update entry
                                            -> if fail, evict -> step 1
11. CLEANUP: On eviction: incremental.unregister_plan_dependencies(fp)
```

### Error handling

Change detection is best-effort. If the differential dataflow computation fails (e.g., timely worker error), the error is logged via `tracing::warn!` and the affected change event is dropped. The cache entry remains valid until the next successful change propagation or LRU eviction. LRU eviction remains as a capacity-based fallback -- it is not replaced by differential invalidation.

If a statistics provider becomes temporarily unavailable, no change events are emitted and cached plans remain in use. This is safe: the worst case is serving a stale plan until statistics become available again, which is identical to the current behavior.

### Performance considerations

**Cache lookup**: O(1) hash table lookup. Zero staleness-checking overhead. No fingerprint comparison, no generation counter check. If a plan is in the cache and not marked stale, it is valid.

**Change propagation**: When a statistics change occurs, `compute_affected_plans()` runs a differential dataflow computation. Cost is O(changes x deps) for the join, but in practice the join is highly selective: most changes affect a small fraction of cached plans. For a 1024-entry cache where a single table change affects 10 plans, the computation takes <1ms.

**Dependency registration**: O(resources per plan) at insertion time, typically 10-30 resources for a 3-table query. Stored in a `HashMap<QueryFingerprint, PlanDependencies>` -- one entry per cached plan.

**Memory overhead**: `PlanDependencies` stores table names, column names, and index names as strings, plus histogram digests. Per entry: 500-800 bytes (same as the previous fingerprint design). The dependency map adds one `HashMap` entry per cached plan.

**Comparison with polling**: For an OLTP workload (1M queries/sec, 1 ANALYZE/hour):
- Polling: 1M accesses/sec x O(10 deps each) = 10M checks/sec
- Differential: 1 change/hour x O(100 affected plans) = 100 invalidations/hour, plus 1M x O(1) lookups/sec
- Differential is approximately 10,000x more efficient in per-access overhead.

## Drawbacks

- **Differential dataflow dependency**: This design requires the timely/differential-dataflow infrastructure to be running. If Ra is used without differential dataflow (e.g., embedded mode without the timely runtime), invalidation does not operate and the cache falls back to LRU-only behavior. This is acceptable as a degradation mode but means the feature is not available in all deployment configurations.
- **Storage overhead**: Each cache entry grows by 500-800 bytes for `PlanDependencies`. The `IncrementalOptimizer` maintains a parallel map of `QueryFingerprint -> PlanDependencies`. For a 1024-entry cache this is under 2 MB total, but the duplication between cache and optimizer is a maintenance concern.
- **Invalidation latency**: Change events are asynchronous. Between when a statistics change occurs and when the differential dataflow computation completes, a stale plan could be served. The window is small (sub-millisecond for the computation itself) but non-zero. For workloads where even a single stale plan execution is unacceptable, `max_age`-based sweep provides a synchronous safety net.
- **False positives from thresholds**: Threshold-based detection can flag a plan as stale when the statistics change does not actually affect plan quality. A table growing from 100K to 200K rows might not change the optimal join method, but a 2x threshold triggers invalidation regardless. The cost is an unnecessary re-optimization, not an incorrect result.
- **Histogram comparison limitations**: KL-divergence on bucket frequencies is an approximation. Different histograms can produce the same digest if they have the same bucket count and frequency distribution but different boundary values.
- **Complexity**: The design adds `PlanDependencies`, `ResourceId`, `ChangeSource`, and the `compute_affected_plans()` dataflow to the existing `IncrementalOptimizer`. The plan cache gains `invalidate()`, `invalidate_for_table()`, soft vs hard invalidation, and the `stale` flag. This is more machinery than a polling approach, though the per-access simplicity (O(1) lookup with no checking) compensates.
- **Dependency maintenance**: When a plan is evicted (by LRU or invalidation), its dependencies must be unregistered from the `IncrementalOptimizer`. Forgetting to call `unregister_plan_dependencies()` leaks entries in the dependency map.

## Rationale and alternatives

### Why This Design?

**Event-driven over polling** because the common case (no statistics change) should have zero per-access cost. Ra already uses differential dataflow for rule change propagation; extending it to statistics changes is a natural fit that unifies the invalidation infrastructure.

**Differential dataflow over ad-hoc dependency tracking** because the join-based computation is correct by construction: if a plan depends on a resource and that resource changes, the plan is found. No manual graph traversal or subscription management is needed.

**Multi-dimensional thresholds** because different statistics affect different plan decisions. Row-count-only drift detection (as currently implemented in `ra-cache::validity`) misses the most impactful changes: index drops that silently degrade performance and NDV shifts that change join ordering.

**Threshold-based ratios at the source** rather than per-access comparison because evaluating thresholds once when statistics change is cheaper than evaluating them on every cache lookup.

**Soft + hard invalidation** because hot plans (high hit count) deserve an opportunity for lightweight adjustment (RFC 0054) before falling back to expensive re-optimization. Cold plans (low hit count) are not worth adjusting.

### Alternative Approaches

**Polling on every cache access** (the previous version of this RFC): Compare a statistics fingerprint against current statistics on each `lookup()` call. Rejected: O(deps) cost per access is expensive for OLTP workloads. Generation-based short-circuiting reduces this to O(1) in the no-change case, but the design is fundamentally polling-based -- it checks whether something changed rather than being told when something changed.

**Always re-optimize on access**: Run the optimizer on every cache hit and compare the result. Rejected: full re-optimization through equality saturation is too expensive for OLTP workloads, even when the same plan comes out.

**Timestamp-based expiration (TTL)**: Evict plans older than N minutes. Rejected: does not correlate with actual staleness. A plan for a static dimension table never needs eviction; a plan for a rapidly-growing staging table needs invalidation within seconds.

**Subscription-based invalidation (without differential dataflow)**: Each cache entry subscribes to change events for its resource dependencies. When a change occurs, subscribers are notified directly. This is simpler than differential dataflow but requires manual subscription management (subscribe on insert, unsubscribe on evict, handle subscription leaks). Differential dataflow handles this correctly by construction through the collection lifecycle.

### Impact of Not Doing This

Without statistics-based invalidation, the plan cache silently serves suboptimal plans after data changes. Users experience unpredictable performance degradation that is difficult to diagnose because the plan was optimal when it was cached. The only recourse is manual cache flushing (`DISCARD PLANS` in PostgreSQL) or waiting for LRU eviction, neither of which is satisfactory.

## Prior art

### Academic Research

- **Parametric Query Optimization** (Ioannidis et al., VLDB 1992): Pre-computes optimal plans for different parameter ranges. The plan dependencies serve a similar purpose: they delineate the region of statistics space where a cached plan is valid.
- **Progressive Optimization** (Markl et al., SIGMOD 2004): Re-optimizes mid-execution when cardinality estimates prove wrong. This RFC handles pre-execution staleness; RFC 0052 handles mid-execution divergence.
- **Adaptive Query Processing Survey** (Deshpande et al., 2007): Covers the full spectrum of adaptation techniques. Event-driven invalidation sits between static caching (no adaptation) and continuous adaptation (eddies).
- **Naiad** (Murray et al., SOSP 2013): Timely dataflow for incremental computation. The differential dataflow library used by Ra is built on Naiad's principles. Using it for dependency-based invalidation is a natural application.

### Industry Solutions

- **PostgreSQL**: Prepared statement plans are invalidated by DDL (schema changes) and can be forced to re-plan via `plan_cache_mode = force_custom_plan`. Statistics updates from `ANALYZE` do not automatically invalidate cached plans. The `generic_plan_cost` heuristic compares generic plan cost to average custom plan cost, but this is parameter-aware, not statistics-aware.
- **SQL Server**: Uses dependency tracking for plan invalidation. Statistics updates trigger plan recompilation for affected queries via internal dependency tracking. The `AUTO_UPDATE_STATISTICS` option controls when statistics are refreshed. Plan cache entries have "correctness" and "optimality" reasons for recompilation, with statistics changes falling under optimality. This is the closest industry analog to our approach -- SQL Server tracks which statistics a plan depends on and invalidates when those statistics change.
- **Oracle**: SQL Plan Management (SPM) stores plan baselines and evolves them. Statistics changes trigger plan re-evaluation against the baseline. Oracle also supports automatic reoptimization where plans are marked for re-optimization after the first execution reveals estimation errors.
- **MySQL**: `ANALYZE TABLE` invalidates cached plans for the analyzed table. No threshold-based detection; any statistics refresh triggers full invalidation. Simple but causes unnecessary cache churn.
- **DuckDB**: Re-optimizes every query (no plan caching for prepared statements in the traditional sense). The optimizer is fast enough that caching is not needed for typical analytical workloads.
- **Materialize**: Uses differential dataflow for materialized view maintenance. Change events flow through the dataflow graph and update downstream views incrementally. Our approach applies the same principle to plan cache invalidation rather than view maintenance.

### What We Can Learn

1. **From SQL Server**: Dependency-based invalidation works in production and is the expected behavior. Ra should match SQL Server's precision (invalidate only affected plans) while using differential dataflow instead of SQL Server's internal dependency graph.
2. **From Materialize**: Differential dataflow is a proven technology for incremental change propagation. Extending it to plan cache invalidation is a natural application.
3. **From PostgreSQL**: Not invalidating on statistics changes is a known pain point. The `plan_cache_mode` GUC exists specifically because users need to work around stale cached plans.
4. **From MySQL**: Invalidating on any statistics change (no threshold) causes excessive churn. Thresholds are necessary.

## Unresolved questions

### Design questions

1. **Optimal threshold values by workload type**: Should Ra ship separate threshold presets for OLTP (more sensitive: 1.5x cardinality, 1.2x NDV) and OLAP (less sensitive: 5x cardinality, 3x NDV) workloads? Or should a single set of defaults serve both, with explicit configuration for tuning?

2. **Correlated column statistics**: When two columns are correlated (e.g., `city` and `state`), their NDV values change together but the combined selectivity changes non-linearly. Should the dependencies track multi-column statistics, or is per-column NDV sufficient for initial implementation?

3. **Dependency granularity**: Should dependencies be per-fingerprint (as proposed) or per-plan-variant (when fuzzy matching produces multiple plans for similar fingerprints)? Per-fingerprint is simpler; per-variant is more precise.

4. **LRU interaction**: LRU eviction remains as a capacity fallback. When both LRU and differential invalidation want to evict an entry, differential invalidation takes priority (it has semantic information). Should LRU eviction also trigger `unregister_plan_dependencies()`? (Yes -- otherwise the dependency map leaks.)

### Implementation questions

1. **Dependency scope**: Should dependencies include only statistics for tables directly scanned by the plan, or also statistics for tables referenced in subqueries and CTEs?

2. **`ra-cache` unification**: The `ra-cache::validity` module already implements row-count drift detection via polling. This RFC replaces that mechanism entirely with differential invalidation. Should the polling code in `ra-cache` and `ra-adaptive::StatisticsPoller` be removed, or retained as a fallback for deployments without differential dataflow?

3. **Threshold auto-tuning**: RFC 0060's `GeneticTuner` could evolve `StalenessThresholds` alongside `OptimizerConfig` parameters. Should this integration be part of this RFC or deferred?

4. **Timely worker lifecycle**: `compute_affected_plans()` currently creates a new timely worker per invocation (matching `compute_affected_queries()`). For high-frequency change events, a persistent worker with maintained collections would amortize setup cost. Should the initial implementation use transient workers (simpler) or persistent workers (faster)?

### Out of scope

- **Mid-execution statistics-based replanning**: Covered by RFC 0052 (Progressive Re-Optimization).
- **Incremental plan adjustment logic**: Covered by RFC 0054 (Streaming Plan Adjustments). This RFC defines the invalidation signal; RFC 0054 defines what to do with it.
- **Machine-learned threshold selection**: Using historical query performance to learn optimal thresholds per table or per query pattern.

## Future possibilities

### Natural Extensions

- **Persistent differential collections**: Instead of building transient timely workers per change event, maintain persistent differential collections that accumulate dependency edges and change events over time. This enables incremental computation: adding a new plan only inserts its dependency edges, and a new change event only processes the delta. This is the natural evolution toward a fully incremental system.
- **Reactive threshold refinement**: After a staleness-triggered re-optimization produces the same plan as the cached one (false positive), widen the threshold for that entry's resources to reduce future false positives.
- **Multi-column statistics dependencies**: Track joint NDV and functional dependencies between columns to detect correlated distribution shifts that per-column NDV misses.
- **Cost-delta estimation**: Before evicting a stale entry, estimate the cost delta between the cached plan under old statistics and under new statistics. If the delta is below a threshold, suppress the invalidation. This reduces false positives from threshold crossings that do not actually affect plan quality.
- **Cross-plan batch invalidation**: When `ANALYZE` updates many tables at once, the differential dataflow computation naturally handles this in a single join pass -- all change events for all tables are inserted into the changes collection simultaneously, and the join produces all affected fingerprints in one step.

### Long-term Vision

Statistics-based invalidation via differential dataflow completes the plan cache lifecycle and unifies Ra's change propagation infrastructure:

1. **RFC 0060**: Cache lookup via structural fingerprint (query identity).
2. **This RFC**: Event-driven invalidation via differential dataflow (plan validity).
3. **RFC 0054**: Incremental adjustment when invalidation marks a plan stale (plan repair).
4. **RFC 0052**: Full re-optimization when adjustment fails (plan replacement).

All four mechanisms share the same differential dataflow runtime:
- Rule changes flow through `compute_affected_queries()` to reoptimize registered queries.
- Statistics/index/fact changes flow through `compute_affected_plans()` to invalidate cached plans.
- Both use the same timely infrastructure, the same dependency tracking pattern, and the same join-based computation.

This unification means that as Ra evolves, any new change source (e.g., schema changes, constraint additions, workload shifts) can be plugged into the same pipeline with a new `ChangeSource` variant and a new set of dependency edges.
