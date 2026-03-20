# RA Documentation

RA is a query optimizer built on relational algebra transformation
rules, equality saturation, and differential dataflow. It codifies
decades of database optimization knowledge from academic research and
production systems into a single, formally verified framework.

## At a Glance

- **1,350+ transformation rules** across logical, physical, hardware,
  distributed, and multi-model categories
- **Equality saturation** via `egg` e-graphs -- explores all
  equivalent plans simultaneously
- **26 Rust crates** covering parsing, compilation, optimization,
  code generation, dialect translation, and more
- **Literate rule format** (`.rra`) combining YAML metadata, markdown
  documentation, formal algebra, and test cases

## Documentation Map

### Getting Started

- [Getting Started](GETTING_STARTED.md) -- Installation, first
  optimization, understanding output
- [Architecture](architecture.md) -- System components and data flow
- [Contributing](CONTRIBUTING.md) -- Development standards and how to
  contribute

### Guides

- [Rule Authoring](guides/rule-authoring.md) -- Writing `.rra`
  transformation rules
- [Optimization](guides/optimization.md) -- Using the optimizer
  effectively
- [Dialect Translation](guides/dialect-translation.md) -- SQL
  cross-database translation
- [Cost Models](guides/cost-models.md) -- Cost estimation framework
- [Testing](guides/testing.md) -- Running and writing tests

### Concepts

- [Relational Algebra](concepts/relational-algebra.md) -- RA
  fundamentals and notation
- [Pre-Conditions](concepts/pre-conditions.md) -- Rule applicability
  system
- [Facts Provider](concepts/facts-provider.md) -- Unified system
  facts interface
- [Rule Categories](concepts/rule-categories.md) -- Taxonomy of
  transformation rules

### Features

- [Hardware Acceleration](features/hardware-acceleration.md) --
  GPU/FPGA/SIMD/NUMA rules
- [Distributed Optimization](features/distributed-optimization.md) --
  Network-aware query planning
- [Adaptive Execution](features/adaptive-execution.md) -- Runtime
  reoptimization
- [Plan Visualization](features/plan-visualization.md) -- Colorized
  plan diffs
- [Resource Budgets](features/resource-budgets.md) -- Constraining
  optimizer resources
- [Formal Verification](features/formal-verification.md) -- TLA+
  specifications
- [Execution Models](features/execution-models.md) -- Volcano,
  vectorized, push-based
- [Multi-Model](features/multi-model-optimization.md) -- Graph,
  document, time-series
- [WASM Databases](features/wasm-databases.md) -- Browser-based
  database execution
- [ML Cardinality](features/ml-cardinality.md) -- Neural network cost
  estimation

### Integrations

- [PostgreSQL](integrations/postgresql.md) -- PostgreSQL integration
- [Database Adapters](integrations/database-adapters.md) -- Metadata
  and schema integration

### Reference

- [API Reference](api-reference.md) -- Library API documentation
- [SQL Coverage](sql-coverage.md) -- Supported SQL features
- [Deployment](deployment.md) -- Docker, Kubernetes, cloud deployment

### Examples

- [Simple Optimization](examples/simple-optimization.md)
- [Hardware-Aware Optimization](examples/hardware-aware-optimization.md)
- [Distributed Join Strategies](examples/distributed-join-strategies.md)
- [Subquery Unnesting](examples/subquery-unnesting.md)

### Research

- [Research Paper](research/paper.md)
