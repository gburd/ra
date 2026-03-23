# RFC 0058: OpenTracing Instrumentation for Query Planner

- Start Date: 2026-03-23
- Author: Ra Optimizer Team
- Status: Draft
- Tracking Issue: TBD

## Summary

Add distributed tracing to the Ra query optimizer using the OpenTracing standard, enabling operators to observe planner performance in production, identify slow optimization stages, and correlate query planning with execution across distributed systems. Instrumentation is gated behind a Cargo feature flag (`opentracing`) so there is zero cost when disabled.

## Motivation

The Ra optimizer performs multiple phases of work -- expression conversion, rule application, convergence detection, cost pruning, plan extraction -- and today the only observability comes from `tracing` log lines and wall-clock timings printed at `info`/`debug` level. This is insufficient for production use:

1. **Performance diagnosis**: When a query takes too long to optimize, operators need to know *which* phase is slow. Is it the e-graph saturation loop? A single expensive rule? The cost model? Structured spans with timing and metadata answer these questions without log-grepping.

2. **Distributed correlation**: When Ra runs as a PostgreSQL extension (via pgrx) or inside a federated optimizer, query planning is one stage of a larger pipeline. OpenTracing span context propagation lets operators see the full picture -- from SQL parse through optimization through execution -- in a single trace.

3. **Rule profiling**: With 50+ rewrite rules, understanding which rules fire, how often, and how much time they consume is essential for tuning the rule set. Per-rule spans make this data available without custom benchmarking.

4. **Regression detection**: Tracing data exported to a backend (Jaeger, Zipkin, Datadog) can be queried programmatically. Teams can set alerts on p99 optimization latency or detect when a new rule increases cost extraction time.

5. **Production debugging**: When a query produces an unexpected plan, trace logs attached to spans provide a structured timeline of optimizer decisions, far more useful than grep-ing a flat log file.

### Non-goals

- This RFC does not propose tracing *query execution*. Execution tracing is a separate concern.
- This RFC does not mandate a specific tracing backend. Any OpenTracing-compatible collector works.

## Guide-level explanation

### Enabling tracing

Add the `opentracing` feature to your `ra-engine` dependency:

```toml
[dependencies]
ra-engine = { version = "0.1", features = ["opentracing"] }
```

When the feature is disabled (the default), all tracing instrumentation compiles away entirely -- no runtime cost, no extra dependencies.

### Configuring a tracer backend

The optimizer accepts a tracer instance through the `OptimizerConfig`:

```rust
use opentracingrust::{Tracer, tracers::NoopTracer};
use ra_engine::{Optimizer, OptimizerConfig};

// Production: use a Jaeger/Zipkin tracer
let tracer = create_jaeger_tracer("ra-optimizer", "localhost:6831")?;

let config = OptimizerConfig {
    tracer: Some(Arc::new(tracer)),
    ..Default::default()
};

let optimizer = Optimizer::with_config(config);
let optimized = optimizer.optimize(&plan)?;
```

When no tracer is configured (or the feature is disabled), the optimizer behaves identically to today.

### Reading traces

After optimization, spans appear in your tracing backend with this hierarchy:

```
optimizer.optimize (root)
  |-- optimizer.classify_complexity
  |-- optimizer.convert_to_egraph
  |-- optimizer.saturation_loop
  |     |-- optimizer.iteration[0]
  |     |     |-- optimizer.rule_apply ("predicate_pushdown")
  |     |     |-- optimizer.rule_apply ("join_commutativity")
  |     |     |-- optimizer.convergence_check
  |     |     `-- optimizer.cost_prune
  |     |-- optimizer.iteration[1]
  |     |     `-- ...
  |     `-- optimizer.iteration[N]
  |-- optimizer.extract_best
  |     |-- optimizer.cost_calculation
  |     `-- optimizer.stats_lookup
  `-- optimizer.convert_from_egraph
