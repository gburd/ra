# RFC 0046: Flamegraph Query Plan Visualization

- Start Date: 2026-03-22
- Author: RA Contributors
- Status: Draft
- Tracking Issue: TBD

## Summary

Add flamegraph-based visualization of query plan execution time to the
RA optimizer, using the `inferno` crate to render plan operator
hierarchies as interactive SVG flamegraphs. This allows users to see
at a glance which operators in a plan consume the most time, both in
the terminal (ra-tui) and in the browser (ra-web).

## Motivation

The RA optimizer produces detailed query plans, but understanding
where time is spent during execution is difficult from tree-formatted
text output alone. Large plans with dozens of operators require
extensive scrolling and mental arithmetic to identify bottlenecks.

Flamegraphs solve this by mapping hierarchical execution into a
visualization where:

- The **width** of each bar is proportional to cumulative execution
  time.
- The **depth** represents the operator call stack (parent-child plan
  nesting).
- **Self-time** is visible as the portion of a bar not covered by
  child operator bars above it.

This makes it immediately obvious which operators dominate execution
and whether the time is spent in the operator itself or in its
children.

**Use cases:**

1. **Query tuning:** Identify the most expensive operators in a plan
   to guide rewrite rule selection or index creation.
2. **Optimizer validation:** Verify that cost model estimates
   correlate with actual execution time.
3. **Regression detection:** Compare flamegraphs before and after
   optimizer changes to spot performance regressions.
4. **Education:** Teach how query plans execute and where time goes.

## Guide-level explanation

### Collecting timing data

Wrap plan execution with the `PlanProfiler` to collect per-operator
timing:

```rust
use ra_engine::profiler::PlanProfiler;

let profiler = PlanProfiler::new();
let result = profiler.execute(&plan, &context)?;
let profile = profiler.finish();

// profile.total_duration() -> Duration
// profile.operator_timings() -> &[OperatorTiming]
```

Each `OperatorTiming` records the operator name, its position in the
plan tree, wall-clock duration (inclusive of children), and self-time
(exclusive of children).

### Generating a flamegraph

Convert the profile to folded stacks and render with inferno:

```rust
use ra_engine::flamegraph::render_flamegraph;

// SVG output
let svg = render_flamegraph(&profile, &FlamegraphOptions::default())?;
std::fs::write("plan.svg", &svg)?;
```

