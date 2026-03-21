# Ra Implementation Architecture

This guide explains the key libraries and systems that power Ra's optimization engine.

## Core Libraries

### egg - E-graph Equality Saturation

**What:** Equality saturation library for term rewriting using e-graphs

**Version:** 0.9

**Used in:** ra-engine, ra-multimodel

**Purpose:** Core optimization engine that exhaustively explores equivalent query plans

#### How It Works

E-graphs (equality graphs) compactly represent equivalence classes of expressions. Instead of maintaining separate copies of equivalent expressions, an e-graph groups them into equivalence classes called e-classes. Each e-class contains e-nodes (expressions) that are equivalent.

Equality saturation applies rewrite rules exhaustively until saturation (no new expressions can be added) or a resource budget is exceeded. This avoids the phase-ordering problem of traditional optimizers where the order of rule application matters.

**Key Concepts:**

- **E-nodes:** Individual expression nodes (operators with children)
- **E-classes:** Equivalence classes grouping semantically equivalent e-nodes
- **Rewrite rules:** Pattern-based transformations (`(filter ?p (filter ?q ?input))` → `(filter (and ?p ?q) ?input)`)
- **Extraction:** Cost-based selection of the best expression from an e-class

#### Why We Chose It

Egg enables exhaustive exploration of the query plan space without exponential memory blowup. Traditional optimizers apply rules in phases (predicate pushdown, then join reordering, then physical planning) which risks missing optimal plans when earlier decisions block later optimizations. Equality saturation explores all equivalent plans simultaneously.

From `/Users/gregburd/src/ra/crates/ra-engine/src/egraph.rs:6`:
> Drives equality saturation to explore all equivalent query plans

#### Integration in Ra

**Language Definition** (`ra-engine/src/egraph.rs:28`): Ra defines `RelLang` using egg's `define_language!` macro, representing relational operators as S-expressions:

```rust
define_language! {
    pub enum RelLang {
        "scan" = Scan([Id; 1]),
        "filter" = Filter([Id; 2]),
        "join" = Join([Id; 4]),
        "project" = Project([Id; 2]),
        // ... 50+ operators
    }
}
```

**Rewrite Rules** (`ra-engine/src/rewrite.rs:18`): Rules are defined using egg's `rewrite!` macro:

```rust
rewrite!("filter-merge";
    "(filter ?p1 (filter ?p2 ?input))" =>
    "(filter (and ?p1 ?p2) ?input)"
)
```

**Optimization Pipeline** (`ra-engine/src/egraph.rs:338`): The `Optimizer` converts `RelExpr` to egg's `RecExpr`, runs equality saturation using `Runner::default()`, and extracts the best plan:

```rust
let runner: Runner<RelLang, RelAnalysis> = Runner::default()
    .with_iter_limit(config.iter_limit)
    .with_node_limit(config.node_limit)
    .run(&all_rules())
```

**Analysis Pass** (`ra-engine/src/analysis.rs`): Ra implements `egg::Analysis` to propagate metadata (table references, cardinality estimates) through the e-graph during optimization.

### sqlparser - SQL Parsing

**What:** SQL parsing library for multiple SQL dialects

**Version:** 0.52

**Used in:** ra-parser, ra-dialect, ra-regression

**Purpose:** Parse SQL strings into abstract syntax trees (AST)

#### Why We Chose It

Sqlparser supports multiple SQL dialects (PostgreSQL, MySQL, SQLite, ClickHouse) with a single unified AST. It's actively maintained, well-tested, and provides visitor patterns for AST traversal.

#### Integration in Ra

**SQL to RelExpr** (`ra-parser`): Converts sqlparser AST to Ra's `RelExpr` (relational algebra):

```
SQL string → sqlparser::Statement → ra_core::RelExpr → ra-engine e-graph
```

**Dialect Extensions** (`ra-dialect`): Extends sqlparser for dialect-specific syntax (PostgreSQL `LATERAL`, MySQL optimizer hints).

**Regression Testing** (`ra-regression/Cargo.toml:9`): Uses sqlparser with visitor features to analyze query structure for regression detection.

### Apache DataFusion - Query Execution

**What:** Query execution framework built in Rust

**Version:** 43.0

**Used in:** ra-regression

**Purpose:** Execute optimized query plans for regression testing

