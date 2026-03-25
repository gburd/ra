# Relational Algebra Rule System

> The definitive open-source repository of relational algebra transformation rules

A system for database query optimization built on literate programming,
equality saturation, and differential dataflow. It codifies decades of
database optimization knowledge from academic research and production
systems (PostgreSQL, MySQL, DuckDB, SQLite, and more) into a single,
maintainable, formally verified framework.

## Features

- **1,327+ Transformation Rules** in 5 categories: logical, hardware
  (GPU/FPGA/SIMD), distributed, multi-model, and physical
- **Literate Programming** -- Each rule is a documented `.rra` file
  with formal algebra, implementation, preconditions, cost model,
  and test cases
- **Equality Saturation** -- Uses the `egg` library for e-graph-based
  optimization that explores all equivalent plans simultaneously
- **Progressive Re-Optimization** -- Mid-execution plan switching
  when runtime statistics diverge from estimates (RFC 0052)
- **Rule Complexity Prioritization** -- Intelligent rule ordering by
  cost-to-benefit ratio for 20-27% faster optimization on complex
  queries (RFC 0058)
- **Plan Cache** -- 37x OLTP speedup with template-based plan caching
  (97.5% hit rate across 5 templates)
- **Streaming Statistics** -- Lock-free ring buffer pipeline with
  adaptive cost model and monitoring adapters (OpenTelemetry,
  Prometheus, StatsD)
- **Hardware-Aware Optimization** -- Cost models for GPU, FPGA, SIMD,
  and NUMA-aware operator placement
- **Distributed Query Planning** -- Broadcast, shuffle, co-located,
  and semi-join strategies with exchange operator management
- **Multi-Model Support** -- Rules for graph traversal, document
  queries, and time-series operations
- **SQL Dialect Translation** -- Translate SQL between 20+ dialects
  including PostgreSQL, MySQL, SQLite, DuckDB, MSSQL, and Oracle
- **PostgreSQL Extension** -- Native `ra_pg_extension` that hooks
  into PostgreSQL's planner for transparent query optimization
- **Isolation Testing** -- Cross-database transaction isolation
  verification using PostgreSQL's `.spec` format
- **WASM Database Adapters** -- Run SQLite and DuckDB in the browser
  via WebAssembly
- **ML Cardinality Estimation** -- Neural network models trained on
  execution feedback
- **Adaptive Execution** -- Runtime reoptimization and mid-query plan
  switching
- **Multiple Backends** -- JIT compilation (Cranelift), WASM, and
  bytecode interpretation
- **Formal Verification** -- TLA+ specifications proving termination,
  cost monotonicity, and semantic equivalence
- **Resource Budgets** -- Constrain optimizer time, memory, and
  iterations with predefined profiles (interactive, standard, batch,
  memory-constrained) and custom limits
- **Plan Diff Visualization** -- Colorized structural diffs between
  original and optimized plans in four formats (colored, plain,
  side-by-side, compact)
- **Distributed Query Optimization** -- Network-aware cost modeling,
  broadcast/shuffle/co-located distribution strategies, two-phase and
  three-phase distributed aggregation, and federated query planning
  across multiple databases

## Quick Start

### Using Nix (Recommended)

```bash
nix develop
cargo build --release
cargo test --all-features
```

### Without Nix

Requirements: Rust 1.85+ with cargo

```bash
cargo build --release
cargo test --all-features
```

### CLI Usage

```bash
# Optimize a query
cargo run --bin ra-cli -- optimize "SELECT * FROM t1 WHERE x > 10"

# Explain optimization steps
cargo run --bin ra-cli -- explain "SELECT c.name FROM customers c JOIN orders o ON c.id = o.cid WHERE o.amount > 1000"

# Translate between SQL dialects
cargo run --bin ra-cli -- translate --from postgres --to mysql \
  "SELECT * FROM orders WHERE created_at > NOW() - INTERVAL '7 days'"

# View a colorized plan diff
cargo run --bin ra-cli -- optimize "SELECT * FROM t1 WHERE x > 10" --diff colored

# Optimize with a resource budget
cargo run --bin ra-cli -- optimize "SELECT * FROM orders JOIN customers ON orders.cid = customers.id" --resource-budget interactive

# List available rules
cargo run --bin ra-cli -- list

# Validate .rra rule files
cargo run --bin ra-cli -- validate rules/

# Run benchmarks
cargo bench --package ra-engine
```

See the [Getting Started guide](docs/getting-started.md) for a full
walkthrough of all features.

### Web Explorer

Run the interactive web explorer locally:

```bash
# Docker (simplest)
./scripts/docker-run.sh

# Docker Compose (better for development)
./scripts/docker-compose-up.sh

# Deploy to Fly.io cloud
./scripts/deploy-fly.sh
```

Then open http://localhost:8000 for local, or https://ra-explorer.fly.dev for cloud.

See [Deployment Guide](docs/deployment.md) for full details on Docker, Kubernetes, cloud providers, and production deployment.