```

Each span carries structured tags:

| Tag | Type | Example | Description |
|-----|------|---------|-------------|
| `query.table_count` | i64 | `5` | Number of tables in the query |
| `query.complexity` | string | `"Medium"` | Complexity classification |
| `optimizer.iter_limit` | i64 | `12` | Configured iteration limit |
| `optimizer.timeout_ms` | i64 | `5000` | Configured timeout |
| `optimizer.iterations` | i64 | `8` | Actual iterations run |
| `optimizer.termination` | string | `"converged"` | Why saturation stopped |
| `optimizer.egraph_nodes` | i64 | `1247` | Final e-graph size |
| `optimizer.egraph_classes` | i64 | `312` | Final equivalence classes |
| `rule.name` | string | `"predicate_pushdown"` | Rule being applied |
| `rule.matches` | i64 | `3` | Number of matches found |
| `cost.best` | f64 | `142.7` | Best extracted cost |
| `cost.improvement_pct` | f64 | `23.5` | Improvement over input |

### Example: diagnosing slow optimization

```rust
// Query with 8 tables -- optimizer is slow, why?
let plan = build_8_way_join();
let optimized = optimizer.optimize(&plan)?;

// In Jaeger UI, the trace shows:
//   optimizer.optimize          total: 4.2s
//     optimizer.saturation_loop total: 3.9s   <-- bottleneck
//       optimizer.iteration[6]  total: 1.1s
//         rule_apply("join_associativity") 0.9s  <-- this rule
//
// Fix: increase large_join_threshold to bypass e-graph for 8+ tables
```

## Reference-level explanation

### Implementation Details

#### Span creation macro

To avoid boilerplate and ensure zero-cost when disabled, a conditional macro wraps span creation:

```rust
#[cfg(feature = "opentracing")]
macro_rules! ot_span {
    ($tracer:expr, $name:expr) => {{
        $tracer.as_ref().map(|t| {
            t.span($name, opentracingrust::StartOptions::default())
        })
    }};
    ($tracer:expr, $name:expr, child_of: $parent:expr) => {{
        $tracer.as_ref().map(|t| {
            let opts = opentracingrust::StartOptions::default()
                .child_of($parent.context().clone());
            t.span($name, opts)
        })
    }};
}

#[cfg(not(feature = "opentracing"))]
macro_rules! ot_span {
    ($tracer:expr, $name:expr) => { None::<()> };
    ($tracer:expr, $name:expr, child_of: $parent:expr) => {
        None::<()>
    };
}
```

A companion macro handles finishing spans and setting tags:

```rust
#[cfg(feature = "opentracing")]
macro_rules! ot_finish {
    ($span:expr $(, $key:expr => $val:expr)*) => {{
        if let Some(ref span) = $span {
            $(span.tag($key, $val.into());)*
            span.finish();
        }
    }};
}

#[cfg(not(feature = "opentracing"))]
macro_rules! ot_finish {
    ($span:expr $(, $key:expr => $val:expr)*) => {};
}
```

#### Instrumented optimize function

The core `Optimizer::optimize` method gains span creation around each phase:

```rust
pub fn optimize(&self, expr: &RelExpr) -> Result<RelExpr, EGraphError> {
    let total_start = Instant::now();
    let root_span = ot_span!(self.config.tracer, "optimizer.optimize");

    // Phase 1: Complexity classification
    let classify_span = ot_span!(
        self.config.tracer, "optimizer.classify_complexity",
        child_of: root_span
    );
    let table_count = LargeJoinOptimizer::count_tables(expr);
    let complexity = QueryComplexity::from_expr(expr);
    ot_finish!(classify_span,
        "query.table_count" => table_count,
        "query.complexity" => format!("{complexity:?}")
    );

    // Phase 2: Convert to e-graph
    let convert_span = ot_span!(
        self.config.tracer, "optimizer.convert_to_egraph",
        child_of: root_span
    );
    let rec_expr = to_rec_expr(expr)?;
    ot_finish!(convert_span);

    // Phase 3: Saturation loop
    let loop_span = ot_span!(
        self.config.tracer, "optimizer.saturation_loop",
        child_of: root_span
    );
    // ... iteration spans created inside the loop ...
    ot_finish!(loop_span,
        "optimizer.iterations" => actual_iterations,
        "optimizer.termination" => termination_reason
    );

    // Phase 4: Extract best plan
    let extract_span = ot_span!(
        self.config.tracer, "optimizer.extract_best",
        child_of: root_span
    );
    let result = extract_best(&egraph, root, &stats_cache, &hw);
    ot_finish!(extract_span,
        "cost.best" => best_cost
    );

    ot_finish!(root_span,
        "optimizer.egraph_nodes" => egraph.total_size(),
        "optimizer.egraph_classes" => egraph.number_of_classes()
    );

    result
}
```

#### Per-rule instrumentation

Inside the saturation loop, each rule application can optionally be traced individually. Because rule-level tracing adds overhead proportional to the number of rules times the number of iterations, it is controlled by a separate configuration flag:

```rust
pub struct OptimizerConfig {
    /// OpenTracing tracer instance (requires `opentracing` feature).
    #[cfg(feature = "opentracing")]
    pub tracer: Option<Arc<dyn opentracingrust::Tracer>>,