#### How We Use It

Ra generates optimized plans, then passes them to DataFusion's execution engine to produce actual results. The regression detector compares plan fingerprints and costs before/after optimizer changes.

From `/Users/gregburd/src/ra/crates/ra-regression/Cargo.toml:10`:
```toml
datafusion = { version = "43.0", features = ["crypto_expressions"] }
```

**Integration Pattern:**
1. Ra optimizer produces optimized `RelExpr`
2. Convert `RelExpr` to DataFusion `LogicalPlan`
3. DataFusion executes plan and returns results
4. Regression detector compares execution metrics

**Note:** DataFusion is used for testing/validation, not production execution. Ra has its own execution backends (Volcano iterator, bytecode VM, Cranelift JIT).

### Cranelift & Wasmtime - Code Generation

**What:** JIT compilation toolchain for native code generation

**Version:** Cranelift 0.110, Wasmtime 25.0

**Used in:** ra-codegen

**Purpose:** Compile expressions to native machine code (Cranelift) or WebAssembly (Wasmtime)

#### Why We Use Them

Query execution spends most time evaluating expressions (predicates, projections, aggregates). Interpreting expression trees is slow. JIT compilation generates tight machine code that avoids pointer chasing and branch mispredictions.

From `/Users/gregburd/src/ra/crates/ra-codegen/src/lib.rs:12`:
> JIT compilation of integer expressions to native machine code via Cranelift

#### Integration in Ra

**Cranelift Backend** (`ra-codegen/src/cranelift_backend.rs`): Compiles integer expressions to native x86-64 code. Used for tight loops in scans and joins.

**WASM Backend** (`ra-codegen/src/wasm.rs`): Compiles expressions to WebAssembly for sandboxed execution (untrusted UDFs, web UI query playground).

**Bytecode Fallback** (`ra-codegen/src/bytecode.rs`): Stack-based bytecode VM for platforms without JIT support.

**When to Use Each:**
- Cranelift: Hot loops, trusted code, x86-64 platforms
- WASM: Sandboxed execution, untrusted UDFs, web browsers
- Bytecode: Cold code paths, unsupported architectures

### Timely & Differential Dataflow - Incremental Computation

**What:** Stream processing framework for incremental computation

**Version:** Timely 0.12, Differential Dataflow 0.12

**Used in:** ra-engine

**Purpose:** Incrementally update optimization results when rules or queries change

#### How It Works

Differential dataflow maintains collections as (data, time, diff) triples. When inputs change, it propagates only the differences through the dataflow graph. This enables incremental optimization: when a rule is added/removed, only affected queries are reoptimized.

From `/Users/gregburd/src/ra/crates/ra-engine/src/differential.rs:1`:
> Uses differential dataflow to incrementally maintain optimization results. When rules are added or removed, only the affected queries are reoptimized rather than rerunning the full optimizer.

#### Integration in Ra

**Incremental Optimizer** (`ra-engine/src/differential.rs:34`): Tracks rule dependencies per query. When the rule set changes, identifies affected queries using differential dataflow operators.

```rust
pub struct IncrementalOptimizer {
    // Differential collections:
    // 1. Rules collection - active rewrite rules
    // 2. Queries collection - registered queries and rule dependencies
}
```

**Use Case:** Long-running optimization servers where users interactively enable/disable rules and observe plan changes.

### proptest - Property-Based Testing

**What:** Property-based testing framework (QuickCheck for Rust)

**Version:** 1.5

**Used in:** ra-engine, ra-multimodel

**Purpose:** Verify optimizer correctness through randomized testing

#### Why It Matters

Query optimizers have subtle correctness requirements: rewrites must preserve semantics, cost models must respect query structure, null handling must be precise. Unit tests catch known bugs but miss edge cases.

Property-based testing generates thousands of random query plans and verifies invariants:
- Optimized plan produces same results as original
- Multiple optimization passes converge (idempotence)
- Null propagation rules preserve three-valued logic

From `/Users/gregburd/src/ra/crates/ra-engine/tests/proptest_optimization.rs` and `proptest_algebraic_properties.rs`:
> Uses proptest to verify optimization correctness across thousands of generated query plans