The output is a self-contained SVG with interactive zoom and search
(provided by inferno's built-in JavaScript).

### CLI usage

```
$ ra explain --flamegraph query.sql > plan.svg
$ ra explain --flamegraph --format=folded query.sql > plan.folded
```

The `--format=folded` variant emits raw folded-stack lines, useful for
piping into other flamegraph tools or for diffing.

### TUI integration

In the ra-tui terminal interface, press `F` on a plan view to switch
to a simplified flamegraph rendering using Unicode block characters.
The TUI flamegraph uses color intensity to represent time proportion
rather than bar width, since terminal cells are coarse-grained.

```
 ┌─────────────── HashJoin (342ms) ───────────────┐
 │ ┌── SeqScan orders (198ms) ──┐┌ IdxScan (12ms)┐│
 │ │                             ││               ││
 │ └─────────────────────────────┘└───────────────┘│
 └─────────────────────────────────────────────────┘
```

Operator bars are drawn proportional to their inclusive time. Pressing
Enter on a bar zooms in; Escape zooms out.

### Web integration

The ra-web interface embeds the SVG flamegraph directly in the plan
visualization panel. The existing plan tree view gains a "Flamegraph"
tab that renders the interactive SVG. Because inferno produces
self-contained SVGs with embedded JavaScript, no additional frontend
dependencies are needed.

The web UI also supports differential flamegraphs: select two plan
profiles and view a diff flamegraph showing where time increased
(red) or decreased (blue).

## Reference-level explanation

### Architecture

```
                  ┌─────────────┐
                  │  PlanProfiler│
                  │  (ra-engine) │
                  └──────┬──────┘
                         │ OperatorTimings
                         ▼
                ┌────────────────┐
                │ FoldedStackEmitter│
                │   (ra-engine)    │
                └────────┬────────┘
                         │ folded stack lines
              ┌──────────┼──────────┐
              ▼          ▼          ▼
        ┌──────────┐ ┌────────┐ ┌────────┐
        │  inferno  │ │ ra-tui │ │ ra-web │
        │  (SVG)    │ │(block) │ │ (SVG)  │
        └──────────┘ └────────┘ └────────┘
```

### Implementation Details

#### `PlanProfiler`

A wrapper around plan execution that instruments each operator node.
It walks the `RelExpr` tree and, for each node, records
`Instant::now()` before and after executing the node's logic.

```rust
pub struct PlanProfiler {
    timings: Vec<OperatorTiming>,
}

pub struct OperatorTiming {
    /// Operator name (e.g., "HashJoin", "SeqScan(orders)").
    pub name: String,
    /// Depth in the plan tree (root = 0).
    pub depth: usize,
    /// Unique node ID within the plan.
    pub node_id: usize,
    /// Parent node ID (None for root).
    pub parent_id: Option<usize>,
    /// Wall-clock time including children.
    pub inclusive_time: Duration,
    /// Wall-clock time excluding children.
    pub self_time: Duration,
}

pub struct PlanProfile {
    pub timings: Vec<OperatorTiming>,
    pub total_duration: Duration,
}
```

The profiler assigns each `RelExpr` variant a human-readable name:

| `RelExpr` variant    | Flamegraph label          |
|----------------------|---------------------------|
| `Scan { table, .. }` | `SeqScan({table})`       |
| `IndexScan { .. }`   | `IndexScan({table}.{index})` |
| `Filter { .. }`      | `Filter`                 |
| `Join { join_type, .. }` | `{join_type}Join`    |
| `Aggregate { .. }`   | `Aggregate`              |
| `Sort { .. }`        | `Sort`                   |
| `HashJoin { .. }`    | `HashJoin`               |
| `Window { .. }`      | `Window`                 |
| (etc.)               | (variant name)           |

#### `FoldedStackEmitter`

Converts `PlanProfile` into the folded-stack text format that inferno
expects. Each line is a semicolon-separated stack from root to leaf,
followed by a space and a count (microseconds of self-time):

```
HashJoin;SeqScan(orders) 198000
HashJoin;IndexScan(customers.pk) 12000
HashJoin 132000
```

The emitter walks the timing tree depth-first. For each node with
nonzero self-time, it emits one line with the full ancestor path.

```rust
pub fn emit_folded(profile: &PlanProfile) -> String {
    // Build parent chain for each node, emit "root;...;leaf count\n"
}
```

#### SVG generation via inferno

The `render_flamegraph` function calls inferno's library API:

```rust
use inferno::flamegraph::{self, Options};

pub fn render_flamegraph(
    profile: &PlanProfile,
    opts: &FlamegraphOptions,
) -> Result<Vec<u8>> {
    let folded = emit_folded(profile);
    let mut fg_opts = Options::default();
    fg_opts.title = Some(opts.title.clone());
    fg_opts.count_name = "microseconds".to_string();
    fg_opts.colors = opts.color_scheme.into();
    fg_opts.min_width = 0.1; // show narrow operators

    let mut output = Vec::new();
    flamegraph::from_lines(
        &mut fg_opts,
        folded.lines(),
        &mut output,
    )?;
    Ok(output)
}
```

#### Differential flamegraphs

For regression detection, compare two profiles:

```rust
use inferno::differential::DiffOptions;

pub fn render_diff_flamegraph(
    baseline: &PlanProfile,
    current: &PlanProfile,
) -> Result<Vec<u8>> {
    let folded_base = emit_folded(baseline);
    let folded_curr = emit_folded(current);
    // Use inferno's diff-folded to produce differential output,
    // then render as flamegraph with differential coloring.
}
```

### Integration Points

**ra-engine:** The `PlanProfiler` and `FoldedStackEmitter` live here,
since this crate already owns plan execution and cost modeling. The
`inferno` dependency is added to ra-engine behind a `flamegraph`
feature flag to keep the default build lean.

**ra-tui:** Adds a flamegraph view widget using `ratatui` canvas. The
TUI does not depend on inferno; it reads `PlanProfile` directly and
renders using Unicode block drawing. Press `F` to toggle the view.

**ra-web:** Adds a `/api/flamegraph` endpoint that returns the SVG.
The frontend embeds it in an `<iframe>` or renders inline. The
differential endpoint is `/api/flamegraph/diff`.

**ra-cli:** Adds `--flamegraph` and `--flamegraph-diff` flags to the
`explain` subcommand. Output goes to stdout (SVG) or a file.

**ra-wasm:** The inferno crate compiles to WASM, so the web UI can
generate flamegraphs client-side without a server round trip.

### Error Handling

- If profiling is not enabled, `render_flamegraph` returns
  `Error::ProfilingNotEnabled` with a message suggesting the
  `--profile` flag.
- If a plan has zero operators (empty plan), return an empty SVG with
  a "No operators to display" message.
- If inferno's rendering fails (malformed folded data), propagate the
  error with context about which operator caused the issue.

### Performance Considerations

**Profiling overhead:** `Instant::now()` is called twice per operator
node. On modern hardware this adds ~20ns per node. A plan with 1000
operators adds ~40 microseconds of overhead, which is negligible
compared to actual execution time.

**SVG rendering:** Inferno generates SVGs in single-digit milliseconds
for typical plan sizes (< 100 operators). Plans with thousands of
operators may produce large SVGs (several MB), but this is rare in
practice.

**Memory:** The `PlanProfile` stores one `OperatorTiming` per node.
Each timing is ~120 bytes. A 1000-operator plan uses ~120KB.

**Feature flag:** The `inferno` dependency (~2MB compiled) is behind
`features = ["flamegraph"]` so it does not affect users who don't
need it. The TUI flamegraph view has zero additional dependencies.

## Drawbacks

- **CDDL license:** The inferno crate is CDDL-1.0 licensed, which is
  incompatible with GPL but compatible with MIT/Apache-2.0 (RA's
  license). However, CDDL requires attribution and source
  availability for modifications to inferno itself. This is acceptable
  since we use it as-is.

- **Execution dependency:** Flamegraphs require actual plan execution,
  not just plan generation. For optimizer-only use cases (no
  execution engine), this feature provides no value. However,
  estimated-cost flamegraphs (using the cost model instead of wall
  time) can serve as a fallback.

- **Terminal rendering fidelity:** The TUI flamegraph is an
  approximation. Terminal cells are too coarse for precise
  proportional widths. This is mitigated by showing exact
  millisecond values on hover/select.

- **Maintenance burden:** The folded-stack emitter must stay in sync
  with `RelExpr` variants. Adding a new operator variant requires
  updating the name mapping. This is a small, mechanical task.

## Rationale and alternatives

### Why This Design?

**Folded-stack intermediary.** The key insight from Tanel Poder's
P99 CONF 2023 talk is that SQL plan execution maps naturally to
folded stacks: each operator is a "function call" in a hierarchy, and
time attribution follows the same inclusive/self-time model as CPU
profiling. By converting to folded stacks, we reuse the entire
flamegraph ecosystem (inferno, speedscope, flamescope) without
building custom visualization.

**Inferno as library.** Inferno provides a well-tested Rust library
API for generating flamegraph SVGs. It compiles to WASM, handles
interactive zoom/search in the SVG output, and is ~20x faster than
the original Perl FlameGraph.pl. No other Rust crate offers this
combination.

**Feature flag isolation.** Making inferno optional keeps the core
build lean. Users who only need the optimizer (not execution
profiling) pay no compile-time or binary-size cost.

### Alternative Approaches

**Custom SVG renderer.** Building our own SVG flamegraph renderer
would avoid the CDDL dependency but duplicates significant work
(layout algorithm, interactive JavaScript, color schemes, text
fitting). Not worth it when inferno already exists.

**Brendan Gregg's FlameGraph.pl.** The original Perl implementation
is widely used but cannot be embedded as a library. It would require
shelling out to a Perl process, adding a runtime dependency.

**Speedscope JSON format.** Speedscope uses a different input format
(JSON profile) and is a web-only viewer. We could export to
speedscope format as a future extension, but it does not help with
TUI or CLI output.

**d3-flame-graph (JavaScript).** A pure-JS solution that would work
for ra-web but not for ra-tui or ra-cli. Using inferno gives us
server-side SVG generation that works everywhere.

### Impact of Not Doing This

Users would continue to rely on text-based `EXPLAIN ANALYZE` output
to understand execution time distribution. For large plans, this
means manually tracing operator hierarchies and summing durations.
The optimizer team would lack a visual tool for validating cost model
accuracy against real execution profiles.

## Prior art

### Academic Research

Brendan Gregg introduced flamegraphs in 2011 for CPU profiling.
The visualization has since been applied to memory allocation, I/O
latency, and off-CPU analysis. The key property exploited here is
that any hierarchical time attribution can be rendered as a
flamegraph.

### Industry Solutions

- **Oracle:** Tanel Poder demonstrated SQL plan flamegraphs at P99
  CONF 2023, converting `DBMS_XPLAN` timing output into folded
  stacks. His `sqlflame.sql` proof-of-concept showed the approach
  works well for identifying expensive operators in complex plans.

- **PostgreSQL:** The `auto_explain` extension logs plans with
  timing, but provides no flamegraph output. Third-party tools like
  pgMustard and explain.depesz.com offer tree-based visualization
  but not flamegraphs.

- **DuckDB:** Provides a profiling mode that outputs JSON timing
  data. No built-in flamegraph support, but the data can be
  converted externally.

- **MySQL:** `EXPLAIN ANALYZE` (since 8.0.18) shows actual timing
  per operator in a tree format. No flamegraph integration.

- **Apache Calcite:** No execution profiling (optimizer only), but
  the RelNode tree structure would map to flamegraphs similarly.

### What We Can Learn

Tanel Poder's key insight: "a typical RDBMS SQL plan execution is
just a bunch of function calls executed in the hierarchy and order
that's defined in the plan itself." This means the folded-stack
format is a natural fit. Wide bars indicate expensive operators;
bars without children above them indicate self-time-dominant
operators (the actual bottlenecks).

The Oracle proof-of-concept required recursive SQL to walk the plan
tree and compute self-time. In RA, we instrument execution directly,
so the data is cleaner and more accurate.

## Unresolved questions

### Before merging this RFC

- **Cost-model flamegraphs:** Should we also support flamegraphs
  based on estimated cost (not actual execution time)? This would
  work without an execution engine and help visualize what the
  optimizer *thinks* the time distribution will be. This is likely
  valuable but may warrant a separate RFC.

- **Icicle graphs:** Should we also support icicle graphs (inverted
  flamegraphs, growing downward)? Inferno supports these. They may
  be more intuitive for plan trees since plans are typically drawn
  top-down.

- **Time units:** Should the folded-stack count be microseconds,
  milliseconds, or nanoseconds? Microseconds provide good
  granularity without overflow risk for long-running queries.

### During implementation

- The exact ratatui canvas widget layout for the TUI flamegraph
  needs prototyping to determine the minimum useful terminal width.

- Whether the WASM build of inferno fits within acceptable bundle
  size limits for ra-wasm.

### Out of scope

- Flamegraphs for optimizer rule application time (which rules take
  longest). This is a separate profiling concern and would be a
  future RFC.
- Integration with external profiling tools (perf, DTrace) for
  system-level flamegraphs of the RA engine itself.

## Future possibilities

### Natural Extensions

- **Estimated-cost flamegraphs:** Render the cost model's predicted
  time distribution as a flamegraph, then overlay or diff with
  actual execution. This directly validates cost model accuracy.

- **Rule application flamegraphs:** Profile which optimizer rules
  consume the most transformation time, useful for optimizing the
  optimizer itself.

- **Speedscope export:** Emit profiles in speedscope's JSON format
  for users who prefer that viewer.

- **Flamegraph annotations:** Overlay cardinality estimates,
  row counts, and memory usage onto the flamegraph bars.

- **Regression flamegraph CI integration:** Automatically generate
  differential flamegraphs in CI when plan performance changes,
  linking to RFC 0013 (Query Regression Detection).

### Long-term Vision

Flamegraphs become the primary tool for understanding query plan
performance in RA. The combination of cost-model flamegraphs
(predicted) and execution flamegraphs (actual) creates a feedback
loop: where the two diverge, the cost model needs calibration
(linking to RFC 0026, Adaptive Cost Model Calibration). This makes
flamegraphs not just a visualization tool but a driver for optimizer
improvement.

## Implementation phases

### Phase 1: Core profiling and folded-stack emission

- Add `PlanProfiler` and `OperatorTiming` to ra-engine
- Implement `FoldedStackEmitter` to convert profiles to folded stacks
- Add `inferno` as optional dependency behind `flamegraph` feature
- Implement `render_flamegraph` SVG generation
- Unit tests for profiler accuracy and folded-stack output

### Phase 2: CLI integration

- Add `--flamegraph` flag to `ra explain`
- Add `--format=folded` for raw folded-stack output
- Add `--flamegraph-diff` for comparing two profiles

### Phase 3: TUI flamegraph widget

- Implement ratatui canvas-based flamegraph renderer
- Add keyboard navigation (zoom in/out, hover details)
- Wire `F` keybinding in plan view

### Phase 4: Web integration

- Add `/api/flamegraph` endpoint to ra-web
- Embed SVG in plan visualization panel
- Add differential flamegraph tab
- Test WASM client-side rendering as alternative

## References

- [inferno crate](https://github.com/jonhoo/inferno) -- Rust port
  of FlameGraph tools
- [Visualizing SQL Plan Execution Time with FlameGraphs](https://www.p99conf.io/2023/09/21/visualizing-sql-plan-execution-time-with-flamegraphs/)
  -- Tanel Poder, P99 CONF 2023
- [Brendan Gregg's Flame Graphs](https://www.brendangregg.com/flamegraphs.html)
  -- Original concept and methodology
- RFC 0013: Query Regression Detection -- differential flamegraphs
  for regression CI
- RFC 0026: Adaptive Cost Model Calibration -- cost vs. actual
  flamegraph comparison