    /// Trace individual rule applications (high overhead).
    #[cfg(feature = "opentracing")]
    pub trace_rules: bool,
}
```

When `trace_rules` is enabled, the iteration loop creates child spans per rule:

```rust
for (rule_idx, rule) in rules.iter().enumerate() {
    let rule_span = if self.config.trace_rules {
        ot_span!(
            self.config.tracer, "optimizer.rule_apply",
            child_of: iter_span
        )
    } else {
        None
    };

    let matches = rule.search(&egraph);
    let applied = rule.apply(&mut egraph, &matches);

    if self.config.trace_rules {
        ot_finish!(rule_span,
            "rule.name" => rule.name(),
            "rule.matches" => matches.len(),
            "rule.applied" => applied
        );
    }
}
```

### Integration Points

#### Existing `tracing` crate compatibility

The `tracing` crate (used throughout Ra for structured logging) and OpenTracing serve different purposes. `tracing` provides in-process structured logging; OpenTracing provides cross-process distributed tracing. They coexist without conflict:

- `tracing` spans remain for debug/info logging at the Rust level.
- OpenTracing spans propagate context across service boundaries.
- A `tracing` subscriber that bridges to OpenTracing (via `tracing-opentracing`) can unify both if desired, but this is not required.

#### pgrx PostgreSQL extension

When Ra runs as a PostgreSQL extension, the tracer can be initialized during `_PG_init()` and stored in a `static`. The PostgreSQL query hook injects a parent span context so that optimizer spans appear as children of the overall query trace:

```rust
#[pg_guard]
pub extern "C" fn _PG_init() {
    // Initialize tracer from GUC settings
    let tracer = init_tracer_from_gucs();
    GLOBAL_TRACER.set(tracer).ok();
}

fn plan_hook(query: &str, cursor_options: i32) -> PlannedStmt {
    let parent_ctx = extract_span_context_from_pg();
    let span = GLOBAL_TRACER.get()
        .map(|t| t.span("pg.ra_optimize",
            StartOptions::default().child_of(parent_ctx)));
    // ... run optimizer ...
}
```

#### Federated optimizer

The `FederatedOptimizer` already coordinates across multiple data sources. Span context can be propagated to remote sources via HTTP headers (the standard OpenTracing mechanism), allowing a single trace to cover planning decisions across all federated nodes.

### Error Handling

Tracing must never cause optimization to fail. All tracer interactions are wrapped in `Option` -- if the tracer is `None` (unconfigured or feature disabled), no work is done. If a tracer method returns an error (e.g., backend unreachable), the error is logged via `tracing::warn!` and the span is dropped. The optimizer continues normally.

```rust
// Tracer errors never propagate to the caller
let span = match &self.config.tracer {
    Some(t) => match t.span("optimizer.optimize", opts) {
        Ok(s) => Some(s),
        Err(e) => {
            warn!("Failed to create tracing span: {e}");
            None
        }
    },
    None => None,
};
```

### Performance Considerations

**Zero-cost when disabled**: The `opentracing` feature flag gates all tracing code behind `#[cfg(feature = "opentracing")]`. When the feature is off, the macros expand to no-ops and the compiler eliminates them entirely. No extra dependencies are compiled.

**Overhead when enabled (tracer configured)**:

