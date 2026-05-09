# Relational Algebra Rule System

> The definitive open-source repository of relational algebra transformation rules

A system for database query optimization built on literate programming,
equality saturation, and differential dataflow. It codifies decades of
database optimization knowledge from academic research and production
systems (PostgreSQL, MySQL, DuckDB, SQLite, and more) into a single,
maintainable, formally verified framework.

## Features

- **1,387 Transformation Rules** in 5 categories: logical, hardware
  (GPU/FPGA/SIMD), distributed, multi-model, and physical
- **Literate Programming** -- Each rule is a documented `.rra` file
  with formal algebra, implementation, preconditions, cost model,
  and test cases
- **Equality Saturation** -- Uses the `egg` library for e-graph-based
  optimization that explores all equivalent plans simultaneously
- **Neural-Guided Optimization** -- Full-pipeline neural integration:
  learned rule selection, adaptive saturation convergence, hybrid
  neural/traditional cost extraction, and online learning from
  execution feedback
- **Three-Tier Rule Tracking** -- Inspect which rules applied, which
  were evaluated but didn't match, and which are available in the
  system for debugging and optimization analysis
- **Index Access Method Abstraction** -- Database-agnostic index
  capability discovery that automatically detects and uses GIN, RUM,
  GiST, BRIN, and custom index types without hardcoding
- **PostgreSQL Extension** -- Native `ra_pg_extension` that hooks
  into PostgreSQL's planner for transparent query optimization
- **Progressive Re-Optimization** -- Mid-execution plan switching
  when runtime statistics diverge from estimates (RFC 0052)
- **Rule Complexity Prioritization** -- Intelligent rule ordering by
  cost-to-benefit ratio for 20-27% faster optimization on complex
  queries (RFC 0058)
- **Plan Cache** -- 37x OLTP speedup with template-based plan caching
  (97.5% hit rate across 5 templates)
- **Hardware-Aware Optimization** -- Cost models for GPU, FPGA, SIMD,
  and NUMA-aware operator placement
- **Distributed Query Planning** -- Broadcast, shuffle, co-located,
  and semi-join strategies with exchange operator management
- **Multi-Model Support** -- Rules for graph traversal, document
  queries, and time-series operations
- **SQL Dialect Translation** -- Translate SQL between 20+ dialects
  including PostgreSQL, MySQL, SQLite, DuckDB, MSSQL, and Oracle
- **Formal Verification** -- TLA+ specifications proving termination,
  cost monotonicity, and semantic equivalence
- **Resource Budgets** -- Constrain optimizer time, memory, and
  iterations with predefined profiles (interactive, standard, batch,
  memory-constrained) and custom limits
- **Plan Diff Visualization** -- Colorized structural diffs between
  original and optimized plans in four formats (colored, plain,
  side-by-side, compact)

## Quick Start

### Using Nix (Recommended)

```bash
nix develop
cargo build
cargo test
```

### Without Nix

Requirements: Rust 1.88+, clang (for lime-sys build)

```bash
git submodule update --init
cargo build
cargo test
```

### Library Usage

The core optimizer is available as a Rust library. A plain `cargo build`
builds only the default workspace members (the library layer):

```rust
use ra_parser::sql_to_relexpr::sql_to_relexpr;
use ra_engine::Optimizer;

let expr = sql_to_relexpr("SELECT * FROM users WHERE age > 30")?;
let optimized = Optimizer::new().optimize(&expr)?;
```

### CLI Usage

Build and run the interactive CLI:

```bash
cargo build -p ra-cli
```

The CLI commands are organized into logical groups:

**Query analysis** — Parse, optimize, compare, and translate SQL:
```bash
ra-cli explain  'SELECT ...'              # Show relational algebra plan tree
ra-cli optimize 'SELECT ...'              # Optimize with rewrite rules
ra-cli optimize 'SELECT ...' --diff colored  # Show before/after diff
ra-cli optimize 'SELECT ...' --trace      # Trace which rules fired
ra-cli compare  'SELECT ...' --db postgres://...  # Compare vs native EXPLAIN
ra-cli translate --from postgres --to mysql 'SELECT ...'
ra-cli format   'SELECT ...'              # Pretty-print SQL
```

**Rule management** — Inspect, validate, and test the 1,387 optimization rules:
```bash
ra-cli list                     # List all rules (filterable by --category, --tag)
ra-cli show <rule-id>           # Show detailed rule metadata
ra-cli validate rules/          # Validate .rra file syntax
ra-cli test rules/              # Run embedded test cases
ra-cli stats                    # Rule collection statistics
```