**Example Properties Tested:**
- **Equivalence:** `optimize(plan)` produces same results as `plan`
- **Idempotence:** `optimize(optimize(plan))` = `optimize(plan)`
- **Associativity:** Join order changes preserve results
- **Null handling:** `NULL AND FALSE` = `FALSE` (not `NULL`)

## Inspirational Systems

### Apache Calcite - Framework Concepts

**What:** Java-based query optimizer framework

**Not directly used** - Ra is implemented in Rust with a different architecture

**Influence:** Rule organization, cost model design, logical/physical split

#### Concepts We Adopted

From `/Users/gregburd/src/ra/docs/research/paper.md:26`:
> Traditional optimizers apply rules in a fixed order, risking local optima. Ra uses Volcano/Cascades patterns from Calcite but replaces phase-based rule application with equality saturation.

**Volcano/Cascades Pattern:** Separation of logical (semantic equivalence) and physical (implementation choice) rules. Ra's rule categories follow this split.

**Rule Metadata:** Calcite rules have preconditions and cost estimates. Ra's `.rra` format extends this with formal algebra, test cases, and database references.

**Metadata Propagation:** Calcite's RelMetadataProvider pattern inspired Ra's e-graph analysis system for tracking table statistics.

**References:** 55 rules in Ra's documentation reference Calcite implementations (`docs/rules/REFERENCES.md:74`). These are inspirational - Ra doesn't use Calcite code but documents where similar optimizations exist.

## Supporting Libraries

### serde - Serialization

**Version:** 1.0

**Used in:** All crates

**Purpose:** Serialize query plans, rules, and metadata to JSON/YAML

**Use Cases:**
- Plan visualization in web UI
- Rule metadata in `.rra` files
- Cost calibration profiles
- Regression test storage

### tracing - Structured Logging

**Version:** 0.1

**Used in:** All crates

**Purpose:** Structured logging and diagnostics

**Why Not println!:**
- Tracing is zero-cost when disabled (filtered at compile time)
- Structured fields enable log aggregation (query_id, rule_name)
- Spans track nested operations (optimization duration, rule application)

**Usage Pattern:**
```rust
#[tracing::instrument]
fn optimize(&self, plan: &RelExpr) -> Result<RelExpr> {
    tracing::info!("starting optimization");
    tracing::debug!(plan = ?plan, "input plan");
    // ...
}
```

### tokio - Async Runtime

**Version:** 1.0

**Used in:** ra-web, ra-pg-monitor

**Purpose:** Async HTTP server (web UI), async PostgreSQL monitoring

**Why Async:** Web UI serves multiple concurrent users, each requesting plan visualizations. Async I/O prevents blocking threads while waiting for database queries or file I/O.

## Dependency Philosophy

Ra prefers libraries that are:
- **Mature:** Proven in production (egg: 0.9 stable, sqlparser: widely used)
- **Focused:** Do one thing well (egg does e-graphs, sqlparser does parsing)
- **Rust-native:** Avoid FFI overhead and unsafe code
- **Supply-chain vetted:** Use `cargo deny` to audit dependencies

**Avoided Dependencies:**
- LLVM (too large, slow compile times) → Cranelift instead
- Python bindings (FFI complexity) → Pure Rust
- Database-specific clients (tight coupling) → Generic traits

## References

- **egg paper:** Willsey et al. "egg: Fast and Extensible Equality Saturation" POPL 2021
- **Equality saturation:** Tate et al. "Equality Saturation: A New Approach to Optimization" POPL 2009
- **Volcano optimizer:** Graefe "The Volcano Optimizer Generator" ICDE 1993
- **Cascades framework:** Graefe "The Cascades Framework for Query Optimization" IEEE Data Engineering Bulletin 1995
- **Differential dataflow:** McSherry et al. "Differential Dataflow" CIDR 2013

## Further Reading

- **Rule authoring:** `/Users/gregburd/src/ra/docs/guides/rule-authoring.md`
- **Cost models:** `/Users/gregburd/src/ra/docs/guides/cost-models.md`
- **Research paper:** `/Users/gregburd/src/ra/docs/research/paper.md`
- **E-graph integration:** `/Users/gregburd/src/ra/crates/ra-engine/src/egraph.rs`
- **Rewrite rules:** `/Users/gregburd/src/ra/crates/ra-engine/src/rewrite.rs`