| Operation | Overhead | Notes |
|-----------|----------|-------|
| Root span creation | ~200ns | One per optimize call |
| Phase spans (4-5) | ~1us total | Fixed count per call |
| Iteration spans | ~200ns each | Proportional to iterations |
| Rule spans (if enabled) | ~10us/iter | 50 rules x 200ns each |
| Tag setting | ~50ns each | String allocation |
| Span finishing | ~100ns each | Sends to collector |

For a typical 8-iteration optimization with phase-level tracing only (rule tracing disabled), the overhead is approximately 3-5 microseconds -- negligible compared to the millisecond-scale optimization itself. With per-rule tracing enabled, overhead rises to approximately 80-100 microseconds per optimization, still under 1% of typical optimization time.

**Overhead when enabled (no tracer configured)**: When the feature is compiled but no tracer is set, all `ot_span!` macros hit the `None` branch immediately. Overhead is a single pointer check (~1ns) per span site.

**Sampling**: Production deployments should use sampling to reduce overhead further. The tracer backend controls sampling -- for example, Jaeger supports probabilistic sampling (e.g., 1% of queries) and rate-limited sampling (e.g., 2 traces/second). The optimizer does not need to implement sampling logic.

## Drawbacks

- **Dependency weight**: The `opentracingrust` crate and its transitive dependencies add to compile time when the feature is enabled. Mitigated by making it an optional feature.

- **API stability**: The `opentracingrust` crate follows the OpenTracing specification, which has been superseded by OpenTelemetry. The crate is maintained but not under active feature development. See "Future possibilities" for migration path.

- **Code complexity**: Span creation macros and conditional compilation add visual noise to the optimizer's hot path. The macro approach minimizes this, but reviewers must understand the dual-compilation model.

- **Maintenance burden**: Each new optimization phase or rule must remember to add tracing spans. Forgetting a span is not a correctness bug but degrades observability. This can be mitigated with code review checklists.

- **Tag cardinality**: Unbounded tag values (e.g., query text) can cause storage problems in tracing backends. The implementation must avoid high-cardinality tags -- use query fingerprints, not raw SQL.

## Rationale and alternatives

### Why This Design?

**Feature-flag gating** ensures zero cost for users who do not need distributed tracing, which is the majority of users today. This is the standard Rust approach for optional observability (used by `tokio`, `hyper`, `tonic`).

**OpenTracing over custom instrumentation** because it provides a vendor-neutral API with wide backend support. Users choose their preferred collector without changing optimizer code.

**Macro-based span creation** over manual `if cfg!(...)` blocks because macros produce cleaner code and the compiler can verify that both branches type-check.

**Optional per-rule tracing** because the overhead of 50+ rule spans per iteration is measurable, and most users only need phase-level visibility.

### Alternative Approaches

**OpenTelemetry directly**: OpenTelemetry is the successor to OpenTracing and offers a richer API (metrics + tracing + logs). However, the Rust OpenTelemetry SDK is larger and more complex. Starting with OpenTracing provides a simpler integration path with a clear migration to OpenTelemetry later (the APIs are similar and bridge libraries exist).

**`tracing` crate only**: The `tracing` crate could provide span-like output via subscribers. However, `tracing` is designed for in-process observability and does not natively support cross-process context propagation, which is essential for distributed tracing in federated scenarios.

**Custom metrics emission**: Emitting timing metrics (e.g., via Prometheus) instead of traces. Metrics show aggregates but not individual query traces. Traces and metrics are complementary -- this RFC focuses on traces; metrics can be added separately.

**Compile-time tracing with `#[instrument]`**: The `tracing` crate's `#[instrument]` attribute provides automatic span creation. While convenient, it does not support OpenTracing context propagation and adds overhead even when no subscriber is configured (span creation still allocates).

### Impact of Not Doing This

Without structured tracing, diagnosing optimizer performance in production requires:
- Adding ad-hoc `Instant::now()` timing and log statements
- Correlating log lines across services manually
- Rebuilding with extra debug output for each investigation

This is time-consuming and error-prone, especially in distributed deployments where the optimizer runs as a PostgreSQL extension or federated query coordinator.

## Prior art

### Academic Research

- **"Monitoring and Diagnosing Query Execution in Database Management Systems"** (Oracle, 2012): Describes structured tracing of query execution stages, noting that phase-level granularity provides the best cost-benefit ratio for production systems.

