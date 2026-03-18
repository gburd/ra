# Resource Budgets

Resource budgets constrain how much time, memory, and computational effort the
optimizer spends on a given query. They provide predictable latency guarantees
for interactive applications, prevent runaway optimization on pathological
queries, and enable memory-safe operation in constrained environments.

## Motivation

Equality saturation explores exponentially many equivalent plans. For simple
queries this converges quickly, but complex joins and nested subqueries can
cause the e-graph to grow without bound. Resource budgets let operators choose
the right tradeoff between optimization quality and resource consumption.

Without budgets, the optimizer uses fixed limits from `OptimizerConfig`
(node limit, iteration limit, time limit) that apply uniformly. Resource
budgets add per-query control, overflow strategies, and detailed usage
reporting.

## Predefined Profiles

Five profiles cover common workloads. Each sets wall-clock time, CPU time,
memory, e-graph node count, and iteration limits with a sensible overflow
strategy.

### Interactive

```
Time:       100ms
Memory:     50 MB
E-graph:    10,000 nodes
Iterations: 10
Strategy:   return best-so-far
```

For latency-sensitive paths: autocomplete, query previews, IDE integrations.
The optimizer produces the best plan it can within 100ms and returns it even
if the e-graph has not saturated. Suitable for OLTP queries with 1-3 tables.

### Standard

```
Time:       1s
Memory:     500 MB
E-graph:    100,000 nodes
Iterations: 30
Strategy:   return best-so-far
```

The default for most workloads. Provides good optimization quality for
queries with up to 5-6 tables and moderate predicate complexity. Suitable
for application backends and batch ETL that process one query at a time.

### Batch

```
Time:       10s
Memory:     2 GB
E-graph:    1,000,000 nodes
Iterations: 100
Strategy:   return best-so-far
```

For analytical queries where optimization time is small relative to execution
time. TPC-H, TPC-DS, and star-schema data warehouse queries benefit from
the extended exploration. Use when a few seconds of planning saves minutes
of execution.

### Memory-Constrained

```
Time:       5s
Memory:     10 MB
E-graph:    5,000 nodes
Iterations: 15
Strategy:   return best-so-far
```

For serverless functions, embedded devices, or containerized environments
with strict memory limits. The tight e-graph cap prevents unbounded memory
growth. May sacrifice optimization quality on large join graphs.

### Unlimited

```
Time:       none
Memory:     none
E-graph:    none
Iterations: none
Strategy:   return best-so-far
```

Removes all budget constraints. The optimizer still respects the base
`OptimizerConfig` limits (node limit, iteration limit, time limit). Use
for testing or when external tooling enforces resource limits.

## Custom Budgets

Start from any profile and override individual fields:

```rust
use ra_engine::ResourceBudget;
use std::time::Duration;

let budget = ResourceBudget::standard()
    .with_time_limit(Duration::from_millis(500))
    .with_memory_limit(256 * 1024 * 1024)  // 256 MB
    .with_iteration_limit(20);
```

Or build from scratch:

```rust
let budget = ResourceBudget::unlimited()
    .with_time_limit(Duration::from_secs(2))
    .with_egraph_node_limit(50_000)
    .with_overflow_strategy(OverflowStrategy::Fail);
```

## CLI Usage

The `optimize` command accepts budget flags:

```bash
# Use a named profile
ra-cli optimize "SELECT ..." --resource-budget interactive

# Custom limits
ra-cli optimize "SELECT ..." --max-time 500ms --max-memory 256MB

# Combine profile with overrides
ra-cli optimize "SELECT ..." --resource-budget standard --max-iterations 5

# Set overflow strategy
ra-cli optimize "SELECT ..." --resource-budget batch --overflow-strategy fail
```

When a budget is active, the CLI prints a resource usage report:

```
Resource Usage:
  Status: complete
  Time: 42.3ms
  Iterations: 8
  Peak e-graph nodes: 3,421
```

With `--verbose`, the report also shows estimated peak memory and plan cost.

## Overflow Strategies

When a limit is exceeded, the overflow strategy determines the result.

### `ReturnBestSoFar` (default)

Returns the lowest-cost plan extracted during optimization, even if the
e-graph was not fully explored. This is the safest choice: the caller always
gets a valid plan, and incomplete optimization still improves on the original
in most cases.

### `ReturnOriginal`

Returns the unoptimized input plan. Use when you need a guaranteed-correct
baseline and prefer no optimization over partial optimization.

### `Fail`

Returns an error. Use in testing or CI pipelines where exceeding a budget
indicates a regression or misconfiguration.

## Resource Tracking

The `ResourceTracker` monitors usage during optimization:

- **Wall-clock time** -- Measured with `Instant::now()`.
- **CPU time** -- Approximated by wall-clock in the current implementation.
- **Memory** -- Estimated at ~64 bytes per e-graph node. This is a rough
  heuristic; actual memory depends on expression sizes and analysis data.
- **E-graph nodes** -- The total node count in the e-graph after each
  iteration.
- **Iterations** -- The number of equality saturation passes completed.

Budget checks happen *between* iterations, so a single iteration can
temporarily exceed the memory or time limit. The check granularity is
one iteration.

## `ResourceUsageReport`

After optimization, the report contains:

| Field                | Type               | Description                      |
|----------------------|--------------------|----------------------------------|
| `elapsed_time`       | `Duration`         | Total wall-clock time            |
| `iterations_used`    | `usize`            | Completed iterations             |
| `peak_egraph_nodes`  | `usize`            | Largest observed e-graph size    |
| `peak_memory_estimate` | `u64`            | Largest observed memory (bytes)  |
| `budget_exceeded`    | `Option<ExceededResource>` | Which limit was hit, if any |

The `completed_within_budget()` method returns `true` when no limit was
exceeded.

## Best Practices

1. **Start with a named profile.** Override only the fields that differ
   for your workload. This ensures all dimensions are constrained.

2. **Use `interactive` for user-facing paths.** Users notice latency above
   100ms. Interactive budgets keep the optimizer from dominating response
   time.

3. **Monitor `peak_memory_estimate`.** In memory-constrained environments,
   log this value and alert if it approaches the limit. Tighten the
   e-graph node limit if needed.

4. **Prefer `ReturnBestSoFar`.** It provides the best available plan even
   on timeout. Use `Fail` only in test suites to catch regressions.

5. **Profile under load.** Single-query benchmarks do not capture
   contention. Run benchmarks with concurrent queries to validate that
   budget limits prevent one query from starving others.

6. **Interpret incomplete plans carefully.** An incomplete optimization
   found a valid plan but may not have found the optimal one. If a query
   consistently hits the budget limit, consider the `batch` profile or
   increasing specific limits.
