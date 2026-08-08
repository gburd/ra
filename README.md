# Ra

**Experimental parser, planner, and optimizer for PostgreSQL. Research
prototype — see correctness status below.**

## Correctness status

Ra is not yet a drop-in replacement. The goal is to replace PostgreSQL's
parsing, planning, and optimization with results logically identical to the
native planner, but that parity is unproven and a fallback to the native
planner is still present. Progress is tracked mechanically:

| Tier | Corpus | Answer-correctness vs PG (matched / checked) | Wrong answers | Fallback | Harness |
|------|--------|------|------|------|---------|
| 0 | 120-query smoke suite | 109 / 112 checked | **0 mismatches** (3 re-emission gaps) | 3 emit-fail / 112 | `ra verify --tier 0 --db <url>` |
| 1 | PostgreSQL `src/test/regress` | 2217 / 2243 checked | **26 wrong-answer defects** (tracked #28) | 1039 / 3282 = 31.7% | `ra verify --tier 1 --db <url> --corpus <regress-sql>` |
| 2 | sqllogictest | not yet run | — | — | — |
| 3 | TPC-H / TPC-DS / JOB (results) | not yet run | — | — | — |
| 4 | Differential fuzzing | not yet run | — | — | — |

`ra verify --db <url>` runs the **differential result oracle**: it executes
both the original SQL and Ra's optimized-then-re-emitted SQL against a live
PostgreSQL and compares row multisets. This checks *answer-correctness*, not
just structural success. Tier 0 has **0 wrong answers** (3 queries fail only to
re-emit, tracked). Tier 1 points the same oracle at PostgreSQL's own
regression suite: of 2,243 read-only statements checked, 2,217 match and **26
produce wrong answers** (tracked as #28 — e.g. re-emission defects the harness
surfaced). 83 of 227 regress files also crash the recursive optimizer/emitter
(tracked #29), isolated per-file.

**Fallback counter (RA-STEERING §5.1).** A *fallback* is any statement Ra
cannot plan end-to-end (parse/optimize/re-emit failure) — exactly what the
PostgreSQL extension hands back to the native planner. It is now instrumented
and published per tier: Tier 1's fallback rate is **31.7%** (1,039 of 3,282
attempted statements). This count is the project's headline number and the bar
is zero (Gate 2). The in-process counter inside the PG extension itself is
separate and still pending (needs a pgrx build).

No performance claim is published until correctness parity (Gate 1) is reached
and measured end-to-end (plan + execute) against native PostgreSQL.

## Overview

Ra converts SQL into a relational algebra tree, runs equality saturation
(e-graph rewrite rules) to explore equivalent plan forms, then extracts the
lowest-cost plan using a BitNet 1.58-bit neural cost model trained online from
execution feedback. A speculative router makes an O(1) prediction (~80 ns
BitNet forward pass on Apple M3 Max, release build) about each query's
optimization difficulty and routes trivial cases (equi-join chains,
single-table scans) directly to heuristic construction, reserving the full
e-graph search for queries that actually benefit from it.

## Architecture

```
                         ┌──────────────────────┐
                         │        SQL           │
                         └──────────┬───────────┘
                                    │
                                    ▼
┌───────────────────────────────────────────────────────────────────┐
│  LIME PARSER  (LALR grammar, codeberg.org/gregburd/lime)          │
│  SQL → RelExpr (relational algebra tree)                          │
└───────────────────────────────────┬───────────────────────────────┘
                                    │
                                    ▼
┌───────────────────────────────────────────────────────────────────┐
│  SPECULATIVE ROUTER  (~80ns BitNet predict_all on M3 Max)         │
│                                                                   │
│  Extract OptimizationFeatures (16D) from RelExpr                  │
│  Predict: difficulty, iterations_needed, improvement_potential    │
│                                                                   │
│  Route decision:                                                  │
│    SKIP       → return unchanged (single-table, trivial)          │
│    LEFT_DEEP  → heuristic join ordering (equi-join chains)        │
│    EGRAPH_LOW → e-graph, 3 iterations, 5ms budget                 │
│    EGRAPH_MED → e-graph, 8 iterations, 15ms budget                │
│    EGRAPH_HI  → e-graph, 15 iterations, 50ms budget               │
└──────────┬────────────────────┬───────────────────────────────────┘
           │                    │
     (fast paths)         (e-graph path)
           │                    │
           │                    ▼
           │  ┌───────────────────────────────────────────────────────┐
           │  │  E-GRAPH EQUALITY SATURATION (egg library)            │
           │  │                                                       │
           │  │  rewrite rules applied simultaneously:                     │
           │  │    • Predicate pushdown (filter through joins)        │
           │  │    • Join reordering (commutativity, associativity)   │
           │  │    • Projection pruning (remove unused columns)       │
           │  │    • Expression simplification (constant folding)     │
           │  │    • Aggregate optimization (push through joins)      │
           │  │    • CTE inlining (small CTEs materialized inline)    │
           │  │    • Semi-join reduction, redundant join elimination  │
           │  │    • Functional dependency exploitation               │
           │  │                                                       │
           │  │  CONTINUATION GATE (every 2 iterations):              │
           │  │    If cost improvement < 0.1% → stop early            │
           │  │    If model predicts P(improve) < 30% → stop          │
           │  └──────────────────────┬────────────────────────────────┘
           │                         │
           │                         ▼
           │  ┌─────────────────────────────────────────────────────┐
           │  │  COST EXTRACTION                                    │
           │  │  BitNet cost model scores all equivalent plans      │
           │  │  Extract lowest-cost plan from e-graph              │
           │  └──────────────────────┬──────────────────────────────┘
           │                         │
           │                         ▼
           │  ┌───────────────────────────────────────────────────────┐
           │  │  ORDERING PASS (RFC 0025)                             │
           │  │  Eliminate redundant Sort, convert to IncrementalSort │
           │  └──────────────────────┬────────────────────────────────┘
           │                         │
           ▼                         ▼
┌───────────────────────────────────────────────────────────────────┐
│  OPTIMIZED RelExpr                                                │
│                                                                   │
│  → Plan cache (template-based)                                    │
│  → Training coordinator (feeds back to BitNet model)              │
│  → PostgreSQL PlannedStmt (via plan_builder)                      │
└───────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                         ┌──────────────────────┐
                         │  PostgreSQL Executor │
                         └──────────────────────┘
```

## Parser: Lime

Ra uses [Lime](https://codeberg.org/gregburd/lime), an LALR(1) parser generator with conflict resolution strategies, GLR support, and a literate grammar format. The Lime grammar defines PostgreSQL-compatible SQL syntax and produces a `RelExpr` (relational algebra) tree directly during parsing — no intermediate AST.

Lime is included as a git submodule at `crates/lime-sys/lime`. `ra-parser` uses Lime's **generated-Rust parser** (`lime --target=rust`): the grammar's reduction actions are emitted as native Rust (`%action_rust` bodies) that call a native builder layer, so SQL is parsed entirely in Rust with no C FFI on the parse path. The C tokenizer (`lime-sys`) is still used for SIMD tokenization. Structured "expected one of …" syntax-error diagnostics are built from Lime v1.1.0's Rust-target introspection (`token_name` + `expected_tokens_in_state`). The legacy C parser (`ra_sql.c`) has been fully retired. The `ra-parser` crate combines the generated parser with a `sql_to_relexpr` module that handles semantic analysis, type resolution, and expression lowering.

## Neural Cost Model: BitNet 1.58-bit

### Architecture

```
Input: [f32; 16]  OptimizationFeatures (post-A4)
         │
    ┌────┴──────┐
    │ Normalize │  x_norm = (x - μ) * σ⁻¹  (learned per-feature)
    └────┬──────┘
         │
    ┌────┴────────────────────────────────────┐
    │ Layer 1:  16 → 32                       │
    │ W₁: 512 ternary weights {-1, 0, +1}     │
    │ h = ReLU(W₁ · x_norm · α₁ + b₁)         │
    │ 128 bytes packed (2 bits per weight)    │
    └────┬────────────────────────────────────┘
         │
    ┌────┴────────────────────────────────────┐
    │ Layer 2:  32 → 16                       │
    │ W₂: 512 ternary weights {-1, 0, +1}     │
    │ y = softplus(W₂ · h · α₂ + b₂)          │
    │ 128 bytes packed                        │
    └────┬────────────────────────────────────┘
         │
Output: [f32; 16]  CostVector + routing signals
```

**Weights-only footprint: 452 bytes** (W₁ 128 + W₂ 128 + biases 192 + α₁ 4) —
see `BitNetCostModel::weights_only_bytes`. Including the second scale
α₂ and the 128-byte per-feature normalization table that loads alongside
the weights, the on-disk JSON footprint is **584 bytes**
(`model_size_bytes`). Inference (`predict_all`, all 16 dims): ~80 ns
median on Apple M3 Max release build (criterion, see `cargo bench -p
ra-bitnet`). The single-dim `predict_cpu_ms` is slightly **slower**
(~106 ns) because column-strided access to `w2_fast` defeats
auto-vectorization; prefer `predict_all` for everything but
single-output diagnostics.

### Quantization

Each weight is ternary {-1, 0, +1}, encoded in 2 bits using the absmean method from "The Era of 1-bit LLMs" (Microsoft Research, 2024):

```
α = mean(|W|)
W_q = round_clip(W / α, -1, 1)
```

At load time, ternary values are pre-multiplied by α into f32 arrays. Inference is standard FMA loops that auto-vectorize to NEON/AVX2 — the ternary nature only affects model size and training, not runtime.

### Training: QAT with Straight-Through Estimator

The `BitNetTrainer` maintains full-precision latent weights and quantizes on every forward pass. Gradients flow through quantization via STE (identity approximation). Adam optimizer with weight decay and gradient clipping.

Training happens online: every e-graph optimization run produces an `OptimizationTrace` (features, per-iteration costs, termination reason, optimal stopping point). Traces are batched (64 samples) and fed to the trainer. The model snapshots every 256 steps and is immediately available to the speculative router.

### Output Dimensions

| Dims | Purpose |
|------|---------|
| 0-11 | Cost prediction (CPU, memory, I/O, locks, WAL, cache) |
| 12 | Difficulty score (speculative router) |
| 13 | Predicted iterations needed |
| 14 | Expected improvement percentage |
| 15 | Prediction confidence |

## E-Graph Rule System

The optimizer uses [egg](https://arxiv.org/abs/2004.03082) (e-graphs good) for equality saturation. Instead of applying transformations sequentially (potentially missing better orderings), the e-graph represents ALL equivalent plans simultaneously and extracts the cheapest.

### Rule Categories

The optimizer loads **275 active rewrite rules** (hand-coded rules in
`ra-engine` plus rules compiled from `.rra` sources). Regenerate the count with
`cargo run --release -p ra-engine --example count_rules`. The table below groups
representative categories; the counts are approximate minimums, not a partition
of the 275 total.

| Category | Rules | Examples |
|----------|-------|----------|
| Predicate pushdown | 20+ | Filter through join, filter through project |
| Join reordering | 15+ | Commutativity, associativity, left-deep conversion |
| Projection pushdown | 10+ | Remove unused columns early |
| Expression simplification | 25+ | Constant folding, boolean simplification, NULL propagation |
| Aggregate optimization | 12+ | Push aggregates through joins, merge aggregates |
| Join elimination | 8+ | Remove redundant joins, self-join elimination |
| CTE optimization | 5+ | Inline small CTEs, fold constants |
| Semi-join reduction | 6+ | Distinct elimination, filter merging |
| Column pruning | 8+ | Project through set ops, limit, distinct |
| Functional deps | 5+ | Eliminate redundant sorts/distincts using FDs |
| DuckDB-inspired | 15+ | Filter combination, type-specific optimizations |
| SQLite-inspired | 10+ | Index covering, OR-to-UNION transforms |
| Runtime filters | 8+ | Bloom filter injection, min/max pruning |
| Join transformations | 10+ | Outer-to-inner conversion, null-rejecting detection |

### Rule Format (.rra)

Rules are defined in literate `.rra` files with formal algebra, implementation, preconditions, cost model, and test cases:

```
rules/
├── logical/           Predicate pushdown, join reordering, ...
└── physical/          Join algorithms, index selection, ...
```

> Hardware, distributed, and multi-model rule sources moved to
> [ra-lab](https://codeberg.org/gregburd/ra-lab).

## Dataflow: Planning and Statistics

### Planning Pipeline (inside PostgreSQL)

```
1. planner_hook intercepts Query node
2. Lime parser: SQL text → RelExpr
3. Subquery decorrelation: IN/EXISTS → SemiJoin/AntiJoin
4. Speculative router: predict route from 16D features
5. Route execution:
   - SKIP: return RelExpr unchanged
   - LEFT_DEEP: cardinality-ordered join tree construction
   - EGRAPH: equality saturation with adaptive budget
6. Ordering pass: eliminate redundant sorts, convert to IncrementalSort
7. Plan builder: RelExpr → PostgreSQL PlannedStmt
8. Return PlannedStmt to executor
```

### Statistics Flow

```
PostgreSQL catalogs (pg_statistic, pg_class)
         │
         ▼
┌─────────────────────────────────┐
│  Metadata Cache                 │
│  - Invalidated via relcache CB  │
│  - Row counts, column stats     │
│  - Index availability           │
└────────────┬────────────────────┘
             │
             ▼
┌─────────────────────────────────┐
│  Optimizer                      │
│  - Table stats → join ordering  │
│  - Column NDV → selectivity     │
│  - Index info → access paths    │
└────────────┬────────────────────┘
             │
             ▼
┌─────────────────────────────────┐
│  Execution Feedback             │
│  (executor_end_hook)            │
│  - Actual time, rows, buffers   │
│  - Compared to predicted cost   │
│  - Fed to FeedbackCollector     │
│  - Updates MAPE tracker         │
│  - Triggers model training      │
└─────────────────────────────────┘
```

The feedback loop closes the gap between predicted and actual costs. The MAPE
(Mean Absolute Percentage Error) tracker monitors prediction quality with
exponential decay (β=0.99, ~100 sample half-life). **No MAPE value has been
published yet** — whether the learned cost model beats PostgreSQL's own cost
estimates on a held-out workload is an open question (see the cost-model
validation gate in the steering doc). Until that number exists, the model's
influence on routing is unproven.

## Quick Start

### Build

```bash
git submodule update --init
cargo build
cargo test
```

Requirements: Rust 1.88+, clang (for lime-sys)

### Library Usage

```rust
use ra_parser::sql_to_relexpr;
use ra_engine::Optimizer;

let expr = sql_to_relexpr("SELECT * FROM users WHERE age > 30")?;
let optimized = Optimizer::new().optimize(&expr)?;
```

### PostgreSQL Extension

```bash
# Build and install (requires pg_config in PATH)
cargo pgrx install --features pg18 --release

# Enable in PostgreSQL
CREATE EXTENSION pg_ra_planner;

# Ra is now active for all queries. Disable per-session:
SET ra_planner.enabled = off;
```

### CLI: `ra`

```bash
cargo build -p ra-cli        # produces the `ra` binary

ra explain  'SELECT ...'            # Show relational algebra tree
ra optimize 'SELECT ...'            # Optimize with rewrite rules
ra optimize 'SELECT ...' --diff     # Before/after plan diff
ra optimize 'SELECT ...' --trace    # Per-iteration rules fired, cost deltas
ra optimize 'SELECT ...' --rules-applied   # Which rules changed the plan
ra list                             # List active rules
ra verify --tier 0 --report         # Run the qualification corpus

# `ra verify --tier 0` runs the 120-query corpus and reports per-category
# parse+optimize success. It checks STRUCTURAL success only (Ra parses and
# optimizes without error) — NOT answer-correctness vs PostgreSQL, which
# needs the PG oracle (a live connection; tracked, not yet wired).

# `ra benchmark` compares Ra against a real PostgreSQL instance.
# Set RA_BENCHMARK_PG_URL to a libpq-style URL and the command will run
# `EXPLAIN (ANALYZE, FORMAT JSON)` on PG for each query. Without the
# variable the command fails with a clear error rather than fabricating
# output (the prior `simulate_native_*` helpers were removed in E1).
RA_BENCHMARK_PG_URL='host=localhost user=postgres dbname=tpch' \
    ra benchmark --workload tpch
```

## Project Structure

```
ra/
├── models/                       # Trained BitNet model (committed)
│   └── cost_model.bitnet.json
├── crates/
│   ├── Core layer (cargo build):
│   │   ├── ra-core/              # Types: RelExpr, Expr, Cost, Statistics, config
│   │   ├── ra-parser/            # SQL → RelExpr (Lime LALR + sql_to_relexpr)
│   │   ├── ra-compiler/          # .rra rule file compilation
│   │   ├── ra-engine/            # Optimizer: e-graph, speculative router, training
│   │   ├── ra-bitnet/            # BitNet 1.58-bit model: inference + QAT training
│   │   ├── ra-dialect/           # SQL dialect translation (20+ dialects)
│   │   ├── ra-hardware/          # Hardware detection, cost calibration
│   │   ├── ra-stats-advanced/    # Advanced statistics (lib name: ra_stats)
│   │   ├── ra-cache-api/         # Cache trait definitions
│   │   ├── ra-sql-parser/        # SQL parser fork (lib name: sqlparser)
│   │   ├── lime-sys/             # Lime parser generator (C, git submodule)
│   ├── CLI layer (--features cli):
│   │   ├── ra-cli/               # Command-line interface
│   │   └── ra-metadata/          # Database metadata factory
│   ├── Experimental layer (--features experimental):
│   │   ├── ra-ml/                # Cost-model ML extras (legacy interface)
│   │   ├── ra-cache-api/         # (re-exported)
│   │   ├── ra-cache-impl/        # LRU/LFU/adaptive cache implementations
│   │   ├── ra-adaptive/          # Adaptive optimization experiments
│   │   ├── ra-test-utils/        # Shared test fixtures
│   │   ├── ra-grammar-fuzzer/    # Property-based grammar fuzzer
│   │   ├── ra-bench/             # Benchmarks: TPC-H, JOB, ra_vs_pg
│   │   ├── ra-sqltest/           # Cross-engine SQL test runner
│   │   └── ra-difftest/          # Differential testing harness
│   ├── Out of workspace build (requires pg_config + PG headers):
│   │   └── ra-pg-extension/      # PostgreSQL planner_hook extension (pgrx)
│   └── Compatibility shims:
│       └── ra-config/            # Re-export shim for ra_core::config
├── rules/                        # optimization rule sources (.rra files)
├── benchmarks/                   # Benchmark suites and results
├── tla/                          # TLA+ formal specifications
├── rfcs/                         # Design documents
└── docs/                         # Documentation
```

> Note: The `rules/` tree contains 1,449 `.rra` rule sources. Of those,
> a subset currently compile to active rewrite rules; combined with the
> hand-coded rules in `ra-engine`, `Optimizer::all_rules()` returns **275
> active rewrite rules** (regenerate via `cargo run --release -p ra-engine
> --example count_rules`). The remaining `.rra` files are spec-only and
> require additional condition functions or operator-mapping work to
> activate. Pre-2026-05-26, two malformed `.rra` rules
> (`push-func-filter-to-left/right`) contained metavariables in operator
> position and panicked the entire generated batch via `catch_unwind`; the
> build script now rejects such patterns and `all_generated_rules()` wraps
> each category independently so a single bad rule cannot drop the rest.

## Performance

No performance comparison is published. Planning-time-only speedups against
native PostgreSQL were removed: they were measured with statistics disabled on
the Ra side and did not measure plan quality, so they compared unlike work.
End-to-end (plan + execute) numbers against native PostgreSQL, with statistics
loaded on both sides and per-query regressions reported, will replace this
section once correctness parity is reached.

## References

- [egg: Fast and Extensible Equality Saturation](https://arxiv.org/abs/2004.03082)
- [The Era of 1-bit LLMs](https://arxiv.org/abs/2402.17764) (Microsoft Research, 2024)
- [Lime Parser Generator](https://codeberg.org/gregburd/lime)
- [Access Path Selection in System R](https://dl.acm.org/doi/10.1145/582095.582099) (Selinger et al.)
- [The Volcano Optimizer Generator](https://dl.acm.org/doi/10.1109/69.273032) (Graefe)

## Copyright and License

The Author (Greg Burd <greg@burd.me>) directed and reviewed this work, which was
written with substantial AI assistance under his guidance. The Author asserts
copyright in the selection, arrangement, direction, and human-authored portions
of this work, and licenses the whole under the terms below. Where individual
AI-generated fragments may not be independently copyrightable under current US
law, they are offered under the same terms for the avoidance of doubt; no
additional restriction is placed on their use. Background reading:

- https://legalclarity.org/can-you-copyright-ai-generated-content/
- https://www.congress.gov/crs-product/LSB10922

You may use this work under any one of the following licenses:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))
- ISC License ([LICENSE-ISC](LICENSE-ISC))

## Disclosure

This work was created with an even blend of human and AI contributions. AI was
used to make content edits, such as changes to scope, information, and ideas.
AI was used to make new content, such as text, images, analysis, and ideas. AI
was prompted for its contributions, or AI assistance was enabled. AI-generated
content was reviewed and approved. The following model(s) or application(s)
were used: claude-opus-4.7.  (AIA HAb CeNc Hin R claude-opus-4.7 v1.0)

![Alt Text](ai-stmt.svg) 
