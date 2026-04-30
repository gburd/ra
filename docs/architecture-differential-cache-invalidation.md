# Differential Dataflow Plan Cache Invalidation

## Executive Summary

**Current State**: Plan cache uses only LRU eviction (capacity-based). Cached plans never invalidate based on statistics/index/fact changes, leading to stale plans.

**Solution**: Extend differential dataflow (already used for rule-based query reoptimization) to track statistics/index/fact changes and invalidate affected cached plans.

**Key Benefits**:
- Event-driven (not polling): O(1) cache access, O(affected plans) on change
- Incremental: Only compute affected plans, not all plans
- Unified: Same infrastructure for rules, stats, indexes, facts
- Precise: Track explicit dependencies, invalidate only what's affected

## Architecture

```
                    Differential Dataflow Collections
                    ─────────────────────────────────
Rule Changes        →   Collection[RuleChange]          (current ✅)
Statistics Changes  →   Collection[StatisticsChange]    (NEW)
Index Changes       →   Collection[IndexChange]         (NEW)
Fact Changes        →   Collection[FactChange]          (NEW)
                              ↓
                    Join with Dependencies
                              ↓
                    Affected Plan Fingerprints
                              ↓
                    Plan Cache Invalidation
```

## How It Works

### 1. Track Plan Dependencies

When a plan is cached, record what it depends on:

```rust
pub struct PlanDependencies {
    /// Tables whose cardinality influenced this plan
    pub table_cardinalities: HashMap<String, u64>,
    
    /// Indexes this plan uses
    pub indexes: HashSet<(String, String)>,  // (table, index)
    
    /// Distinct counts that affect selectivity
    pub distinct_counts: HashMap<(String, String), u64>,
    
    /// Facts that enabled certain rules
    pub facts: HashSet<String>,
}

// Example for: SELECT * FROM orders WHERE status = ?
let deps = PlanDependencies {
    table_cardinalities: {
        "orders" => 1_000_000,
    },
    indexes: {
        ("orders", "orders_status_idx"),
    },
    distinct_counts: {
        ("orders", "status") => 5,
    },
    facts: {},
};
```

### 2. Detect Significant Changes

Statistics provider monitors for threshold crossings:

```rust
// In streaming statistics pipeline
impl StreamingPipeline {
    pub fn check_thresholds(&self) -> Vec<StatisticsChange> {
        let mut changes = Vec::new();
        
        if self.row_count_changed_by_factor(2.0) {
            changes.push(StatisticsChange::RowCount {
                table: "orders".into(),
                old_value: 1_000_000,
                new_value: 10_000_000,
                factor: 10.0,
            });
        }
        
        if self.distinct_count_changed_by_factor(1.5) {
            changes.push(StatisticsChange::DistinctCount {
                table: "orders".into(),
                column: "status".into(),
                old_value: 5,
                new_value: 50,
                factor: 10.0,
            });
        }
        
        changes
    }
}
```

### 3. Differential Computation

Extend existing `IncrementalOptimizer` to compute affected plans:

```rust
impl IncrementalOptimizer {
    pub fn compute_affected_plans(
        &self,
        changes: &[ChangeSource],
    ) -> Vec<QueryFingerprint> {
        timely::execute_directly(|worker| {
            worker.dataflow(|scope| {
                // Collection 1: Changed resources
                let (mut changes_input, changes_coll) = 
                    scope.new_collection::<ResourceId, isize>();
                
                // Collection 2: (fingerprint, resource_id) dependencies
                let (mut deps_input, deps_coll) = 
                    scope.new_collection::<(QueryFingerprint, ResourceId), isize>();
                
                // Join: Find plans depending on changed resources
                let affected_plans = changes_coll
                    .map(|resource_id| (resource_id, ()))
                    .join(&deps_coll)
                    .map(|(_resource, ((), fp))| fp);
                
                // Insert data and compute
                for change in changes {
                    changes_input.insert(change.resource_id());
                }
                
                for (fp, deps) in &self.plan_dependencies {
                    for resource in deps.all_resources() {
                        deps_input.insert((fp.clone(), resource));
                    }
                }
                
                // Collect results
                affected_plans.inspect(|fp| {
                    results.push(fp.clone());
                });
            });
        });
    }
}
```

