# Optimizer Architecture

## Overview

Ra's query optimizer transforms relational algebra expressions into
efficient execution plans using **equality saturation** powered by the
[egg](https://egraphs-good.github.io/) library. Unlike traditional
optimizers that apply transformations sequentially and commit to each
choice, equality saturation explores the entire space of equivalent plans
simultaneously, then extracts the cheapest plan at the end.

The optimizer lives in the `ra-engine` crate and is the central
performance-critical component of the system. It takes a `RelExpr` tree
from `ra-core`, converts it to an e-graph representation, applies 170+
rewrite rules until convergence, then uses a hardware-aware cost model to
extract the best plan.

---

## Pipeline Overview

A query flows through five major stages:

```
                    Ra Optimizer Pipeline

  SQL Text
    |
    v
+-------------------+
|  1. Parsing       |  ra-parser: SQL/RRA --> RelExpr AST
+-------------------+
    |
    v
+-------------------+
|  2. Algebra       |  ra-core: RelExpr tree (Scan, Filter,
|                   |  Join, Aggregate, Sort, Limit, ...)
+-------------------+
    |
    v
+-------------------+
|  3. E-Graph       |  ra-engine: equality saturation
|     Optimization  |  (170+ rewrite rules, convergence
|                   |   detection, cost pruning, beam search)
+-------------------+
    |
    v
+-------------------+
|  4. Cost-Based    |  ra-engine + ra-hardware + ra-stats-advanced:
|     Extraction    |  hardware-aware, staleness-adjusted
|                   |  cost function selects cheapest plan
+-------------------+
    |
    v
+-------------------+
|  5. Execution     |  Optimized RelExpr ready for
|     Plan          |  code generation or interpretation
+-------------------+
```

### Stage 1: Parsing

The `ra-parser` crate converts SQL text (or `.rra` literate rule files) into
a `RelExpr` AST. Key modules:

- `lexer.rs` -- tokenization
- `parser.rs` -- recursive descent parser producing `RelExpr`
- `sql_to_relexpr.rs` -- SQL-specific translation layer
- `validator.rs` -- semantic validation

### Stage 2: Relational Algebra

The `ra-core::algebra` module defines the `RelExpr` enum
(`crates/ra-core/src/algebra.rs:19`), the canonical intermediate
representation:

```rust
pub enum RelExpr {
    Scan { table, alias },
    Filter { predicate, input },
    Project { columns, input },
    Join { join_type, condition, left, right },
    Aggregate { group_by, aggregates, input },
    Sort { keys, input },
    Limit { count, offset, input },
    Union { all, left, right },
    Intersect { all, left, right },
    Except { all, left, right },
    Window { functions, input },
    Distinct { input },
    RecursiveCTE { name, base_case, recursive_case, body, ... },
    CTE { name, definition, body },
    Values { rows },
    // Physical operators
    IndexScan { table, column },
    IndexOnlyScan { table, index, columns, predicate },
    MvScan { view_name, alias },
    BitmapIndexScan { table, index, predicate },
    BitmapAnd { inputs },
    BitmapOr { inputs },
    BitmapHeapScan { table, bitmap, recheck_cond },
    ParallelScan { table, workers },
    ParallelHashJoin { ... },
    ParallelAggregate { ... },
    Gather { input, workers },
    IncrementalSort { prefix_keys, suffix_keys, input },
    Unnest { expr, alias, input, with_ordinality },
    MultiUnnest { exprs, aliases, with_ordinality },
    TableFunction { name, args, columns, input },
    RowPattern { ... },
}
```

### Stage 3: E-Graph Optimization

The heart of the optimizer. Covered in detail in the sections below.

### Stage 4: Cost-Based Extraction

After equality saturation, the `extract` module uses `egg::Extractor` with
a cost function to select the cheapest equivalent expression from each
e-class. Three extraction strategies are available:

| Function | Cost Model | Use Case |
|----------|-----------|----------|
| `extract_best` | Hardware-aware `RelCostFn` | Default path |
| `extract_best_with_staleness` | `IntegratedCostFn` with staleness | Production with stale stats |
| `extract_best_with_cardinality` | `CardinalityAwareCostFn` with ML | Highest accuracy |

### Stage 5: Execution Plan

The extracted `RelExpr` is the optimized plan. Downstream consumers
(`ra-codegen` for JIT compilation, `ra-dialect` for SQL translation,
or direct interpretation) receive this tree.

---

## E-Graph Language Definition

The `RelLang` enum (`crates/ra-engine/src/egraph.rs:30`) defines the
S-expression language used inside the e-graph. It is declared using
egg's `define_language!` macro and maps every `RelExpr` variant and
scalar expression to a flat node type:

```
RelLang (S-expression language)
|
+-- Relational operators
|   scan, filter, project, join, aggregate, sort, limit,
|   union, intersect, except, recursive-cte, cte, window,
|   distinct-rel, values, metadata-lookup, index-scan,
|   index-only-scan, mv-scan, bitmap-index-scan, bitmap-and,
|   bitmap-or, bitmap-heap-scan
|
+-- Join types
|   inner, left-outer, right-outer, full-outer, cross, semi, anti
|
+-- Scalar expressions
|   col, qcol, const-null, const-bool, const-int, const-float,
|   const-str, add, sub, mul, div, mod, eq, ne, lt, le, gt, ge,
|   and, or, not, is-null, is-not-null, neg, concat, json-access
|
+-- Aggregate functions
|   count, sum, avg, min, max
|
+-- Structural nodes
|   list, nil, proj-col, proj-alias, sort-key, agg-expr,
|   window-expr, window-fn, window-frame, func, values-row
|
+-- Leaf symbols
    Symbol(egg::Symbol)  -- table names, column names, literals
```

The conversion between `RelExpr` and `RelLang` is handled by:

- **`to_rec_expr`** (`crates/ra-engine/src/egraph.rs`) -- `RelExpr` to
  `RecExpr<RelLang>` for insertion into the e-graph
- **`rec_expr_to_rel_expr`** (`crates/ra-engine/src/extract.rs:241`) --
  `RecExpr<RelLang>` back to `RelExpr` after extraction

---

## The Optimizer Struct

The `Optimizer` (`crates/ra-engine/src/egraph.rs:270`) is the main entry
point:

```rust
pub struct Optimizer {
    config: OptimizerConfig,
    table_stats: HashMap<String, Statistics>,
    hardware_profile: Option<HardwareProfile>,
    resource_budget: Option<ResourceBudget>,
    plan_cache: Option<Mutex<PlanCache>>,
}
```

### Configuration

`OptimizerConfig` (`crates/ra-engine/src/egraph.rs:172`) controls all
tunable parameters:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `node_limit` | 100,000 | Max e-graph nodes |
| `iter_limit` | 30 | Max iterations (fallback) |
| `time_limit_secs` | 10 | Wall-clock timeout |
| `large_join_threshold` | 10 | Tables to trigger heuristic fallback |
| `large_join_strategy` | SimulatedAnnealing | Strategy for 10+ table joins |
| `max_optimization_time_ms` | 30,000 | Hard timeout |
| `use_adaptive_limits` | true | Scale limits by query complexity |
| `use_cost_pruning` | true | Early termination on cost stagnation |
| `cost_pruning_threshold` | 1.5 | Prune plans >50% worse than best |
| `use_join_graph_filtering` | true | Prune invalid join combinations |
| `beam_search_config` | None | Optional beam search for complex queries |
| `transaction_context` | None | Isolation-aware cost penalties |
| `enable_plan_cache` | false | Fingerprint-based plan caching |

---

## Optimization Flow

The `Optimizer::optimize` method (`crates/ra-engine/src/egraph.rs:382`)
implements a multi-tier optimization strategy:

```
                         Optimizer::optimize(expr)
                                  |
                                  v
                    +---------------------------+
                    |  Plan Cache Lookup         |
                    |  (fingerprint match)       |
                    +---------------------------+
                         |              |
                       HIT            MISS
                         |              |
                    return cached        v
                                  +---------------------------+
                                  |  Left-Deep Fast Path      |
                                  |  (2-7 tables, simple)     |
                                  +---------------------------+
                                       |              |
                                    SUCCESS         FAIL
                                       |              |
                                  return plan          v
                                  +---------------------------+
                                  |  Large Join Heuristic     |
                                  |  (10+ tables)             |
                                  +---------------------------+
                                       |              |
                                  10+ tables      <10 tables
                                       |              |
                                  heuristic            v
                                  optimize    +---------------------------+
                                       |      |  E-Graph Equality         |
                                       |      |  Saturation               |
                                       |      |  (full optimization)      |
                                       |      +---------------------------+
                                       |              |
                                       v              v
                                  +---------------------------+
                                  |  Cost-Based Extraction    |
                                  +---------------------------+
                                            |
                                            v
                                  +---------------------------+
                                  |  Cache Insert (if enabled)|
                                  +---------------------------+
                                            |
                                            v
                                      Return RelExpr
```

### Tier 1: Plan Cache

When `enable_plan_cache` is true, the optimizer computes a
`QueryFingerprint` (`crates/ra-engine/src/genetic_fingerprint.rs:30`) and
checks the cache before doing any optimization work.

The fingerprint captures three dimensions:
- **Join graph hash** -- tables and join topology
- **Predicate pattern hash** -- operator structure without literal values
- **Aggregation signature hash** -- GROUP BY shape and aggregate functions

Queries that differ only in literal values (e.g., `WHERE id = 1` vs
`WHERE id = 42`) produce identical fingerprints and reuse cached plans.

The cache supports both exact and fuzzy matching (configurable similarity
threshold, default 0.9). See `crates/ra-engine/src/plan_cache.rs`.

### Tier 2: Left-Deep Fast Path

For queries with 2-7 tables and simple join patterns, the left-deep
builder (`crates/ra-engine/src/left_deep.rs`) constructs an optimized
join tree directly without e-graph overhead:

1. Peel outer operators (Aggregate, Sort, Project, Window)
2. Extract all scan nodes and join conditions
3. Sort tables by cardinality (smallest first)
4. Build left-deep tree: `((T1 JOIN T2) JOIN T3) JOIN T4`
5. Re-apply outer operators

This provides a 10-50x speedup over full equality saturation for common
OLTP-style queries.

### Tier 3: Large Join Heuristic

When table count exceeds `large_join_threshold` (default: 10), the
e-graph search space grows exponentially. The `LargeJoinOptimizer`
(`crates/ra-engine/src/large_join.rs`) provides two heuristic strategies:

| Strategy | Description |
|----------|-------------|
| `Greedy` | Greedy join ordering -- pick cheapest next join |
| `SimulatedAnnealing` | Stochastic search with cooling schedule |

Default: `SimulatedAnnealing` with `initial_temp=1000.0`,
`cooling_rate=0.95`, `max_iterations=10000`.

### Tier 4: Full E-Graph Optimization

The core optimization loop (`crates/ra-engine/src/egraph.rs:555-738`):

```
1. Classify query complexity
2. Compute adaptive iteration limit and timeout
3. Initialize e-graph with RecExpr
4. For each iteration:
   a. Run one egg iteration (apply all rules)
   b. Record convergence metrics
   c. Track cost improvement (if cost pruning enabled)
   d. Record beam search stats (if beam search enabled)
   e. Check convergence detector
   f. Check egg saturation
   g. Check timeout
5. Extract best plan using cost function
```

---

## Query Complexity Classification

The `QueryComplexity` enum (`crates/ra-engine/src/query_complexity.rs:10`)
classifies queries to set appropriate optimization budgets:

| Complexity | Tables | Iter Limit | Timeout |
|------------|--------|------------|---------|
| Trivial | 0-1 | 3 | 50ms |
| Simple | 2-4 | 5 | 200ms |
| Medium | 5-7 | 10 | 500ms |
| Complex | 8-9 | 15 | 2,000ms |
| VeryComplex | 10+ | 20 | 5,000ms |

Classification considers not just table count but also join count,
presence of subqueries, and outer join usage. A 3-table query with
subqueries may be upgraded from Simple to Medium.

---

## Rewrite Rules

The rewrite rule system is organized into categories, each in its own
function or module. Rules are collected by `all_rules_unsorted()`
(`crates/ra-engine/src/rewrite.rs:38`) and then sorted by priority.

### Rule Categories

```
Rewrite Rules (~170 total)
|
+-- Null Simplification (18 rules)
|   null_simplification.rs
|   AND/OR with NULL, comparison with NULL, filter NULL elimination
|
+-- Predicate Pushdown (9 rules)
|   rewrite.rs:94
|   Push filters through joins, projects, unions, intersects
|   Merge/split conjunctive filters
|
+-- Join Reordering (7 rules)
|   rewrite.rs:148
|   Commutativity, associativity, cartesian-to-join,
|   outer-to-inner conversion
|
+-- Projection Pushdown (1 rule)
|   rewrite.rs:191
|   Merge redundant projects
|
+-- Expression Simplification (~30 rules)
|   rewrite.rs:205
|   Boolean: AND/OR with true/false, double negation, De Morgan
|   Arithmetic: identity elements, zero multiplication
|   Commutativity: canonical ordering for all commutative ops
|
+-- Join Elimination (1 rule)
|   rewrite.rs:307
|   Cross join with single-row right side
|
+-- Aggregate Optimization (2 rules)
|   rewrite.rs:323
|   Push filter below aggregate, aggregate-over-aggregate
|
+-- Limit/Sort Optimization (3 rules)
|   rewrite.rs:343
|   Push limit through project, merge limits, sort elimination
|
+-- Set Operations (5 rules)
|   rewrite.rs:367
|   Union/intersect commutativity, self-identity, except-self
|
+-- Subquery Optimization (2 rules)
|   rewrite.rs:400
|   Semi/anti join + filter merge
|
+-- DuckDB-Inspired Rules (10 rules)
|   rewrite.rs:420
|   Project pushdown, filter through left join, sub-self,
|   comparison negation, limit through union, sort below aggregate
|
+-- SQLite-Inspired Rules (7 rules)
|   rewrite.rs:480
|   Range-to-equality, transitive closure, OR distribution,
|   eq-implies-not-null, constant propagation
|
+-- Runtime Filter Rules (3 rules)
|   rewrite.rs:533
|   Hash-to-semi bloom filter, push through project/filter
|
+-- Consensus Rules (DataFusion + Calcite)
|   consensus_rules.rs
|   Equijoin extraction, null join key filtering, empty set rules
|
+-- Join Transformations
|   join_transformations.rs
|   Outer-to-inner with comparison, outer-to-semi/anti
|
+-- Parquet Pushdown
|   parquet_pushdown.rs
|   Filter splitting for row group pruning
|
+-- Metadata Shortcuts
|   count_metadata.rs
|   COUNT(*) and COUNT(col) to metadata lookup
|
+-- Covering Index
|   covering_index.rs
|   Scan-to-index-only-scan when index covers all needed columns
|
+-- MIN/MAX Index
|   shortcuts/min_max_index.rs
|   MIN/MAX aggregate to index scan (B-tree first/last key)
|
+-- Materialized View Rewrite
|   mv_rewrite.rs
|   Aggregate queries to materialized view scans
```

### Rule Priority System

Rules are sorted by priority using RFC 0058's complexity-based
prioritization (`crates/ra-engine/src/rule_priority.rs`):

```
priority = expected_benefit / complexity_weight
```

Where:
- `expected_benefit` = midpoint of the rule's benefit range `[min, max]`
- `complexity_weight` = `O(1)=1, O(n)=2, O(n^2)=4, O(exp)=8`

Examples of priority ordering (highest first):

| Rule | Complexity | Benefit | Priority |
|------|-----------|---------|----------|
| `filter-true` | O(1) | [0.6, 1.0] | 0.80 |
| `cartesian-to-join` | O(1) | [0.7, 1.0] | 0.85 |
| `count-star-to-metadata` | O(1) | [0.7, 1.0] | 0.85 |
| `filter-through-join-left` | O(n) | [0.5, 0.9] | 0.35 |
| `join-associativity-left` | O(n^2) | [0.3, 0.9] | 0.15 |
| `add-commutative` | O(1) | [0.0, 0.1] | 0.05 |

High-benefit, low-complexity rules (constant folding, metadata shortcuts)
run first. Expensive join reordering rules run later. Canonicalization
rules (commutativity) have near-zero benefit scores since they only
normalize without directly improving cost.

---

## E-Graph Analysis

The `RelAnalysis` (`crates/ra-engine/src/analysis.rs:27`) implements
egg's `Analysis` trait to track metadata per e-class:

```rust
pub struct RelData {
    pub tables: HashSet<String>,    // Referenced table names
    pub is_relational: bool,        // Whether this is a relational op
    pub estimated_rows: Option<f64>, // Estimated cardinality
}
```

This analysis propagates through the e-graph:

- **Scan** nodes contribute their table name
- **Filter/Project/Sort** inherit tables from their input
- **Join/Union/Intersect/Except** merge tables from both children
- **Merge** unions table sets when e-classes are unified

The analysis data drives:
1. Cost estimation (table lookup for statistics)
2. Rewrite rule applicability checks
3. Debugging and visualization

---

## Cost Model Architecture

Ra uses a layered cost model with three levels of sophistication:

```
                    Cost Model Hierarchy

+-----------------------------------------------+
|  CardinalityAwareCostFn                        |
|  ML-based cardinality estimation               |
|  (ra-ml HeuristicEstimator)                    |
+-----------------------------------------------+
          |
          v
+-----------------------------------------------+
|  IntegratedCostFn                              |
|  Statistics + staleness + hardware             |
|  (ra-stats-advanced + ra-hardware)                      |
+-----------------------------------------------+
          |
          v
+-----------------------------------------------+
|  RelCostFn                                     |
|  Pure hardware-aware operator costs            |
|  (ra-hardware::HardwareProfile)                |
+-----------------------------------------------+
```

### RelCostFn (Base Layer)

Defined in `crates/ra-engine/src/extract.rs:26`. Assigns costs based on
the hardware profile:

| Operator | Cost Formula | Hardware Factor |
|----------|-------------|-----------------|
| Scan | `100 * (100 / storage_bandwidth_gbps)` | Storage bandwidth |
| Filter/Project | `1 * (256 / simd_width_bits)` | SIMD width |
| Join | `500 * (16 / l3_cache_mb)` | L3 cache size |
| Aggregate | `200 * (16 / l3_cache_mb)` | L3 cache size |
| Sort | `150 * (8 / cpu_cores)` | CPU core count |
| IncrementalSort | `60 * (8 / cpu_cores)` | CPU core count (40% of sort) |
| Window | `200 * (8 / cpu_cores)` | CPU core count |
| Distinct | `150 * (16 / l3_cache_mb)` | L3 cache size |
| Limit | 0.5 (fixed) | -- |
| MetadataLookup | 1.0 (fixed) | O(1) catalog lookup |
| IndexOnlyScan | `5 * storage_factor` | Much cheaper than full scan |
| MvScan | `15 * storage_factor` | Pre-computed data |
| RecursiveCTE | 1000 (fixed) | Expensive |

### IntegratedCostFn (Statistics Layer)

Defined in `crates/ra-engine/src/cost.rs:54`. Wraps `RelCostFn` with:

1. **Staleness inflation** -- stale statistics inflate row count estimates
   to bias toward robust plans:

   | Staleness | Inflation Factor |
   |-----------|-----------------|
   | Fresh | 1.0x |
   | SlightlyStale | 1.05x |
   | ModeratelyStale | 1.2x |
   | VeryStale | 1.5x |
   | Unknown | 2.0x |

2. **Confidence discount** -- low-confidence statistics widen cost ranges:
   `discount = 2.0 - confidence` (range: 1.0 to 2.0)

3. **Default row count** -- 1,000 rows when no statistics are available

### CardinalityAwareCostFn (ML Layer)

Defined in `crates/ra-engine/src/cardinality_cost.rs:37`. Extends the
integrated model with ML-based cardinality estimation from `ra-ml`:

1. Estimates output cardinality for each operator
2. Scales base operator cost by estimated result size
3. Uses `HeuristicEstimator` as the default ML backend

---

## Convergence Detection

The `ConvergenceDetector` (`crates/ra-engine/src/convergence.rs:11`)
monitors e-graph growth to terminate early when optimization is no longer
productive:

**Detection criteria** (checked over a sliding window of 3 iterations):
1. **Zero unions** -- no new equivalences for 3 consecutive iterations
2. **Low growth** -- node growth rate below 5% for 3 consecutive
   iterations

**Metrics tracked per iteration:**
- Number of new equivalences (unions)
- Total node count
- Total equivalence class count

This prevents the optimizer from burning cycles on iterations that only
expand the e-graph without finding better plans. Combined with cost
pruning (stagnation detection after 3 iterations of <1% cost
improvement), the optimizer terminates as early as possible.

---

## Search Space Management

For complex queries, the e-graph can grow exponentially. Ra uses several
techniques to manage the search space:

### Cost Pruning

The `CostPruner` (`crates/ra-engine/src/cost_pruning.rs`) tracks the
best cost found so far and terminates optimization when cost improvement
stagnates. The optimizer records cost at each iteration and terminates if
cost has not improved by more than 1% for 3 consecutive iterations.

### Beam Search

Optional beam search (`crates/ra-engine/src/beam_search.rs`) limits the
number of plans tracked at each iteration. After each iteration, only the
top-k plans (by cost) survive to the next round. This trades optimality
for speed on very complex queries.

### Join Graph Filtering

The `JoinGraph` (`crates/ra-engine/src/join_graph.rs`) builds a graph of
table-to-table join relationships and reports density statistics. For
sparse join graphs, many join orderings are invalid, and the optimizer can
skip them. The graph reports:
- Table count, edge count, density
- Estimated reduction factor (fraction of orderings that are valid)

### Resource Budget

The `ResourceBudget` (`crates/ra-engine/src/resource_budget.rs`) provides
hard limits on optimization resources:
- Maximum e-graph nodes
- Maximum optimization time
- Maximum memory usage

When a budget is exceeded, the overflow strategy determines behavior:
truncate (stop and extract best so far) or fail with an error.

---

## Memo Table

The `MemoTable` (`crates/ra-engine/src/memo.rs:17`) provides a simple
hash-map cache of previously-optimized expressions. It maps structural
hashes to optimized `RelExpr` results, avoiding redundant optimization of
repeated subqueries within the same session.

```rust
pub struct MemoTable {
    cache: HashMap<u64, RelExpr>,
}
```

Structural hashing (`structural_hash` in `memo.rs:62`) recursively hashes
the expression tree structure and leaf values. Two structurally identical
expressions produce the same hash.

---

## Plan Cache (RFC 0060)

The `PlanCache` (`crates/ra-engine/src/plan_cache.rs`) provides
cross-query plan reuse using genetic fingerprints. Unlike the memo table
(which caches within a session), the plan cache persists across queries.

### Fingerprinting

The `QueryFingerprint` (`crates/ra-engine/src/genetic_fingerprint.rs:30`)
captures three orthogonal dimensions of query structure:

```
QueryFingerprint
|
+-- join_graph_hash (u64)     -- tables + join topology
+-- predicate_hash (u64)      -- operator structure (no literals)
+-- aggregation_hash (u64)    -- GROUP BY shape + aggregate functions
+-- table_count (u16)         -- quick pre-filter
+-- join_count (u16)
+-- has_aggregation (bool)
+-- has_distinct (bool)
+-- has_limit (bool)
+-- has_sort (bool)
```

### Similarity Matching

Fingerprint similarity uses weighted comparison:

| Dimension | Weight |
|-----------|--------|
| Join graph match | 40% |
| Predicate pattern match | 30% |
| Aggregation signature match | 20% |
| Structural flags match | 10% |

A similarity score of 1.0 means identical fingerprints; the default
fuzzy match threshold is 0.9 (90% similarity).

### Cache Eviction

LRU eviction with configurable `max_entries` (default: 1024). Each entry
tracks `last_access` (monotonic counter) and `hit_count` for monitoring.

---

## Progressive Re-optimization

The progressive re-optimization system (RFC 0052,
`crates/ra-engine/src/progressive_reopt.rs`) monitors execution at
runtime and triggers re-optimization when actual cardinalities diverge
from estimates:

```
                 Progressive Re-optimization

  Execute initial plan
         |
         v
  Monitor stitch points:
  - JoinBuildComplete
  - AggregateInput
  - SortInput
  - SubqueryBoundary
         |
         v
  Compare actual vs estimated cardinality
         |
    divergence > 2x?
    /            \
  NO             YES
  |               |
  continue      spawn BackgroundReoptimizer
                  |
                  v
              Re-optimize with corrected stats
                  |
                  v
              If new plan saves >20% remaining cost:
                  atomic switch via plan stitching
```

Key thresholds:
- **Divergence threshold**: 2.0x (actual/estimated ratio)
- **Switch threshold**: 0.8 (new plan must save 20%+ of remaining cost)
- **State transfer costs**: copy=0.01/row, hash build=0.05/row,
  sort=0.1/row

---

## Specialized Optimizers

### Constraint Optimizer

`crates/ra-engine/src/constraint_optimizer.rs` -- optimizes queries with
constraint information (unique keys, foreign keys, NOT NULL) to enable
additional simplifications.

### Distributed Optimizer

`crates/ra-engine/src/distributed_optimizer.rs` -- plans data movement
and aggregation strategies for distributed query execution across cluster
nodes.

### Federated Optimizer

`crates/ra-engine/src/federated_optimizer.rs` -- optimizes queries
spanning multiple data sources with different capabilities, using
`FederatedCostModel` for cross-source cost estimation.

### Trigger Optimizer

`crates/ra-engine/src/trigger_optimizer.rs` -- analyzes DML cost with
trigger cascades and provides warnings about expensive trigger chains.

---

## Data Flow Diagram

```
+-------------+    +------------+    +-----------+
| ra-parser   |--->| ra-core    |--->| ra-engine |
| SQL/RRA     |    | RelExpr    |    | Optimizer |
+-------------+    +------------+    +-----------+
                        ^                 |
                        |                 | uses
                        |                 v
                   +----------+    +--------------+
                   | ra-stats-advanced |<---| Cost Models  |
                   | staleness|    | RelCostFn    |
                   | accuracy |    | Integrated   |
                   +----------+    | Cardinality  |
                        ^          +--------------+
                        |                 |
                        |                 | uses
                        |                 v
                   +-----------+   +--------------+
                   | ra-ml     |   | ra-hardware  |
                   | cardinality|  | HardwareProfile|
                   | estimator |   | CPU/cache/SIMD|
                   +-----------+   +--------------+
```

### Crate Dependencies

| Crate | Role | Used By |
|-------|------|---------|
| `ra-core` | `RelExpr`, `Expr`, `Statistics`, `CostModel` trait | Everything |
| `ra-engine` | Optimizer, e-graph, rewrite rules, cost functions | CLI, Web, WASM |
| `ra-hardware` | `HardwareProfile`, `detect_hardware()`, `HardwareCostModel` | ra-engine |
| `ra-stats-advanced` | Staleness tracking, confidence scoring, delta computation | ra-engine |
| `ra-ml` | `CardinalityEstimator`, `HeuristicEstimator` | ra-engine |
| `ra-parser` | SQL parsing, RRA parsing, validation | CLI, Web |
| `ra-compiler` | Rule indexing, dependency analysis | CLI |

---

## Design Decisions and Trade-offs

### Why Equality Saturation?

Traditional optimizers (Volcano/Cascades) apply transformations one at a
time and must decide at each step whether a transformation improves the
plan. This leads to the "phase ordering problem" -- the order in which
transformations are applied affects the final result, and finding the
optimal order is itself an optimization problem.

Equality saturation sidesteps this by exploring all transformations
simultaneously. The e-graph compactly represents the exponential space of
equivalent expressions, and cost-based extraction at the end selects the
global optimum (within the explored space).

**Trade-off**: equality saturation has higher worst-case memory usage than
a traditional optimizer. Ra mitigates this with:
- Adaptive iteration limits based on query complexity
- Convergence detection for early termination
- Cost pruning to stop when improvement stagnates
- Left-deep fast path to bypass e-graph entirely for simple queries
- Large join heuristics to avoid exponential blowup

### Why Multiple Optimization Tiers?

A single optimization strategy cannot handle the full range of query
complexities:

| Query Type | Best Strategy | Reason |
|------------|---------------|--------|
| 1 table, simple filter | Trivial (3 iters) | E-graph overhead wasted |
| 2-7 table join | Left-deep builder | Direct construction is 10-50x faster |
| 3-9 table complex | Full e-graph | Equality saturation finds non-obvious plans |
| 10+ table join | Simulated annealing | E-graph search space is intractable |
| Repeated query | Plan cache | Avoid re-optimization entirely |

### Why Hardware-Aware Costing?

The same logical plan can have dramatically different costs on different
hardware. A hash join that fits in L3 cache is fast; one that spills to
disk is slow. Ra's cost model accounts for:

- **Storage bandwidth** -- affects scan costs
- **SIMD width** -- affects filter/project per-row costs
- **L3 cache size** -- affects hash join and aggregate costs
- **CPU core count** -- affects sort and window function parallelism

Hardware auto-detection (`ra_hardware::detect_hardware()`) provides
defaults, but profiles can be set explicitly for cross-compilation or
capacity planning.

### Why Staleness-Aware Statistics?

Stale statistics cause cardinality mis-estimation, which causes bad plans.
Rather than treating all statistics equally, Ra inflates cost estimates
proportionally to staleness. This biases the optimizer toward plans that
are robust to cardinality errors (e.g., hash joins over nested loops)
when statistics are stale. Fresh statistics enable more aggressive
optimization.

### Rule Priority Sorting

Applying high-benefit, low-cost rules first means the e-graph reaches
good plans faster. This matters because:
1. Earlier iterations discover more improvements per node added
2. Convergence detection can terminate sooner when good plans arrive early
3. Cost pruning has a better baseline to prune against

---

## Performance Characteristics

### Optimization Time by Complexity

| Complexity | Typical Time | E-Graph Nodes | Iterations |
|------------|-------------|---------------|------------|
| Trivial | <1ms | <100 | 1-3 |
| Simple | 1-10ms | 100-1,000 | 3-5 |
| Medium | 10-100ms | 1,000-10,000 | 5-10 |
| Complex | 100ms-2s | 10,000-50,000 | 10-15 |
| VeryComplex | 1-5s | 50,000-100,000 | 15-20 |

### Memory Usage

E-graph memory is proportional to node count:
- ~100 bytes per node (RelLang variant + metadata)
- ~50 bytes per equivalence class
- Analysis data (table sets) adds ~20 bytes per class

For a 100,000 node e-graph: approximately 10-15 MB.

### Plan Cache Performance

- Fingerprint computation: <1ms for typical queries
- Cache lookup (exact): O(1) hash map lookup
- Cache lookup (fuzzy): O(n) scan with similarity comparison
- Cache hit rate: depends on workload repetitiveness; OLTP workloads
  typically achieve 60-90% hit rates

---

## Key Source Files

| File | Lines | Purpose |
|------|-------|---------|
| `egraph.rs` | ~900 | RelLang definition, Optimizer struct, optimize() |
| `rewrite.rs` | ~660 | 170+ rewrite rules organized by category |
| `analysis.rs` | ~170 | E-graph analysis tracking tables and cardinality |
| `extract.rs` | ~1175 | Plan extraction and RecExpr-to-RelExpr conversion |
| `cost.rs` | ~600 | IntegratedCostFn with staleness and hardware |
| `cardinality_cost.rs` | ~200 | ML-based cardinality-aware cost function |
| `rule_priority.rs` | ~460 | RFC 0058 priority annotations and sorting |
| `convergence.rs` | ~150 | Early convergence detection |
| `query_complexity.rs` | ~200 | Adaptive complexity classification |
| `plan_cache.rs` | ~300 | Fingerprint-based plan caching (RFC 0060) |
| `genetic_fingerprint.rs` | ~300 | Query fingerprint computation |
| `left_deep.rs` | ~250 | Left-deep join tree fast path |
| `large_join.rs` | ~300 | Greedy and simulated annealing for large joins |
| `progressive_reopt.rs` | ~500 | Runtime re-optimization (RFC 0052) |
| `memo.rs` | ~340 | Structural hash memo table |
| `beam_search.rs` | ~200 | Beam search for search space management |
| `cost_pruning.rs` | ~200 | Cost-based early termination |
| `join_graph.rs` | ~200 | Join graph density analysis |
| `resource_budget.rs` | ~250 | Hard resource limits |

All paths are relative to `crates/ra-engine/src/`.