## Project Structure

```
ra/
|---- crates/                  # Rust crates (31 crates)
|   |---- ra-core/             # Core types: RelExpr, Expr, Cost, Rule
|   |---- ra-parser/           # .rra literate format + SQL parser
|   |---- ra-compiler/         # Rule compilation and indexing
|   |---- ra-engine/           # Optimization engine (egg + differential)
|   |---- ra-cache/            # Plan cache with template matching
|   |---- ra-stats/            # Streaming statistics + monitoring adapters
|   |---- ra-codegen/          # Code generation (Cranelift, WASM, bytecode)
|   |---- ra-hardware/         # GPU/FPGA/SIMD/NUMA + network cost models
|   |---- ra-ml/               # ML cardinality estimation
|   |---- ra-adaptive/         # Runtime reoptimization
|   |---- ra-dialect/          # SQL dialect translation (20+ dialects)
|   |---- ra-pg-extension/     # PostgreSQL planner extension (pgrx)
|   |---- ra-pg-advisor/       # PostgreSQL query advisor daemon
|   |---- ra-pg-monitor/       # PostgreSQL monitoring and health checks
|   |---- ra-isolation/        # Cross-database isolation testing
|   |---- ra-wasm/             # WASM database adapters
|   |---- ra-synthesis/        # Natural language to SQL
|   |---- ra-discovery/        # Automatic rule mining from logs
|   |---- ra-multimodel/       # Graph, document, time-series rules
|   |---- ra-cli/              # Command-line interface
|   `---- ra-web/              # Web explorer backend (Rocket.rs)
|---- rules/                   # 1,327+ rule definitions (.rra files)
|   |---- logical/             # Predicate pushdown, join reordering, ...
|   |---- physical/            # Join algorithms, index selection, ...
|   |---- hardware/            # GPU, FPGA, SIMD, NUMA, data placement
|   |---- distributed/         # Exchange, broadcast join, partition pruning
|   |---- multi-model/         # Graph, document, time-series
|   `---- database-specific/   # Engine-specific optimizations
|---- benchmarks/              # JOB and TPC-H benchmark suites
|---- web/                     # Web explorer frontend (Preact)
|---- tla/                     # TLA+ formal specifications
|---- rfcs/                    # Design documents and proposals
|---- docs/                    # Documentation (VitePress)
`---- tests/                   # Integration and property tests
```

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

- [Documentation Index](docs/README.md) -- Full documentation map
- [Getting Started](docs/getting-started.md) -- Installation and all major features
- [Architecture](docs/architecture.md) -- Detailed component design
- [Benchmarks](docs/benchmarks.md) -- JOB and TPC-H benchmark results
- [PostgreSQL Extension](docs/postgresql-extension.md) -- Native PostgreSQL integration
- [Contributing](docs/CONTRIBUTING.md) -- Development standards and contribution guide
- [Rule Authoring Guide](docs/guides/rule-authoring.md) -- How to write `.rra` files
- [API Reference](docs/api-reference.md) -- Library API documentation
- [Cost Models](docs/guides/cost-models.md) -- Cost estimation framework
- [Hardware Acceleration](docs/features/hardware-acceleration.md) -- GPU/FPGA/SIMD rules
- [Execution Models](docs/features/execution-models.md) -- Volcano, vectorized, push-based, differential, column-at-a-time
- [Dialect Translation](docs/guides/dialect-translation.md) -- SQL cross-database translation
- [Isolation Testing](docs/features/isolation-testing.md) -- Transaction isolation verification
- [WASM Databases](docs/features/wasm-databases.md) -- Browser-based database execution
- [Formal Verification](docs/features/formal-verification.md) -- TLA+ specifications and verification approach
- [TLA+ Specifications](tla/README.md) -- Mathematical proofs of correctness properties
- [Resource Budgets](docs/features/resource-budgets.md) -- Predefined profiles, custom limits, and overflow strategies
- [Plan Visualization](docs/features/plan-visualization.md) -- Colorized plan diffs and output formats
- [Distributed Query Optimization](docs/features/distributed-optimization.md) -- Network costs, distribution strategies, aggregation, federated queries
- [Network Modeling](docs/features/network-modeling.md) -- Network topology and transfer cost estimation

### Examples

- [Simple Optimization](docs/examples/simple-optimization.md) -- Predicate pushdown walkthrough
- [Hardware-Aware Optimization](docs/examples/hardware-aware-optimization.md) -- CPU vs GPU operator placement
- [Distributed Join Strategies](docs/examples/distributed-join-strategies.md) -- Broadcast, shuffle, co-located joins

## Development

```bash
# Build all crates
cargo build

# Run all tests
cargo test --all-features

# Run linter (zero warnings required)
cargo clippy --all-targets --all-features -- -D warnings

# Format code
cargo fmt

# Run benchmarks
cargo bench

# Validate all rules
cargo run --bin ra-cli -- validate rules/

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
