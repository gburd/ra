# E-Graph Equality Saturation in Ra

This document describes how Ra uses e-graphs and equality saturation for
query optimization. It covers the theoretical foundations, the integration
with the [egg](https://egraphs-good.github.io/) library, the rewrite rule
system, cost-based plan extraction, and the adaptive strategies that keep
optimization time bounded.

## Table of Contents

- [Overview](#overview)
- [Equality Saturation Fundamentals](#equality-saturation-fundamentals)
- [The RelLang S-Expression Language](#the-rellang-s-expression-language)
- [Conversion Pipeline](#conversion-pipeline)
- [Rewrite Rules](#rewrite-rules)
- [Rule Priority System (RFC 0058)](#rule-priority-system-rfc-0058)
- [E-Graph Analysis](#e-graph-analysis)
- [Plan Extraction and Cost Minimization](#plan-extraction-and-cost-minimization)
- [Saturation Strategies and Termination](#saturation-strategies-and-termination)
- [Optimizer Architecture](#optimizer-architecture)
- [Performance Characteristics](#performance-characteristics)
- [Worked Examples](#worked-examples)
- [Key Source Files](#key-source-files)

---

## Overview

Traditional query optimizers apply transformation rules in a fixed order.
Each rule produces one new plan, discarding the original. This means the
order of rule application matters: applying rule A before rule B can
produce a different result than B before A, and the optimizer may miss
globally optimal combinations.

E-graph equality saturation solves this by exploring all rule applications
simultaneously. Instead of committing to one transformation at a time, the
e-graph stores all equivalent forms of an expression compactly, then
extracts the lowest-cost form after the search space is fully explored.

Ra implements this approach in `crates/ra-engine/src/egraph.rs` using the
egg (e-graphs good) library. The optimizer:

1. Converts a `RelExpr` AST into an e-graph `RecExpr` (S-expression form)
2. Runs equality saturation with ~170 rewrite rules
3. Extracts the cheapest equivalent plan using a hardware-aware cost model

```
                     +-----------+
   RelExpr -------->| to_rec_expr|------> RecExpr (S-expression)
                     +-----------+
                          |
                          v
                    +-------------+
                    |  egg Runner |  <--- rewrite rules (~170)
                    | (saturation)|       applied in priority order
                    +-------------+
                          |
                          v
                    +-------------+
                    | extract_best|  <--- cost function
                    | (extraction)|       (hardware + statistics)
                    +-------------+
                          |
                          v
                    +-----------+
                    | rec_expr_ |------> optimized RelExpr
                    | to_rel    |
                    +-----------+
```

## Equality Saturation Fundamentals

### E-Graphs

An e-graph (equivalence graph) is a data structure that compactly
represents many equivalent expressions. It has two key components:

- **E-nodes**: individual operations (scan, filter, join, etc.)
- **E-classes**: sets of e-nodes that are provably equivalent

When a rewrite rule discovers that expression A is equivalent to
expression B, the e-graph merges their e-classes rather than replacing
one with the other. Both forms remain available.

```
  Before rule application:        After merging equivalence:

  e-class 1: {join(A, B)}         e-class 1: {join(A, B), join(B, A)}
  e-class 2: {join(B, A)}                     ^-- commutativity
```

### Saturation

Equality saturation applies all rewrite rules to all e-classes repeatedly
until one of these conditions holds:

1. **Saturation**: no rule produces any new equivalences (fixed point)
2. **Node limit**: the e-graph exceeds a configured size
3. **Iteration limit**: the configured number of passes is reached
4. **Timeout**: wall-clock time exceeds the budget

At that point, a cost function selects the cheapest expression from each
e-class to assemble the final plan.

### Why E-Graphs for Query Optimization

Traditional optimizers face a phase-ordering problem: the best sequence
of transformations depends on the query, and trying all orderings is
exponential. E-graphs avoid this by:

- Representing all rewritten forms simultaneously (no information loss)
- Sharing common subexpressions across equivalence classes (compact)
- Deferring the cost decision until the full search space is explored

The downside is that the e-graph can grow large for complex queries.
Ra addresses this with adaptive iteration limits, convergence detection,
cost-based pruning, and beam search (described later).

---

## The RelLang S-Expression Language

Ra defines `RelLang`, an S-expression language that maps every relational
and scalar operator to an e-graph node type. The language is defined using
egg's `define_language!` macro at `crates/ra-engine/src/egraph.rs:30`.

### Relational Operators

```
"scan"            = Scan([Id; 1])          -- table scan
"scan-alias"      = ScanAlias([Id; 2])     -- aliased table scan
"filter"          = Filter([Id; 2])        -- predicate, input
"project"         = Project([Id; 2])       -- column list, input
"join"            = Join([Id; 4])          -- type, condition, left, right
"aggregate"       = Aggregate([Id; 3])     -- groups, aggs, input
"sort"            = Sort([Id; 2])          -- keys, input
"limit"           = Limit([Id; 3])         -- count, offset, input
"union"           = Union([Id; 3])         -- all?, left, right
"intersect"       = Intersect([Id; 3])     -- all?, left, right
"except"          = Except([Id; 3])        -- all?, left, right
"window"          = Window([Id; 2])        -- functions, input
"distinct-rel"    = DistinctRel([Id; 1])   -- input
"recursive-cte"   = RecursiveCTE([Id; 4])  -- name, base, recursive, body
"cte"             = CTE([Id; 3])           -- name, definition, body
```

### Optimization-Specific Operators

```
"metadata-lookup" = MetadataLookup([Id; 2])    -- O(1) count from metadata
"index-scan"      = IndexScan([Id; 2])         -- MIN/MAX via B-tree
"index-only-scan" = IndexOnlyScan([Id; 4])     -- covering index scan
"mv-scan"         = MvScan([Id; 4])            -- materialized view scan
"bitmap-index-scan" = BitmapIndexScan([Id; 3]) -- bitmap index access
```

These operators do not exist in the input `RelExpr`; they are introduced
by rewrite rules during saturation (e.g., `count-star-to-metadata`
rewrites a `COUNT(*)` aggregate into a `metadata-lookup`).

### Scalar Expressions

```
"col"         = Col([Id; 1])         -- unqualified column reference
"qcol"        = QCol([Id; 2])        -- qualified (table.column)
"const-null"  = ConstNull            -- NULL literal
"const-bool"  = ConstBool([Id; 1])   -- boolean literal
"const-int"   = ConstInt([Id; 1])    -- integer literal
"const-float" = ConstFloat([Id; 1])  -- floating-point literal
"const-str"   = ConstStr([Id; 1])    -- string literal
"add"         = Add([Id; 2])         -- arithmetic +
"eq"          = Eq([Id; 2])          -- equality comparison
"and"         = And([Id; 2])         -- boolean AND
"not"         = Not([Id; 1])         -- boolean NOT
"is-null"     = IsNull([Id; 1])      -- NULL test
... (full list at egraph.rs:104-167)
```

### Join Types and Flags

```
"inner"       = Inner         "true"  = True
"left-outer"  = LeftOuter     "false" = False
"right-outer" = RightOuter
"full-outer"  = FullOuter
"cross"       = Cross
"semi"        = Semi
"anti"        = Anti
```

### Example: SQL to S-Expression

```sql
SELECT name FROM users WHERE age > 18
```

The `RelExpr` AST:

```
Project { columns: [name],
  Filter { predicate: (age > 18),
    Scan { table: "users" }
  }
}
```

The S-expression in the e-graph:

```
(project (list (proj-col (col age_sym)))
  (filter (gt (col age_sym) (const-int 18_sym))
    (scan users_sym)))
```

Where `age_sym`, `18_sym`, and `users_sym` are `Symbol` leaf nodes.

---

## Conversion Pipeline

### RelExpr to RecExpr (`to_rec_expr`)

The function `to_rec_expr` at `crates/ra-engine/src/egraph.rs:1336`
recursively walks a `RelExpr` tree and builds an egg `RecExpr`:

```rust
pub fn to_rec_expr(expr: &RelExpr) -> Result<RecExpr<RelLang>, EGraphError> {
    let mut rec = RecExpr::default();
    add_rel_expr(&mut rec, expr)?;
    Ok(rec)
}
```

The internal `add_rel_expr` function at line 1342 dispatches on every
`RelExpr` variant, calling helper functions like `add_scalar_expr`,
`add_join_type`, `add_projection_list`, etc. Each helper appends nodes
to the `RecExpr` and returns an `Id` that parent nodes reference.

### RecExpr to RelExpr (`rec_expr_to_rel_expr`)

After extraction, `rec_expr_to_rel_expr` at
`crates/ra-engine/src/extract.rs:241` converts the optimized `RecExpr`
back into a `RelExpr`. It starts from the last node (the root) and
recursively converts each node type:

```rust
pub fn rec_expr_to_rel_expr(rec: &RecExpr<RelLang>) -> Result<RelExpr, EGraphError> {
    let nodes = rec.as_ref();
    if nodes.is_empty() {
        return Err(EGraphError::ExtractionError("empty RecExpr".into()));
    }
    convert_node(nodes, nodes.len() - 1)
}
```

The `convert_node` function at line 249 handles every `RelLang` variant,
including optimization-specific operators like `MetadataLookup` (converted
to an equivalent `Aggregate` for downstream execution) and `IndexOnlyScan`
(converted to a dedicated `RelExpr::IndexOnlyScan`).

---

## Rewrite Rules

Rewrite rules are the core of the optimization search. Each rule is a
pattern match + transformation, written using egg's `rewrite!` macro.
All rules are defined in `crates/ra-engine/src/rewrite.rs`.

### Rule Categories

The rules are organized into functional categories:

| Category | Count | Examples |
|---|---|---|
| Null simplification | ~18 | `and-null-left`, `filter-null-elimination` |
| Predicate pushdown | ~11 | `filter-through-join-left`, `filter-merge` |
| Join reordering | ~7 | `join-commutativity`, `join-associativity-left` |
| Projection pushdown | ~2 | `project-merge` |
| Expression simplification | ~30 | `and-true-left`, `double-negation`, `add-zero-right` |
| Join elimination | ~1 | `cross-join-single-row-right` |
| Aggregate optimization | ~2 | `filter-below-aggregate`, `aggregate-over-aggregate` |
| Limit/Sort optimization | ~5 | `limit-through-project`, `sort-below-sort` |
| Set operations | ~5 | `union-commutativity`, `except-self` |
| Subquery optimization | ~2 | `filter-semi-join-merge` |
| DuckDB-inspired | ~10 | `duckdb-sub-self`, `duckdb-sort-below-aggregate` |
| SQLite-inspired | ~7 | `sqlite-range-to-eq`, `sqlite-const-prop-join` |
| Runtime filters | ~3 | `runtime-filter-hash-to-semi` |
| Consensus (DataFusion+Calcite) | ~11 | `extract-equijoin-from-and-left`, `empty-filter` |
| Join transformations | ~4 | `outer-to-semi-exists`, `outer-to-anti-not-exists` |
| Parquet pushdown | ~1 | `parquet-filter-split-for-pushdown` |
| Metadata shortcuts | ~2 | `count-star-to-metadata`, `count-col-to-metadata` |
| Covering index | ~1 | `scan-to-index-only-scan` |
| MIN/MAX index | ~2 | `min-to-index-scan`, `max-to-index-scan` |
| MV rewrite | ~1 | `mv-agg-rewrite` |

### Rule Anatomy

Each rule has a name, a left-hand side (pattern to match), and a
right-hand side (replacement). Egg variables (prefixed with `?`) match
any subtree:

```rust
// Push filter below inner join (left side)
// crates/ra-engine/src/rewrite.rs:97
rewrite!("filter-through-join-left";
    "(filter ?pred (join inner ?cond ?left ?right))" =>
    "(join inner ?cond (filter ?pred ?left) ?right)"
)
```

This rule matches any `filter` whose input is an inner `join`, and adds
an equivalent form where the filter is pushed to the join's left input.
The original form remains in the e-graph; both coexist.

### Bidirectional Rules

Some rewrites are inherently bidirectional. For example, filter splitting
and merging:

```rust
// crates/ra-engine/src/rewrite.rs:113
rewrite!("filter-merge";
    "(filter ?p1 (filter ?p2 ?input))" =>
    "(filter (and ?p1 ?p2) ?input)"
)

// crates/ra-engine/src/rewrite.rs:117
rewrite!("filter-split-and";
    "(filter (and ?p1 ?p2) ?input)" =>
    "(filter ?p1 (filter ?p2 ?input))"
)
```

Both forms exist simultaneously in the e-graph. The cost function
determines which form ends up in the final plan.

### Database-Inspired Rules

Some rules are sourced from the optimization strategies of established
databases:

**DuckDB-inspired** (from `src/optimizer/` in DuckDB):
```rust
// crates/ra-engine/src/rewrite.rs:468
rewrite!("duckdb-sort-below-aggregate";
    "(aggregate ?g ?a (sort ?k ?input))" =>
    "(aggregate ?g ?a ?input)"
)
```

Removes unnecessary sorts below aggregates, since aggregation destroys
input ordering.

**SQLite-inspired** (from `src/where.c` in SQLite):
```rust
// crates/ra-engine/src/rewrite.rs:484
rewrite!("sqlite-range-to-eq";
    "(and (ge ?a ?b) (le ?a ?b))" =>
    "(eq ?a ?b)"
)
```

Collapses `a >= b AND a <= b` into `a = b`, enabling index lookups.

### Rule Collection

The `all_rules()` function at `crates/ra-engine/src/rewrite.rs:30`
collects all rules from every category and sorts them by priority:

```rust
pub fn all_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    crate::rule_priority::sort_rules_by_priority(all_rules_unsorted())
}
```

The unsorted variant `all_rules_unsorted()` is available for benchmarking
the impact of priority sorting.

---

## Rule Priority System (RFC 0058)

Not all rules are equally valuable. Pushing a filter below a join can
reduce intermediate result sizes by orders of magnitude, while swapping
the order of addition operands is purely cosmetic. Applying cheap,
high-benefit rules first lets the e-graph discover good plans sooner,
which in turn enables earlier termination via convergence detection.

The priority system is implemented in
`crates/ra-engine/src/rule_priority.rs`.

### Priority Formula

Each rule is assigned a complexity class and a benefit range:

```
priority = expected_benefit / complexity_weight
```

Where:
- `expected_benefit` = midpoint of `(min_benefit, max_benefit)`
- `complexity_weight` = numeric weight of the complexity class

### Complexity Classes

Defined in `crates/ra-engine/src/rule_metadata.rs:22`:

| Class | Weight | Description |
|---|---|---|
| `O(1)` | 1.0 | Constant-time pattern match and rewrite |
| `O(n)` | 2.0 | Linear in matched nodes |
| `O(n^2)` | 4.0 | Quadratic (e.g., join reordering) |
| `O(exp)` | 8.0 | Exponential (e.g., full enumeration) |

### Benefit Range

A `BenefitRange` at `rule_metadata.rs:70` is a `(min, max)` pair on
a 0.0-to-1.0 scale:

- 0.0 = no improvement expected
- 1.0 = order-of-magnitude improvement possible

The expected benefit is the midpoint: `(min + max) / 2`.

### Example Priority Scores

```
filter-true:               O(1), benefit [0.6, 1.0] -> score = 0.8 / 1.0 = 0.80
filter-through-join-left:  O(n), benefit [0.5, 0.9] -> score = 0.7 / 2.0 = 0.35
join-associativity-left:   O(n^2), benefit [0.3, 0.9] -> score = 0.6 / 4.0 = 0.15
add-commutative:           O(1), benefit [0.0, 0.1] -> score = 0.05 / 1.0 = 0.05
```

Rules are sorted by score descending, with ties broken by original
insertion order. This means:

1. **Constant-folding and identity removal** fire first (highest score)
2. **Predicate and projection pushdown** fire next
3. **Join reordering** (quadratic) fires later
4. **Commutativity/canonicalization** fires last (lowest benefit)

### Sorting Implementation

The `sort_rules_by_priority` function at `rule_priority.rs:302`:

```rust
pub fn sort_rules_by_priority(
    rules: Vec<Rewrite<RelLang, RelAnalysis>>,
) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let priorities = default_rule_priorities();
    let mut scored: Vec<(f64, usize, Rewrite<...>)> = rules
        .into_iter()
        .enumerate()
        .map(|(idx, rule)| {
            let name = rule.name.as_str();
            let score = if let Some(&(complexity, benefit)) =
                priorities.get(name)
            {
                compute_priority(complexity, benefit)
            } else {
                compute_priority(DEFAULT_COMPLEXITY, DEFAULT_BENEFIT)
            };
            (score, idx, rule)
        })
        .collect();

    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    scored.into_iter().map(|(_, _, rule)| rule).collect()
}
```

Rules without explicit annotations receive a default middle-of-the-road
score (`O(n)` complexity, `[0.2, 0.5]` benefit range), so they are
neither first nor last.

### Priority Annotation Map

The `default_rule_priorities()` function at `rule_priority.rs:66`
maintains a `HashMap` mapping ~120 rule names to their
`(ComplexityClass, BenefitRange)` tuples. This is the central registry
where new rules should be annotated.

---

## E-Graph Analysis

During equality saturation, egg tracks per-e-class metadata via the
`RelAnalysis` type at `crates/ra-engine/src/analysis.rs:27`.

### Tracked Metadata

```rust
pub struct RelData {
    pub tables: HashSet<String>,     // table names referenced
    pub is_relational: bool,         // relational vs scalar node
    pub estimated_rows: Option<f64>, // cardinality estimate
}
```

### How Analysis Propagates

The `make` function at `analysis.rs:33` is called whenever a new e-node
is added. It extracts table names from `Scan` nodes and propagates them
upward through `Filter`, `Join`, `Aggregate`, etc.:

```
Scan("orders")     -> tables = {"orders"}
Scan("customers")  -> tables = {"customers"}
Join(_, _, left, right)
    -> tables = {"orders", "customers"}
Filter(_, join)
    -> tables = {"orders", "customers"}
```

The `merge` function at `analysis.rs:77` combines metadata when two
e-classes are discovered to be equivalent. It takes the union of table
sets and preserves the first available cardinality estimate.

### Use in Rewrite Rules

Analysis data enables conditional rules and cost estimation. For example,
the cost function can look up which tables an e-class references and use
their statistics for cardinality estimation.

---

## Plan Extraction and Cost Minimization

After saturation, the e-graph contains many equivalent plans per e-class.
The extractor selects the cheapest by traversing bottom-up, choosing the
lowest-cost e-node from each e-class.

### Cost Functions

Ra provides three extraction strategies, all in
`crates/ra-engine/src/extract.rs`:

#### 1. Hardware-Aware Cost (`RelCostFn`)

The base cost function at `extract.rs:26` assigns costs based on
hardware characteristics:

```rust
impl egg::CostFunction<RelLang> for RelCostFn {
    type Cost = f64;

    fn cost<C>(&mut self, enode: &RelLang, mut costs: C) -> Self::Cost
    where C: FnMut(Id) -> Self::Cost
    {
        let base_cost = match enode {
            RelLang::Scan(_) => {
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(*table_id) + (100.0 * storage_factor);
            }
            RelLang::Join(_) => {
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb;
                500.0 * cache_factor
            }
            RelLang::Sort(_) => {
                let parallelism = 8.0 / f64::from(self.hardware.cpu_cores);
                150.0 * parallelism.max(0.5)
            }
            RelLang::MetadataLookup(_) => return 1.0,  // O(1) shortcut
            RelLang::IndexOnlyScan(_) => {
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return 5.0 * storage_factor;  // much cheaper than full scan
            }
            ...
        };
        base_cost + children_cost
    }
}
```

Key cost relationships:
- `MetadataLookup` (cost ~1) << `IndexOnlyScan` (cost ~5) << `Scan` (cost ~100)
- `Join` (cost ~500) is the most expensive operator
- SIMD width reduces per-row filter/project cost
- L3 cache size reduces join hash-table cost
- CPU core count reduces sort cost (parallel sort)

#### 2. Statistics-Integrated Cost (`IntegratedCostFn`)

Defined in `crates/ra-engine/src/cost.rs`, this combines hardware
awareness with table statistics and staleness adjustments:

- Looks up `Statistics::row_count` per table
- Applies a staleness inflation factor (fresh=1.0, unknown=2.0)
- Biases toward robust plans when statistics are uncertain

#### 3. Cardinality-Aware Cost (`CardinalityAwareCostFn`)

Defined in `crates/ra-engine/src/cardinality_cost.rs`, this scales
operator costs by estimated intermediate result sizes using ML-based
cardinality estimation.

### Extraction Dispatch

The `extract_best` function at `extract.rs:147` chooses between basic
and statistics-integrated extraction:

```rust
pub fn extract_best(egraph, root, table_stats, hardware) -> Result<RelExpr, _> {
    if table_stats.is_empty() {
        // Use hardware-only cost function
        let cost_fn = RelCostFn::new(hardware.clone());
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        rec_expr_to_rel_expr(&best_expr)
    } else {
        // Use statistics + hardware cost function
        let cost_fn = IntegratedCostFn::new(hardware, stats, staleness);
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        rec_expr_to_rel_expr(&best_expr)
    }
}
```

---

## Saturation Strategies and Termination

Ra uses multiple strategies to keep optimization time bounded while
still finding good plans.

### Adaptive Iteration Limits

Query complexity determines the iteration budget. The classification
is done at `crates/ra-engine/src/query_complexity.rs:25`:

| Complexity | Tables | Iter Limit | Timeout |
|---|---|---|---|
| Trivial | 0-1 | 3 | 50ms |
| Simple | 2-4 | 5 | 200ms |
| Medium | 5-7 | 10 | 500ms |
| Complex | 8-9 | 15 | 2000ms |
| VeryComplex | 10+ | 20 | 5000ms |

The classification considers not just table count but also join count,
presence of subqueries, and outer joins. A 3-table query with correlated
subqueries may be classified as `Medium` rather than `Simple`.

### Convergence Detection

The `ConvergenceDetector` at `crates/ra-engine/src/convergence.rs:11`
monitors e-graph growth across iterations and triggers early termination
when progress stalls:

```
Iteration 1: 500 nodes, 120 classes, 80 unions   -> Continue
Iteration 2: 800 nodes, 180 classes, 40 unions   -> Continue
Iteration 3: 820 nodes, 182 classes,  2 unions   -> Continue
Iteration 4: 821 nodes, 182 classes,  0 unions   -> Continue
Iteration 5: 821 nodes, 182 classes,  0 unions   -> Converged (stop)
```

Default settings: window size = 3 iterations, minimum growth rate = 5%.
If growth drops below the threshold for 3 consecutive iterations, the
optimizer stops.

### Cost-Based Pruning

When enabled (`use_cost_pruning = true`, threshold = 1.5), the optimizer
tracks the best cost across iterations. If the best cost has not improved
by at least 1% for 3 consecutive iterations, it terminates early with
reason `cost_stagnant`.

### Beam Search

An optional beam search tracker limits the number of candidate plans
tracked at each iteration. Only the top-k plans (by cost) survive each
round, reducing memory pressure for complex queries. Disabled by default.

### Resource Budgets

The `optimize_bounded` method at `egraph.rs:901` runs equality
saturation with explicit resource limits (memory, time, iterations).
Three overflow strategies are available:

- `ReturnBestSoFar`: return the cheapest plan found before the limit
- `ReturnOriginal`: return the unoptimized input plan
- `Fail`: return an error

### Termination Summary

The optimizer logs its termination reason for every query:

```
E-graph saturation: 124ms (8 iterations, 4200 nodes, 890 classes, reason: converged)
```

Possible reasons:
- `saturated` -- egg found a fixed point (ideal)
- `converged` -- convergence detector triggered
- `cost_stagnant` -- no cost improvement for 3 iterations
- `timeout` -- wall-clock limit reached
- `iteration_limit` -- max iterations exhausted

---

## Optimizer Architecture

### The `Optimizer` Struct

The main entry point is `Optimizer` at `crates/ra-engine/src/egraph.rs:270`:

```rust
pub struct Optimizer {
    config: OptimizerConfig,
    table_stats: HashMap<String, Statistics>,
    hardware_profile: Option<HardwareProfile>,
    resource_budget: Option<ResourceBudget>,
    plan_cache: Option<Mutex<PlanCache>>,
}
```

### Optimization Fast Paths

The `optimize` method at `egraph.rs:382` tries three strategies in order:

1. **Plan cache lookup**: If caching is enabled, compute a genetic
   fingerprint of the query and check the cache. Structurally identical
   queries (differing only in literal constants) reuse cached plans.

2. **Left-deep tree**: For queries with 2-7 tables and simple join
   patterns, a deterministic left-deep builder runs in microseconds
   without invoking the e-graph.

3. **Large join optimizer**: For 10+ table joins, a specialized
   heuristic (greedy, genetic, or IDP algorithm) builds the join order.
   The e-graph is only used as a fallback.

4. **Full e-graph saturation**: The general case. Runs adaptive
   iterations with convergence detection and cost pruning.

### Incremental Reoptimization

The `optimize_incremental` method at `egraph.rs:1022` handles statistics
changes without full re-optimization:

- Small changes (< 50% row count shift): run a reduced iteration budget
  proportional to the magnitude of the change
- Large changes (structural changes, > 50% shift): fall back to full
  optimization

```rust
let fraction = (pct / 100.0).clamp(0.05, 1.0);
let iters = ((self.config.iter_limit as f64) * fraction).ceil() as usize;
```

### Configuration Defaults

Key defaults from `OptimizerConfig::default()` at `egraph.rs:238`:

| Parameter | Default | Description |
|---|---|---|
| `node_limit` | 100,000 | Max e-graph nodes |
| `iter_limit` | 30 | Fallback iteration limit |
| `time_limit_secs` | 10 | Hard timeout |
| `use_adaptive_limits` | true | Scale limits by query complexity |
| `use_cost_pruning` | true | Enable cost stagnation detection |
| `cost_pruning_threshold` | 1.5 | Prune plans >50% worse than best |
| `use_join_graph_filtering` | true | Filter invalid join combos |
| `large_join_threshold` | 10 | Tables to trigger large-join path |
| `enable_plan_cache` | false | Fingerprint-based plan caching |

---

## Performance Characteristics

### E-Graph Growth

E-graph size is primarily driven by join count due to commutativity and
associativity rules. For n tables:

| Tables | Approx. E-Graph Nodes | Approx. Classes | Typical Time |
|---|---|---|---|
| 1 | ~20 | ~10 | <1ms |
| 2-3 | ~200-500 | ~50-120 | 1-10ms |
| 4-5 | ~1,000-3,000 | ~200-500 | 10-50ms |
| 6-8 | ~5,000-20,000 | ~500-2,000 | 50-500ms |
| 10+ | >50,000 | >5,000 | uses large-join path |

### Phase Timing Breakdown

A typical optimization breaks down as:

```
Total optimization: 124ms
  to_rec_expr:   0.3ms   (AST -> S-expression conversion)
  egraph:       120ms     (equality saturation, ~90% of time)
  extract_best:   3.7ms   (cost-based plan selection)
```

The saturation phase dominates. Within saturation, egg spends most time
on pattern matching and congruence closure (merging equivalence classes).

### Memory Usage

Each e-graph node is approximately 64 bytes. A 10,000-node e-graph
uses ~640 KB. The resource budget system (`optimize_bounded`) tracks
memory estimates and can terminate if the budget is exceeded.

---

## Worked Examples

### Example 1: TPC-H Q5 (5-Way Join)

TPC-H Query 5 joins `region`, `nation`, `customer`, `orders`, and
`lineitem` with filters on region name and order date.

```sql
SELECT n_name, SUM(l_extendedprice * (1 - l_discount)) AS revenue
FROM customer, orders, lineitem, supplier, nation, region
WHERE c_custkey = o_custkey
  AND l_orderkey = o_orderkey
  AND l_suppkey = s_suppkey
  AND c_nationkey = s_nationkey
  AND s_nationkey = n_nationkey
  AND n_regionkey = r_regionkey
  AND r_name = 'ASIA'
  AND o_orderdate >= DATE '1994-01-01'
  AND o_orderdate < DATE '1995-01-01'
GROUP BY n_name
ORDER BY revenue DESC;
```

**E-graph optimization trace:**

```
Complexity: Medium (6 tables, outer_joins=0)
Iter limit: 10, timeout: 500ms

Iteration 1: 1200 nodes, 280 classes
  - filter-true, constant folding rules fire
  - filter-through-join-left pushes date filter toward orders scan

Iteration 2: 2800 nodes, 520 classes
  - join-commutativity creates alternative orderings
  - filter-below-aggregate pushes region filter below aggregation

Iteration 3: 4200 nodes, 780 classes
  - join-associativity explores different join trees
  - sqlite-const-prop-join propagates r_name = 'ASIA'

Iteration 4: 4800 nodes, 810 classes
  - Growth slowing (14% vs 50% in iteration 2)

Iteration 5: 4900 nodes, 815 classes
  - Converged: growth < 5% for 2 consecutive iterations

Extraction: cost function favors plan with:
  1. region scan filtered by r_name = 'ASIA' (small)
  2. nation join (indexed on r_regionkey)
  3. customer join (indexed on c_nationkey)
  4. orders join with date filter pushed down
  5. lineitem join on orderkey
  6. Final aggregate + sort

E-graph: 4900 nodes, 815 classes, 5 iterations, 45ms
```

### Example 2: JOB Query 1a (3-Way Join)

From the Join Order Benchmark:

```sql
SELECT mc.note AS production_note, t.title, t.production_year
FROM movie_companies AS mc, title AS t, company_type AS ct
WHERE ct.kind = 'production companies'
  AND mc.note LIKE '%(Australia)%'
  AND ct.id = mc.company_type_id
  AND t.id = mc.movie_id;
```

**E-graph optimization trace:**

```
Complexity: Simple (3 tables)
Iter limit: 5, timeout: 200ms

Iteration 1: 300 nodes, 80 classes
  - filter-split-and: separate ct.kind and mc.note filters
  - filter-through-join-left: push ct.kind filter to company_type

Iteration 2: 600 nodes, 140 classes
  - join-commutativity: try {mc join t} vs {t join mc}
  - filter-through-join-right: push mc.note filter to movie_companies

Iteration 3: 700 nodes, 155 classes
  - Converged

Extraction: selects plan:
  1. company_type filtered by kind = 'production companies' (~1 row)
  2. Nested-loop or hash join to movie_companies filtered by note LIKE
  3. Hash join to title on movie_id

E-graph: 700 nodes, 155 classes, 3 iterations, 8ms
```

### Example 3: Expression Simplification

```sql
SELECT * FROM t WHERE NOT (NOT (a > 5 AND b < 10))
```

The e-graph applies expression simplification rules:

```
Input:  (not (not (and (gt a 5) (lt b 10))))

Step 1: double-negation rule fires
        -> (and (gt a 5) (lt b 10))

Step 2: filter-split-and fires
        -> (filter (gt a 5) (filter (lt b 10) (scan t)))

Both the merged and split forms exist. Cost function selects the form
that matches available indexes (e.g., if there's an index on column a,
the split form with (gt a 5) as the outer filter may be preferred).
```

### Example 4: COUNT(*) Metadata Shortcut

```sql
SELECT COUNT(*) FROM users
```

The `count-star-to-metadata` rule rewrites this:

```
Input:  (aggregate (list) (list (agg-expr (count nil) all nil)) (scan users))

Rule:   count-star-to-metadata fires
        -> (metadata-lookup users row-count)

Cost:   MetadataLookup cost = 1.0
        Scan + Aggregate cost = ~300 (scan) + ~200 (aggregate) = ~500

Result: metadata-lookup wins by 500x cost reduction
```

---

## Key Source Files

| File | Purpose |
|---|---|
| `crates/ra-engine/src/egraph.rs` | `RelLang` definition, `Optimizer`, `to_rec_expr`, error types |
| `crates/ra-engine/src/rewrite.rs` | All ~170 rewrite rules organized by category |
| `crates/ra-engine/src/rule_priority.rs` | RFC 0058 priority scoring and sorting |
| `crates/ra-engine/src/rule_metadata.rs` | `ComplexityClass`, `BenefitRange`, `.rra` file parsing |
| `crates/ra-engine/src/analysis.rs` | `RelAnalysis` -- per-e-class metadata tracking |
| `crates/ra-engine/src/extract.rs` | `RelCostFn`, `extract_best`, `rec_expr_to_rel_expr` |
| `crates/ra-engine/src/cost.rs` | `IntegratedCostFn` -- statistics + hardware cost model |
| `crates/ra-engine/src/cardinality_cost.rs` | `CardinalityAwareCostFn` -- ML-based cardinality |
| `crates/ra-engine/src/convergence.rs` | `ConvergenceDetector` -- early termination |
| `crates/ra-engine/src/query_complexity.rs` | `QueryComplexity` -- adaptive limit classification |
| `crates/ra-engine/src/cost_pruning.rs` | Cost-based pruning during saturation |
| `crates/ra-engine/src/beam_search.rs` | Beam search for search space management |
| `crates/ra-engine/src/plan_cache.rs` | Fingerprint-based plan caching |
| `crates/ra-engine/src/stats_cache.rs` | Arc-wrapped statistics cache for extraction |