- **"Adaptive Tracing for Database Workload Analysis"** (Microsoft Research, 2018): Proposes adaptive sampling strategies that reduce tracing overhead to under 1% while maintaining statistical significance.

### Industry Solutions

- **PostgreSQL**: `pg_stat_statements` tracks query statistics (calls, total time, rows) but provides no per-query trace or phase breakdown. `auto_explain` logs query plans but not optimizer internals. `EXPLAIN (ANALYZE, BUFFERS)` provides execution detail but only for a single query.

- **CockroachDB**: Built-in distributed tracing using OpenTelemetry. Every SQL statement produces a trace with spans for parsing, planning, and execution. This is the closest model to what this RFC proposes and validates the approach.

- **MySQL**: Performance Schema provides stage-level instrumentation of query processing. Stages include "optimizing", "statistics", and "preparing", roughly analogous to the phases this RFC instruments.

- **DuckDB**: Profiling output (`PRAGMA enable_profiling`) shows per-operator timing in a tree format. Focuses on execution rather than optimization, but demonstrates the value of structured phase timing.

- **Apache Calcite**: No built-in tracing, but the volcano planner fires events (`RelOptListener`) that can be hooked for external tracing. This event-based approach is similar to our macro-based approach in spirit.

### What We Can Learn

CockroachDB's experience shows that always-on tracing with sampling is practical in production databases. Their implementation uses OpenTelemetry with a 1% default sampling rate, which aligns with our approach of making tracing optional and relying on backend sampling.

MySQL's Performance Schema demonstrates that fixed-granularity stage instrumentation (rather than dynamic per-operator tracing) provides the best trade-off between overhead and usefulness.

## Unresolved questions

- **Span naming convention**: Should spans use dot-separated names (`optimizer.saturation_loop`) or slash-separated (`optimizer/saturation_loop`)? Dot-separated is more common in the OpenTracing ecosystem, but slash-separated aligns with OpenTelemetry conventions. This should be decided before implementation.

- **Baggage items**: OpenTracing supports "baggage" -- key-value pairs that propagate across all spans in a trace. Should we propagate query fingerprints or session IDs as baggage? This adds overhead but improves correlation.

- **Span log events**: OpenTracing spans can carry log events (timestamped messages). Should we attach the convergence detector's per-iteration stats as log events on the saturation span? This is useful for debugging but increases trace size.

- **Tracer lifecycle in pgrx**: PostgreSQL extensions have complex lifecycle management. The tracer must survive across queries within a session but be cleanable on session end. The exact initialization and teardown points need prototyping.

- **`opentracingrust` vs `rustracing`**: Both crates implement the OpenTracing API in Rust. `opentracingrust` is more complete but `rustracing` has more recent activity. The choice should be validated against our API needs.

## Future possibilities

### Natural Extensions

**OpenTelemetry migration**: OpenTelemetry is the industry standard going forward. Once the Rust OpenTelemetry SDK stabilizes, migrating from OpenTracing is straightforward -- the span creation API is nearly identical, and bridge libraries (`opentelemetry-opentracing-bridge`) allow gradual migration.

**Metrics integration**: OpenTelemetry unifies traces and metrics. After migration, the same instrumentation points can emit histogram metrics (e.g., `optimizer_saturation_duration_seconds`) alongside traces, enabling dashboards and alerting without separate instrumentation.

**Automatic rule profiling**: With per-rule tracing data collected over time, build a rule profiling report that identifies which rules provide the most cost improvement per time spent. This can feed back into rule ordering and rule set selection.

**Query fingerprint correlation**: Tag spans with query fingerprints (normalized query shapes) to aggregate tracing data across similar queries. This enables "this query shape always takes >100ms to optimize" alerts.

**Adaptive rule ordering**: Use tracing data to dynamically reorder rules so that high-impact, low-cost rules run first. This is a natural evolution of rule profiling.

### Long-term Vision

Distributed tracing is one pillar of production observability alongside metrics and logging. This RFC establishes the tracing pillar. Combined with the existing `tracing` crate logging and future metrics work, Ra will have a complete observability stack suitable for production deployment in enterprise environments.