### 4. Invalidate Affected Plans

```rust
// When statistics change significantly
stats_pipeline.on_threshold_exceeded(|change| {
    let affected = incremental.compute_affected_plans(&[
        ChangeSource::Statistics(change)
    ]);
    plan_cache.invalidate(&affected);
});
```

## Example: Table Growth Scenario

### Initial State
```
Table: orders
  Row count: 1,000,000
  Distinct status values: 5

Cached Plan A (fingerprint_A):
  Query: SELECT * FROM orders WHERE status = ?
  Plan: Index scan on orders_status_idx
  Dependencies: {
      table_cardinalities: { "orders": 1_000_000 },
      indexes: { ("orders", "orders_status_idx") },
      distinct_counts: { ("orders", "status"): 5 },
  }

Cached Plan B (fingerprint_B):
  Query: SELECT * FROM users WHERE city = ?
  Plan: Index scan on users_city_idx
  Dependencies: {
      table_cardinalities: { "users": 50_000 },
      indexes: { ("users", "users_city_idx") },
  }
```

### After Data Growth
```
ANALYZE orders;  -- Now has 10,000,000 rows

Statistics Provider detects:
  orders.row_count: 1,000,000 → 10,000,000 (10x change, threshold: 2x)
  
Threshold exceeded\! Trigger invalidation.
```

### Differential Computation
```
Step 1: Insert change into differential collection
  changes_coll = [("orders.row_count", ())]

Step 2: Query plan dependencies
  deps_coll = [
      (fingerprint_A, "orders.row_count"),
      (fingerprint_B, "users.row_count"),   // Different table\!
  ]

Step 3: Join to find affected plans
  affected_plans = changes_coll.join(deps_coll)
                 = [fingerprint_A]
  // fingerprint_B NOT affected (depends on users, not orders)

Step 4: Invalidate
  plan_cache.invalidate([fingerprint_A])
  // Only 1 plan invalidated, not all 1024\!
```

### Next Access
```
// User executes: SELECT * FROM orders WHERE status = 'shipped'
optimizer.optimize(query);
  ↓
plan_cache.lookup(fingerprint_A);
  ↓
Cache miss (invalidated\!)
  ↓
Full optimization with new statistics
  ↓
New plan: Sequential scan (10M rows, selectivity 0.2 = 2M rows)
           vs Index scan (2M index accesses)
           Sequential wins\!
  ↓
plan_cache.insert(fingerprint_A, new_plan, new_deps)
```

## Integration Points

### With Streaming Statistics (Track 2)
```rust
// crates/ra-stats-advanced/src/streaming.rs
impl StreamingPipeline {
    pub fn on_significant_change<F>(&mut self, callback: F)
    where
        F: Fn(StatisticsChange) + Send + 'static,
    {
        self.change_callback = Some(Box::new(callback));
    }
}

// In optimizer initialization
stats_pipeline.on_significant_change(move |change| {
    incremental_optimizer.notify_statistics_change(change);
    let affected = incremental_optimizer.compute_affected_plans(&[
        ChangeSource::Statistics(change)
    ]);
    plan_cache.invalidate(&affected);
});
```

### With PostgreSQL Extension
```rust
// crates/ra-pg-extension/src/stats_bridge.rs
impl PostgresStatsBridge {
    pub fn on_analyze_complete(&self, table: &str) {
        let old_stats = self.cached_stats.get(table);
        let new_stats = self.gather_table_stats(table);
        
        if let (Some(old), Some(new)) = (old_stats, new_stats) {
            if new.row_count > old.row_count * 2 {
                self.notify_change(StatisticsChange {
                    table: table.into(),
                    old_value: old.row_count as f64,
                    new_value: new.row_count as f64,
                    factor: (new.row_count as f64) / (old.row_count as f64),
                });
            }
        }
    }
}
```

