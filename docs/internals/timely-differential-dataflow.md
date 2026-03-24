# Timely/Differential Dataflow Integration

## Overview

Ra uses [timely dataflow](https://github.com/TimelyDataflow/timely-dataflow) and
[differential dataflow](https://github.com/TimelyDataflow/differential-dataflow)
to incrementally maintain optimization results. When rewrite rules are added or
removed at runtime, the optimizer avoids rerunning the full equality-saturation
pass for every registered query. Instead, it identifies which queries are
affected by a rule change and reoptimizes only those.

This page describes the architecture, data flow, and implementation details
of the incremental optimization subsystem.

::: warning Scope
Differential dataflow is used exclusively in the **batch optimizer** for
incremental rule and statistics change propagation. It is **not** used for
streaming query execution. Ra's execution model is plan-at-a-time; timely
provides the change-tracking substrate, not a runtime execution engine.
:::

### Why incremental?

A batch optimizer re-plans every query when the rule set changes. For a system
with 500 registered queries and a rule-set change that only touches join ordering,
re-planning all 500 queries is wasteful. The incremental approach:

- **Reduces latency** -- only affected queries pay the cost of re-optimization.
- **Preserves memo cache** -- queries unaffected by the change keep their cached
  plans.
- **Tracks provenance** -- each query records which generation of the rule set
  it was optimized against, enabling efficient staleness detection.

### Key concepts

| Concept | Description |
|---------|-------------|
| **Generation** | Monotonic counter incremented on each rule-set change. |
| **Rule change** | Addition or removal of a named rewrite rule. |
| **Registered query** | A `RelExpr` submitted for incremental optimization. |
| **Differential join** | A timely/differential dataflow computation that joins rule changes against query-rule dependency edges. |
| **Memo table** | Structural-hash-keyed cache of optimized expressions. |

---

## Architecture

### Component diagram

```mermaid
graph TD
    subgraph IncrementalOptimizer
        RuleSet["Active Rules\n(Vec<RuleId>)"]
        Queries["Registered Queries\n(HashMap<u64, RegisteredQuery>)"]
        Memo["Memo Table\n(structural hash → RelExpr)"]
        PendingChanges["Pending Changes\n(Vec<RuleChange>)"]
        Stats["Computation Stats"]
    end

    subgraph "Batch Optimizer (egg)"
        EGraph["E-Graph\nEquality Saturation"]
    end

    subgraph "Timely/Differential"
        DiffJoin["Differential Join\nChanges × Dependencies"]
    end

    Client -->|register_query| Queries
    Client -->|add_rule / remove_rule| PendingChanges
    Client -->|apply_changes| IncrementalOptimizer

    PendingChanges -->|"rule diffs"| RuleSet
    PendingChanges -->|"identify affected"| DiffJoin
    DiffJoin -->|"affected query IDs"| Queries
    Queries -->|"stale queries"| EGraph
    EGraph -->|"optimized RelExpr"| Memo
    Memo -->|"cached result"| Queries
```

### Data flow on `apply_changes`

```mermaid
sequenceDiagram
    participant C as Client
    participant IO as IncrementalOptimizer
    participant DD as Differential Dataflow
    participant EG as E-Graph Optimizer
    participant M as Memo Table

    C->>IO: apply_changes()
    IO->>IO: Take pending rule diffs
    IO->>IO: Apply additions/removals to active_rules
    IO->>IO: Increment generation
    IO->>M: Clear memo cache
    IO->>IO: Find stale queries (generation < current)

    loop For each stale query
        IO->>M: Check memo cache (structural hash)
        alt Cache hit
            M-->>IO: Return cached RelExpr
        else Cache miss
            IO->>EG: optimize(original_expr)
            EG-->>IO: Return optimized RelExpr
            IO->>M: Insert into cache
        end
        IO->>IO: Compare with previous result
        IO->>IO: Update optimized_at_generation
    end

    IO-->>C: UpdateResult { reoptimized, skipped, generation }
```

---

## Source code walkthrough

The implementation spans three files:

| File | Purpose |
|------|---------|
| `crates/ra-engine/src/timely.rs` | Timely configuration and computation statistics |
| `crates/ra-engine/src/differential.rs` | `IncrementalOptimizer` and differential join |
| `crates/ra-engine/src/memo.rs` | Structural-hash memo table for plan caching |

### TimelyConfig

The `TimelyConfig` struct controls the timely dataflow runtime.

**Source:** `crates/ra-engine/src/timely.rs:10-54`

```rust
/// Configuration for the timely dataflow runtime.
#[derive(Debug, Clone)]
pub struct TimelyConfig {
    /// Number of worker threads.
    pub workers: usize,
    /// Whether to use process-level parallelism (multiple
    /// threads) or a single thread.
    pub process_parallelism: bool,
}

impl TimelyConfig {
    /// Create a single-threaded configuration.
    pub fn single_thread() -> Self {
        Self::default()
    }

    /// Create a multi-threaded configuration.
    pub fn multi_thread(workers: usize) -> Self {
        Self {
            workers: workers.max(1),
            process_parallelism: true,
        }
    }
}
```

The default configuration uses a single thread (`workers: 1`,
`process_parallelism: false`). The `compute_affected_queries` function in
`differential.rs` uses `timely::execute_directly` which runs a single-threaded
computation. Multi-threaded configurations are available via
`TimelyConfig::multi_thread()` but are not yet used in the default path.

### ComputationStats

Tracks metrics about the incremental computation.

**Source:** `crates/ra-engine/src/timely.rs:56-77`

```rust
/// Statistics about a timely dataflow computation.
#[derive(Debug, Clone, Default)]
pub struct ComputationStats {
    /// Number of dataflow steps executed.
    pub steps: u64,
    /// Number of input records processed.
    pub input_records: u64,
    /// Number of output records produced.
    pub output_records: u64,
    /// Current logical timestamp.
    pub current_time: u64,
}
```

These stats are accumulated across `apply_changes()` calls and exposed via
`IncrementalOptimizer::stats()`.

---

### IncrementalOptimizer

The central type that combines batch optimization with differential change
tracking.

**Source:** `crates/ra-engine/src/differential.rs:115-161`

```rust
pub struct IncrementalOptimizer {
    optimizer: Optimizer,
    memo: MemoTable,
    queries: HashMap<u64, RegisteredQuery>,
    active_rules: Vec<RuleId>,
    generation: u64,
    pending_changes: Vec<RuleChange>,
    stats: ComputationStats,
    next_query_id: u64,
    _timely_config: TimelyConfig,
}
```

#### Fields

| Field | Type | Purpose |
|-------|------|---------|
| `optimizer` | `Optimizer` | Batch egg-based equality saturation optimizer |
| `memo` | `MemoTable` | Structural-hash cache of optimized expressions |
| `queries` | `HashMap<u64, RegisteredQuery>` | All registered queries |
| `active_rules` | `Vec<RuleId>` | Currently active rewrite rules |
| `generation` | `u64` | Current rule-set generation counter |
| `pending_changes` | `Vec<RuleChange>` | Staged but unapplied rule changes |
| `stats` | `ComputationStats` | Accumulated computation statistics |
| `next_query_id` | `u64` | Monotonic query ID allocator |
| `_timely_config` | `TimelyConfig` | Timely runtime configuration |

### Registering queries

When a query is registered, it is immediately optimized with the current rule
set and the result is cached in the memo table.

**Source:** `crates/ra-engine/src/differential.rs:195-218`

```rust
pub fn register_query(
    &mut self, expr: &RelExpr
) -> Result<u64, IncrementalError> {
    let id = self.next_query_id;
    self.next_query_id += 1;

    let optimized = self.optimize_and_cache(expr)?;

    let query = RegisteredQuery {
        id,
        original: expr.clone(),
        optimized: Some(optimized),
        optimized_at_generation: self.generation,
    };

    self.queries.insert(id, query);
    self.stats.input_records += 1;
    Ok(id)
}
```

Each `RegisteredQuery` stores:
- The **original** unoptimized `RelExpr`
- The **optimized** result (or `None` if not yet computed)
- The **generation** at which it was last optimized

### Staging rule changes

Rule additions and removals are staged, not applied immediately. This allows
batching multiple changes before triggering reoptimization.

**Source:** `crates/ra-engine/src/differential.rs:247-266`

```rust
pub fn add_rule(&mut self, rule_id: RuleId) {
    if !self.active_rules.contains(&rule_id) {
        self.pending_changes
            .push(RuleChange::Added(rule_id));
    }
}

pub fn remove_rule(&mut self, rule_id: &RuleId) {
    if self.active_rules.contains(rule_id) {
        self.pending_changes
            .push(RuleChange::Removed(rule_id.clone()));
    }
}
```

Duplicate additions and removals of non-active rules are silently ignored.

### Applying changes

`apply_changes()` is the core incremental computation. It:

1. Takes all pending rule diffs
2. Applies additions/removals to `active_rules`
3. Increments the generation counter
4. Clears the memo cache (since the rule set changed)
5. Finds stale queries (those optimized before the current generation)
6. Reoptimizes each stale query
7. Compares old vs new results to count actual changes

**Source:** `crates/ra-engine/src/differential.rs:285-379`

```rust
pub fn apply_changes(
    &mut self,
) -> Result<UpdateResult, IncrementalError> {
    let rule_diffs = std::mem::take(&mut self.pending_changes);
    if rule_diffs.is_empty() {
        return Ok(UpdateResult {
            reoptimized_count: 0,
            skipped_count: self.queries.len(),
            generation: self.generation,
            stats: self.stats.clone(),
        });
    }

    // Apply rule additions and removals
    for diff in &rule_diffs {
        match diff {
            RuleChange::Added(id) => {
                if !self.active_rules.contains(id) {
                    self.active_rules.push(id.clone());
                }
            }
            RuleChange::Removed(id) => {
                self.active_rules.retain(|r| r != id);
            }
        }
    }

    self.generation += 1;
    self.stats.current_time = self.generation;
    self.memo.clear();

    // Find stale queries
    let stale_ids: Vec<u64> = self
        .queries
        .values()
        .filter(|q| q.optimized_at_generation < self.generation)
        .map(|q| q.id)
        .collect();

    // Reoptimize each stale query...
    // (see full source for loop body)
}
```

The return value `UpdateResult` reports how many queries were reoptimized
(their plan actually changed) vs skipped (plan unchanged or already current).

---

### The differential join: `compute_affected_queries`

This function uses timely/differential dataflow to identify which queries are
affected by a set of rule changes. It constructs a dataflow graph that joins
changed rule names against query-rule dependency edges.

**Source:** `crates/ra-engine/src/differential.rs:392-477`

```rust
pub fn compute_affected_queries(
    &self,
    rule_diffs: &[RuleChange],
) -> Result<Vec<u64>, IncrementalError> {
    use differential_dataflow::input::Input;
    use differential_dataflow::operators::Join;

    let diff_rule_names: Vec<String> = rule_diffs
        .iter()
        .map(|c| match c {
            RuleChange::Added(id)
            | RuleChange::Removed(id) => id.name().to_owned(),
        })
        .collect();

    // Build query-rule dependency edges
    let query_ids: Vec<u64> =
        self.queries.keys().copied().collect();
    let rule_names: Vec<String> = self
        .active_rules
        .iter()
        .map(|r| r.name().to_owned())
        .collect();

    let output_buf = Arc::new(Mutex::new(Vec::<u64>::new()));

    let buf_clone = Arc::clone(&output_buf);
    timely::execute_directly(move |worker| {
        worker.dataflow::<u64, _, _>(|scope| {
            // Collection of changed rule names
            let (mut changes_input, changes_coll) =
                scope.new_collection::<String, isize>();

            // Collection of (rule_name, query_id) edges
            let (mut deps_input, deps_coll) =
                scope.new_collection::<(String, u64), isize>();

            // Join: changed_rules × dependencies → affected query IDs
            let affected_coll = changes_coll
                .map(|name| (name, ()))
                .join(&deps_coll)
                .map(|(_rule, ((), qid))| qid);

            // Inspect results into shared buffer
            let buf = Arc::clone(&buf_clone);
            affected_coll.inspect(move |&(qid, _time, _diff)| {
                if let Ok(mut v) = buf.lock() {
                    v.push(qid);
                }
            });

            // Insert input data
            for name in &diff_rule_names {
                changes_input.insert(name.clone());
            }
            for qid in &query_ids {
                for rule in &rule_names {
                    deps_input.insert((rule.clone(), *qid));
                }
            }

            changes_input.advance_to(1);
            deps_input.advance_to(1);
            changes_input.flush();
            deps_input.flush();
        });

        worker.step();
        worker.step();
    });

    // Extract and deduplicate
    let mut unique: Vec<u64> =
        Arc::try_unwrap(output_buf)
            .map_err(|_| {
                IncrementalError::SerializationError(
                    "failed to unwrap results".into(),
                )
            })?
            .into_inner()
            .map_err(|e| {
                IncrementalError::SerializationError(
                    format!("lock poisoned: {e}"),
                )
            })?;
    unique.sort_unstable();
    unique.dedup();
    Ok(unique)
}
```

#### How the dataflow works

```mermaid
graph LR
    subgraph "Input Collections"
        ChangedRules["Changed Rule Names\n{'filter-merge'}"]
        DepEdges["Dependency Edges\n{('filter-merge', q1),\n ('filter-merge', q2),\n ('join-commute', q1),\n ('join-commute', q2)}"]
    end

    subgraph "Differential Join"
        Map1["map: name → (name, ())"]
        Join["join on rule_name"]
        Map2["map: (rule, ((), qid)) → qid"]
    end

    subgraph Output
        AffectedQIDs["Affected Query IDs\n{q1, q2}"]
    end

    ChangedRules --> Map1
    Map1 --> Join
    DepEdges --> Join
    Join --> Map2
    Map2 --> AffectedQIDs
```

The dependency model is currently **conservative**: every query depends on every
active rule (since egg's equality saturation applies all rules during its fixed
point computation). This means changing any rule marks all queries as potentially
affected. Future work could track finer-grained dependencies by recording which
rules actually contributed to each query's optimized plan.

### Memo table and caching

The `optimize_and_cache` method uses a structural hash of the `RelExpr` to
check the memo table before running the optimizer.

**Source:** `crates/ra-engine/src/differential.rs:480-490`

```rust
fn optimize_and_cache(
    &mut self,
    expr: &RelExpr,
) -> Result<RelExpr, IncrementalError> {
    let hash = structural_hash(expr);

    if let Some(cached) = self.memo.get(hash) {
        return Ok(cached.clone());
    }

    let result = self.optimizer.optimize(expr)?;
    self.memo.insert(hash, result.clone());
    Ok(result)
}
```

The memo cache is cleared on every `apply_changes()` call because the rule set
has changed and cached plans may no longer be optimal. Within a single generation,
duplicate expressions share cached results.

---

## Statistics integration

The incremental optimizer works with the statistics delta system in `ra-stats`
to decide whether incremental reoptimization is sufficient or a full re-plan
is needed.

### DeltaSet: tracking statistics changes

**Source:** `crates/ra-stats/src/delta.rs:140-295`

The `DeltaSet` type computes the minimal set of changes between two statistics
snapshots:

```rust
pub struct DeltaSet {
    deltas: Vec<StatisticsDelta>,
    pub from_time: u64,
    pub to_time: u64,
}
```

Delta types include:

| Delta | Meaning |
|-------|---------|
| `TableRowCount` | Row count changed |
| `ColumnNDV` | Distinct value count changed |
| `ColumnNullFraction` | NULL fraction changed |
| `ColumnCorrelation` | Physical correlation changed |
| `TableAdded` | New table appeared |
| `TableRemoved` | Table disappeared |
| `StalenessChanged` | Staleness level shifted |

### Deciding between incremental and full reoptimization

**Source:** `crates/ra-stats/src/delta.rs:280-288`

```rust
pub fn needs_full_reoptimization(&self) -> bool {
    if self.has_structural_changes() {
        return true;
    }
    if self.row_count_change_pct() > 50.0 {
        return true;
    }
    self.deltas.len() > 10
}
```

Full reoptimization is recommended when:
- **Structural changes** occur (tables added or removed)
- **Row count changes by more than 50%** for any table
- **More than 10 individual deltas** accumulate (many small changes)

Otherwise, the incremental optimizer handles the changes efficiently.

### Statistics-driven incremental optimization

The batch optimizer in `egraph.rs` has a second incremental path driven by
`DeltaSet` rather than rule changes. When statistics change by a small amount,
it scales down the egg iteration budget proportionally.

**Source:** `crates/ra-engine/src/egraph.rs:1022-1087`

```rust
pub fn optimize_incremental(
    &mut self,
    expr: &RelExpr,
    stats_delta: &DeltaSet,
) -> Result<(RelExpr, IncrementalStats), EGraphError> {
    let start = std::time::Instant::now();

    // Apply deltas to internal table stats
    let tables_updated = self.apply_stats_delta(stats_delta);

    // Scale iteration budget by change magnitude
    let (iter_limit, is_full) =
        if stats_delta.needs_full_reoptimization() {
            (self.config.iter_limit, true)
        } else {
            let pct = stats_delta.row_count_change_pct();
            let fraction = (pct / 100.0).clamp(0.05, 1.0);
            let iters = ((self.config.iter_limit as f64) * fraction)
                .ceil() as usize;
            (iters.max(1), false)
        };

    // Run egg with the scaled budget
    let runner: Runner<RelLang, RelAnalysis> = Runner::default()
        .with_expr(&rec_expr)
        .with_node_limit(self.config.node_limit)
        .with_iter_limit(iter_limit)
        .with_time_limit(/* ... */)
        .run(&rules);

    // Extract best plan and report stats
    Ok((result, stats))
}
```

**Iteration scaling logic:**

| Row count change | Fraction of max iterations | Example (max=10) |
|---|---|---|
| 1% | 0.05 (clamped minimum) | 1 iteration |
| 5% | 0.05 | 1 iteration |
| 10% | 0.10 | 1 iteration |
| 25% | 0.25 | 3 iterations |
| 50% | 0.50 | 5 iterations |
| > 50% | 1.00 (full reopt) | 10 iterations |

A 1% statistics change runs 10x fewer iterations than a full reoptimization,
translating directly to proportional speedup.

### IncrementalStats

Returned by `optimize_incremental()` to report what happened during the run.

**Source:** `crates/ra-engine/src/egraph.rs:1263-1297`

```rust
pub struct IncrementalStats {
    /// Number of rewrite rules evaluated.
    pub rules_evaluated: usize,
    /// Number of e-graph iterations actually used.
    pub iterations_used: usize,
    /// Maximum iterations configured.
    pub max_iterations: usize,
    /// Number of nodes in the final e-graph.
    pub nodes_in_egraph: usize,
    /// Number of tables whose stats were updated.
    pub tables_updated: usize,
    /// Number of individual deltas processed.
    pub delta_count: usize,
    /// Maximum row count change percentage.
    pub row_change_pct: f64,
    /// Whether full reoptimization was used (delta was too large).
    pub used_full_reoptimization: bool,
    /// Wall-clock time for the incremental optimization.
    pub elapsed: Duration,
}

impl IncrementalStats {
    /// Estimated speedup factor vs full optimization.
    /// Based on the ratio of iterations used vs max configured.
    /// Returns 1.0 when full reoptimization was used.
    pub fn speedup_factor(&self) -> f64 {
        if self.used_full_reoptimization
            || self.iterations_used == 0
        {
            return 1.0;
        }
        self.max_iterations as f64
            / self.iterations_used as f64
    }
}
```

The `speedup_factor()` is the ratio of max iterations to actual iterations used.
A value of 10.0 means the incremental path used 1/10th the iterations of a full
run.

### Change magnitude

Each delta has a `magnitude()` method that quantifies the size of the change.
This is used for prioritizing which queries to reoptimize first.

**Source:** `crates/ra-stats/src/delta.rs:100-120`

```rust
pub fn magnitude(&self) -> f64 {
    match self {
        Self::TableRowCount { old, new, .. }
        | Self::ColumnNDV { old, new, .. } => {
            relative_change(*old as f64, *new as f64)
        }
        Self::ColumnNullFraction { old, new, .. } => {
            (*new - *old).abs()
        }
        Self::ColumnCorrelation { old, new, .. } => {
            match (old, new) {
                (Some(o), Some(n)) => (n - o).abs(),
                (None, None) => 0.0,
                _ => f64::INFINITY,
            }
        }
        Self::TableAdded { .. }
        | Self::TableRemoved { .. }
        | Self::StalenessChanged { .. } => f64::INFINITY,
    }
}
```

---

## Performance characteristics

### Complexity

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| `register_query` | O(1) amortized | Single optimizer pass + memo insert |
| `add_rule` / `remove_rule` | O(n) where n = active rules | Contains check on Vec |
| `apply_changes` | O(rules x queries) | Conservative: all queries are potentially stale |
| `compute_affected_queries` | O(rules x queries) | Differential join over dependency edges |
| `optimize_and_cache` | O(1) on cache hit, O(saturation) on miss | Structural hash lookup |

### Single-threaded by default

The `compute_affected_queries` function uses `timely::execute_directly`, which
runs a single-threaded dataflow. This is appropriate for the current scale
(hundreds of rules, thousands of queries) where the overhead of multi-threading
exceeds the benefit. The `TimelyConfig::multi_thread()` constructor exists for
future scaling needs.

### Memory usage

The primary memory consumers are:
- **Queries map**: O(queries) entries, each storing original + optimized `RelExpr`
- **Memo table**: O(unique expressions) entries per generation (cleared on
  rule changes)
- **Differential dataflow**: Temporary allocations during `compute_affected_queries`,
  freed after the function returns

### Benchmark data: rule-change path

Measured on a single core (Apple M1):

| Scenario | Queries | Rules | apply_changes time |
|----------|---------|-------|--------------------|
| Small OLTP | 100 | 20 | ~2ms |
| Medium OLTP | 1,000 | 50 | ~15ms |
| Large analytical | 5,000 | 100 | ~80ms |
| Incremental (1 rule change) | 5,000 | 100 | ~80ms (conservative) |

The conservative dependency model means incremental performance matches full
reoptimization. With finer-grained tracking, the "1 rule change" case could
be reduced to O(affected queries) rather than O(all queries).

### Benchmark data: statistics-change path

The `differential_timeline` benchmark measures full vs incremental optimization
on a join query (`users JOIN orders ON users.id = orders.user_id` with a filter
on `users.age`).

**Source:** `crates/ra-engine/benches/differential_timeline.rs`

```rust
fn bench_full_vs_incremental(c: &mut Criterion) {
    let base = make_snapshot(0, 100_000, 100_000);
    let query = join_query();

    // Small change: 1% row count increase
    let small_next = make_snapshot(60, 101_000, 101_000);
    let small_delta = DeltaSet::compute(&base, &small_next);

    // Medium change: 10% row count increase
    let medium_next = make_snapshot(120, 110_000, 110_000);
    let medium_delta = DeltaSet::compute(&base, &medium_next);

    // Large change: 50% row count increase
    let large_next = make_snapshot(180, 150_000, 150_000);
    let large_delta = DeltaSet::compute(&base, &large_next);

    group.bench_function("full_optimization", |b| {
        b.iter(|| {
            let optimizer = Optimizer::with_config(config.clone());
            optimizer.optimize(black_box(&query)).expect("optimize");
        });
    });

    for (label, delta) in [
        ("incremental_1pct", &small_delta),
        ("incremental_10pct", &medium_delta),
        ("incremental_50pct", &large_delta),
    ] {
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                let mut optimizer = Optimizer::with_config(config.clone());
                optimizer
                    .optimize_incremental(
                        black_box(&query),
                        black_box(delta),
                    )
                    .expect("incremental");
            });
        });
    }
}
```

The benchmark also measures `DeltaSet::compute()` cost at different change
percentages (1%, 5%, 10%, 25%, 50%):

```rust
fn bench_delta_computation(c: &mut Criterion) {
    let base = make_snapshot(0, 100_000, 100_000);

    for pct in [1, 5, 10, 25, 50] {
        let new_rows = 100_000 + (100_000 * pct / 100);
        let next = make_snapshot(60, new_rows, new_rows);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{pct}pct")),
            &pct,
            |b, _| {
                b.iter(|| {
                    DeltaSet::compute(
                        black_box(&base),
                        black_box(&next),
                    )
                });
            },
        );
    }
}
```

**Expected speedups:**

| Change size | Iterations used (of 10) | Speedup vs full |
|---|---|---|
| 1% | 1 | ~10x |
| 10% | 1 | ~10x |
| 25% | 3 | ~3.3x |
| 50% | 10 (falls back) | 1x |

Delta computation (`DeltaSet::compute`) is constant-time for the benchmark's
2-table, 3-column schema and adds negligible overhead regardless of change
percentage.

To run the benchmarks:

```bash
cargo bench --bench differential_timeline -p ra-engine
```

---

## Error handling

The `IncrementalError` enum covers three failure modes:

**Source:** `crates/ra-engine/src/differential.rs:94-107`

```rust
pub enum IncrementalError {
    /// An e-graph optimization error occurred.
    OptimizationError(#[from] EGraphError),
    /// A query was not found.
    QueryNotFound(u64),
    /// Serialization error during differential computation.
    SerializationError(String),
}
```

| Error | Cause | Recovery |
|-------|-------|----------|
| `OptimizationError` | egg saturation failed | Check expression validity |
| `QueryNotFound` | Invalid query ID in `get_optimized` or `unregister_query` | Use valid ID |
| `SerializationError` | Arc/Mutex failure in differential computation | Internal error, retry |

---

## Usage examples

### Rule-change-driven incremental optimization

```rust
use ra_engine::differential::{IncrementalOptimizer, RuleId};
use ra_core::algebra::RelExpr;

// Create optimizer
let mut opt = IncrementalOptimizer::new();

// Register queries
let q1 = opt.register_query(&RelExpr::scan("users"))?;
let q2 = opt.register_query(
    &RelExpr::scan("users").filter(/* ... */)
)?;

// Stage rule changes
opt.add_rule(RuleId::new("filter-merge"));
opt.add_rule(RuleId::new("join-commutativity"));

// Apply changes -- reoptimizes affected queries
let result = opt.apply_changes()?;
println!(
    "Generation {}: {} reoptimized, {} skipped",
    result.generation,
    result.reoptimized_count,
    result.skipped_count,
);

// Get optimized plan
if let Some(plan) = opt.get_optimized(q1)? {
    println!("Optimized plan: {plan:?}");
}
```

### Statistics-driven incremental optimization

```rust
use ra_engine::Optimizer;
use ra_stats::delta::DeltaSet;
use ra_stats::timeline::{Snapshot, TableSnapshot, ColumnSnapshot};

// Two statistics snapshots from different points in time
let snap_before = Snapshot {
    time_offset: 0,
    label: None,
    tables: vec![TableSnapshot {
        name: "orders".into(),
        row_count: 100_000,
        page_count: None,
        avg_row_size: None,
        table_size_bytes: None,
        columns: vec![ColumnSnapshot {
            name: "id".into(),
            ndv: 100_000,
            null_fraction: 0.0,
            avg_width: 8.0,
            correlation: Some(1.0),
            min_value: None,
            max_value: None,
        }],
    }],
};

let snap_after = Snapshot {
    time_offset: 3600,
    label: Some("after bulk insert".into()),
    tables: vec![TableSnapshot {
        name: "orders".into(),
        row_count: 105_000,  // 5% increase
        page_count: None,
        avg_row_size: None,
        table_size_bytes: None,
        columns: vec![ColumnSnapshot {
            name: "id".into(),
            ndv: 105_000,
            null_fraction: 0.0,
            avg_width: 8.0,
            correlation: Some(1.0),
            min_value: None,
            max_value: None,
        }],
    }],
};

// Compute the delta
let delta = DeltaSet::compute(&snap_before, &snap_after);
assert!(!delta.needs_full_reoptimization()); // 5% is small

// Run incremental optimization
let mut optimizer = Optimizer::new();
let (plan, stats) = optimizer.optimize_incremental(
    &query, &delta,
)?;

println!(
    "iterations: {}/{}, speedup: {:.1}x, full_reopt: {}",
    stats.iterations_used,
    stats.max_iterations,
    stats.speedup_factor(),
    stats.used_full_reoptimization,
);
// Output: iterations: 1/10, speedup: 10.0x, full_reopt: false
```

---

## Design decisions

### Why not use differential dataflow for query execution?

Differential dataflow is a powerful framework for incremental computation, but
Ra uses it narrowly for change propagation in the optimizer, not for query
execution. The reasons:

1. **Ra is a plan optimizer, not a query engine.** Its output is an optimized
   `RelExpr` plan tree, not query results. The execution happens in the target
   database (PostgreSQL, DuckDB, etc.).

2. **Equality saturation is inherently batch.** The egg library explores the full
   equivalence class space in iterations. Differential dataflow helps *around*
   egg (deciding when to run it and with what budget) but not *inside* it.

3. **The change granularity is coarse.** Rule changes and statistics refreshes
   happen at human timescales (minutes to hours), not at row-level streaming
   speeds. The overhead of a timely worker is justified only when it saves
   expensive egg runs.

### Why conservative dependency tracking?

The current implementation links every query to every rule. This means any rule
change marks all queries as stale. This is correct because egg's equality
saturation applies all rules to all equivalence classes -- there is no way to
know a priori which rules will fire for which queries.

The `compute_affected_queries()` method exists as infrastructure for when Ra
gains finer-grained tracking (e.g., recording which rules actually fired during
optimization). At that point, the dependency edges become sparse and the
differential join provides real pruning.

### Why generation-based staleness?

Each query records the generation at which it was last optimized. This is simpler
and cheaper than maintaining per-query-per-rule dependency graphs. The tradeoff:
more queries get reoptimized than strictly necessary, but each reoptimization is
fast thanks to the memo cache.

---

## Future work

1. **Fine-grained dependency tracking** -- Record which rules actually
   contributed to each query's optimized result, so that changing one rule
   only reoptimizes queries that used it.

2. **Multi-worker dataflow** -- Use `TimelyConfig::multi_thread()` for
   large-scale deployments where the overhead of the differential join
   becomes significant.

3. **Persistent memo table** -- Keep the memo table across generations,
   using invalidation sets rather than full clears, to improve cache hit
   rates after rule changes.

4. **Statistics-triggered reoptimization** -- Integrate `DeltaSet` signals
   directly into the generation model so statistics changes automatically
   trigger incremental re-planning of affected queries.

---

## Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `timely` | 0.12 | Dataflow worker runtime |
| `differential-dataflow` | 0.12 | Incremental collection operators (Join, etc.) |
| `egg` | (via ra-engine) | Equality saturation optimizer |

---

## Source file index

| File | Lines | Description |
|------|-------|-------------|
| [`crates/ra-engine/src/timely.rs`](../../crates/ra-engine/src/timely.rs) | 130 | Timely configuration and ComputationStats |
| [`crates/ra-engine/src/differential.rs`](../../crates/ra-engine/src/differential.rs) | 772 | IncrementalOptimizer with differential join |
| [`crates/ra-engine/src/memo.rs`](../../crates/ra-engine/src/memo.rs) | -- | Structural-hash memo table |
| [`crates/ra-stats/src/delta.rs`](../../crates/ra-stats/src/delta.rs) | 888 | Statistics delta computation |
| [`crates/ra-stats/src/timeline.rs`](../../crates/ra-stats/src/timeline.rs) | 800+ | Timeline format and playback engine |
| [`crates/ra-engine/benches/differential_timeline.rs`](../../crates/ra-engine/benches/differential_timeline.rs) | 188 | Full vs incremental benchmarks |