**Database integration** — Connect to live databases:
```bash
ra-cli gather-metadata --db postgres://...  # Export schema/stats to JSON
ra-cli monitor --db postgres://...          # Schema analysis + tuning advice
ra-cli proxy --db postgres://...            # Transparent optimizer proxy
ra-cli benchmark --db postgres://...        # Compare Ra vs native optimizer
```

**ML and neural model** — Manage the learned cost model:
```bash
ra-cli ml train --input data.json           # Train from execution feedback
ra-cli ml stats                             # Model accuracy metrics
ra-cli ml export                            # Export model for inspection
```

**Shell completions** — Enable tab-completion:
```bash
ra-cli completions bash > ~/.local/share/bash-completion/completions/ra-cli
ra-cli completions zsh  > ~/.zfunc/_ra-cli
ra-cli completions fish > ~/.config/fish/completions/ra-cli.fish
```

See the [Getting Started guide](docs/getting-started.md) for a full
walkthrough, or `ra-cli <command> --help` for detailed usage of any command.

### Web Explorer

Run the interactive web explorer locally:

```bash
# Docker (simplest)
./scripts/docker-run.sh

# Docker Compose (better for development)
./scripts/docker-compose-up.sh
```

Then open http://localhost:8000 for local.

See [Deployment Guide](docs/deployment.md) for full details on Docker, Kubernetes, and cloud deployment.

## Project Structure

```
ra/
├── crates/                    # Rust workspace (22 crates)
│   ├── ra-core/               # Core types: RelExpr, Expr, Cost, Rule, Statistics
│   ├── ra-parser/             # SQL → RelExpr (Lime LALR grammar + sql_to_relexpr)
│   ├── ra-compiler/           # .rra rule file compilation and indexing
│   ├── ra-engine/             # Optimization engine (egg e-graph + neural pipeline)
│   ├── ra-hardware/           # Hardware detection + cost calibration (CPU/GPU/FPGA)
│   ├── ra-stats-advanced/     # Streaming statistics, staleness tracking, monitoring
│   ├── ra-dialect/            # SQL dialect translation (20+ dialects)
│   ├── ra-cache-api/          # Plan cache trait definitions
│   ├── ra-cache-impl/         # Plan cache LRU/LFU/adaptive implementations
│   ├── ra-sql-parser/         # Custom sqlparser fork (SQL parsing frontend)
│   ├── ra-ml/                 # ML cardinality estimation
│   ├── ra-adaptive/           # Runtime reoptimization
│   ├── ra-metadata/           # Database metadata factory
│   ├── ra-adapters/           # Database connectors (DuckDB, MySQL, Stoolap)
│   ├── ra-cli/                # Command-line interface
│   ├── ra-bench/              # Benchmark harness (JOB, TPC-H, TPROC-C)
│   ├── ra-grammar-fuzzer/     # Grammar-based SQL fuzzer
│   ├── ra-test-utils/         # Shared test utilities
│   ├── ra-quel-parser/        # QUEL language parser (experimental)
│   ├── ra-pg-extension/       # PostgreSQL planner extension (pgrx, excluded)
│   ├── lime-sys/              # C library: Lime parser generator
│   └── lime-rs/               # Rust bindings for lime-sys
├── rules/                     # 1,387 rule definitions (.rra files)
│   ├── logical/               # Predicate pushdown, join reordering, ...
│   ├── physical/              # Join algorithms, index selection, ...
│   ├── hardware/              # GPU, FPGA, SIMD, NUMA, data placement
│   ├── distributed/           # Exchange, broadcast join, partition pruning
│   └── multi-model/           # Graph, document, time-series
├── benchmarks/                # Benchmark suites and results
├── web/                       # Web explorer frontend (Preact)
├── tla/                       # TLA+ formal specifications
├── rfcs/                      # Design documents and proposals
├── docs/                      # Documentation (VitePress)
├── scripts/                   # Shell utilities (docker, benchmarks, TLA+)
└── tests/                     # Integration and property tests
```

## Workspace Layers

The workspace is organized into three layers controlled by Cargo features:

| Layer | Build command | What's included |
|-------|--------------|-----------------|
| **Core** (default) | `cargo build` | Parser, engine, hardware, statistics, dialect — the library |
| **CLI** | `cargo build -p ra-cli` | Core + database adapters + metadata + CLI binary |
| **All** | `cargo build -p ra --features all` | Everything including experimental ML and fuzzer |