### With Index Management
```rust
// When CREATE INDEX / DROP INDEX detected
impl IndexMonitor {
    pub fn on_index_change(&self, change: IndexChange) {
        let affected = incremental_optimizer.compute_affected_plans(&[
            ChangeSource::Index(change)
        ]);
        plan_cache.invalidate(&affected);
    }
}
```

## Performance Analysis

### Comparison Table

| Approach | Check Frequency | Cost per Access | Cost per Change | Invalidation Precision |
|----------|----------------|----------------|----------------|----------------------|
| **None (current)** | Never | O(1) | O(1) | N/A - no invalidation |
| **Manual polling (RFC 0054)** | Every access | O(deps) | O(1) | High |
| **Differential (this approach)** | On change | O(1) | O(affected) | Perfect |

### Example Workload

**Scenario**: 1000 cache accesses, 1 statistics change affecting 2 plans

**Manual polling**:
- 1000 accesses × O(10 dependencies each) = 10,000 dependency checks
- Total cost: O(10,000)

**Differential**:
- 1 change × O(2 affected plans via join) = 2 invalidations
- 1000 accesses × O(1) = 1000 lookups
- Total cost: O(1002)

**Speedup**: ~10x more efficient

### Real-World Impact

**OLTP workload** (1M queries/sec, 1 ANALYZE/hour):
- Manual: 1M × O(10) = 10M checks/sec
- Differential: 1 × O(100 affected) = 100 invalidations/hour + 1M × O(1) lookups
- **Differential is 10,000x more efficient**

## Open Questions

1. **Threshold Values**
   - Cardinality: 2x? 10x?
   - Distinct count: 1.5x? 2x?
   - Should threshold vary by workload type (OLTP vs OLAP)?

2. **Dependency Granularity**
   - Per-fingerprint or per-plan?
   - Should we track join order dependencies?

3. **LRU Interaction**
   - Keep LRU as capacity fallback? (Likely yes)
   - What if both LRU and differential want to evict?

4. **Correlated Statistics**
   - How to handle multi-column correlations?
   - Track join graph statistics?

## Related Work

- **PostgreSQL**: Prepared statement auto-invalidation on DDL
- **Oracle**: Shared pool aging, adaptive cursor sharing
- **SQL Server**: Query Store plan forcing, regression detection
- **Materialize**: Differential dataflow for materialized view maintenance

## Implementation Plan

### Phase 1: Core Infrastructure (2 weeks)
1. Extend `IncrementalOptimizer` with `compute_affected_plans`
2. Add `PlanDependencies` tracking to plan cache
3. Implement `ChangeSource` enum (Statistics, Index, Fact)

### Phase 2: Statistics Integration (1 week)
4. Connect streaming statistics to differential dataflow
5. Implement threshold detection and change notification
6. Add PostgreSQL ANALYZE hook

### Phase 3: Index and Fact Integration (1 week)
7. Implement index change detection
8. Connect facts provider to differential dataflow
9. Add DDL change detection (CREATE/DROP INDEX)

### Phase 4: Testing and Tuning (1 week)
10. Integration tests with JOB benchmark
11. Threshold tuning experiments
12. Performance benchmarks

## RFC Status

This document describes the architectural approach for implementing RFC 0059: Statistics-Based Plan Cache Invalidation using differential dataflow.

**Dependencies**:
- ✅ RFC 0060: Genetic Fingerprinting (implemented)
- ✅ Track 2: Streaming Statistics System (implemented)
- ✅ Differential dataflow infrastructure (exists in differential.rs)
- ⏸️ Integration work (this RFC)

**Next Steps**:
1. Write formal RFC 0059 following TEMPLATE.md
2. Review with team for architectural approval
3. Begin Phase 1 implementation