The PostgreSQL extension (`ra-pg-extension`) is excluded from the workspace and
requires `pg_config` + PostgreSQL headers. Build it separately with `cargo pgrx`.

## Rule Format

Rules are written in `.rra` (Relational Rule Algebra) literate
markdown format:

```markdown
---
id: filter-through-join
name: Filter Pushdown Through Join
category: logical/predicate-pushdown
databases: [postgresql, mysql, duckdb]
---

# Filter Pushdown Through Join

## Description
Pushes selection predicates through join operators when the predicate
only references columns from one side of the join.

## Relational Algebra
sigma[p](R join[c] S) -> (sigma[p](R)) join[c] S
  where attrs(p) is a subset of attrs(R)

## Implementation
[egg rewrite rules in Rust]

## Preconditions
[When the rule applies and when it does not]

## Cost Model
[Estimated benefit with selectivity analysis]

## Test Cases
[SQL examples: positive cases and negative cases]

## References
[Database source code links and academic papers]
```

## Documentation

### For library users (embedding Ra in your project)

- [API Reference](docs/api-reference.md) -- Programmatic usage (`Optimizer`, `RelExpr`, `Statistics`)
- [Architecture](docs/architecture.md) -- Crate dependency graph and data flow
- [Cost Models](docs/guides/cost-models.md) -- Cost estimation, calibration, neural blend
- [Rule Authoring Guide](docs/guides/rule-authoring.md) -- Write custom `.rra` rules
- [Hardware Acceleration](docs/features/hardware-acceleration.md) -- GPU/FPGA/SIMD cost models
- [PostgreSQL Extension](docs/postgresql-extension.md) -- Native planner hook integration

### For educators and learners (understanding query optimization)

- [Getting Started](docs/getting-started.md) -- Installation and interactive walkthrough
- [Plan Visualization](docs/features/plan-visualization.md) -- Colorized plan diffs
- [Resource Budgets](docs/features/resource-budgets.md) -- How optimizers manage time/memory tradeoffs
- [Dialect Translation](docs/guides/dialect-translation.md) -- How SQL varies across databases
- [Distributed Optimization](docs/features/distributed-optimization.md) -- Network-aware query planning
- [Formal Verification](docs/features/formal-verification.md) -- Proving optimizer correctness with TLA+
- [Benchmarks](docs/benchmarks.md) -- JOB and TPC-H performance results

### Reference

- [Documentation Index](docs/README.md) -- Full documentation map
- [Contributing](CONTRIBUTING.md) -- Development standards and contribution guide
- [Neural Cost Model](docs/NEURAL_COST_MODEL.md) -- Learned cost estimation architecture
- [Neural Pipeline](docs/NEURAL_MODEL_PIPELINE.md) -- Training and deployment guide

## Development

```bash
# Build core library (default members)
cargo build

# Build CLI
cargo build -p ra-cli

# Run all tests
cargo test

# Run linter (zero warnings required)
cargo clippy --all-targets --all-features -- -D warnings

# Format code
cargo fmt

# Run benchmarks
cargo bench --package ra-engine

# Validate all rules
cargo run -p ra-cli -- validate rules/

# Run TLA+ formal verification
./scripts/run-tla.sh

# Generate API documentation
cargo doc --no-deps --all-features --open
```

## Contributing

Contributions are welcome in these areas:

1. **Rule Extraction** -- Extract rules from database source code
2. **Rule Writing** -- Document optimizations in `.rra` format
3. **Testing** -- Add test cases and property-based tests
4. **Verification** -- Write TLA+ specifications
5. **Documentation** -- Improve guides and examples
6. **Dialect Support** -- Add SQL dialect translations
7. **Hardware Rules** -- Add rules for new accelerators

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License

This project is licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Acknowledgments

This project builds on decades of database research and open-source
contributions:

- PostgreSQL optimizer team
- DuckDB developers
- Apache DataFusion community
- SQLite project
- egg (e-graphs good) library
- Materialize / Differential Dataflow
- Academic research: Selinger et al. (System R), Graefe (Volcano),
  Neumann (HyPer), Boncz (MonetDB/X100), and many others

## References

- [egg: Fast and Extensible Equality Saturation](https://arxiv.org/abs/2004.03082)
- [Differential Dataflow](https://github.com/TimelyDataflow/differential-dataflow)
- [The Volcano Optimizer Generator](https://dl.acm.org/doi/10.1109/69.273032)
- [Access Path Selection in a Relational Database (System R)](https://dl.acm.org/doi/10.1145/582095.582099)
