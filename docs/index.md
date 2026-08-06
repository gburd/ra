---
layout: home

hero:
  name: "Ra"
  text: "Query Optimizer (research prototype)"
  tagline: "Experimental parser, planner, and optimizer for PostgreSQL"
  actions:
    - theme: brand
      text: Get Started
      link: /getting-started
    - theme: alt
      text: View on Codeberg
      link: https://codeberg.org/gregburd/ra

features:
  - icon: ⚡
    title: Rule Library
    details: 1,451 .rra rule sources, 277 currently active, covering logical and physical optimizations

  - icon: 🔄
    title: 20+ Database Dialects
    details: Seamless SQL translation between PostgreSQL, MySQL, Oracle, SQL Server, SQLite, DuckDB, and more

  - icon: 🎯
    title: Hardware-Aware Optimization
    details: Adaptive plans for CPU (SIMD), GPU, FPGA, and heterogeneous systems with cost-based decisions

  - icon: 📊
    title: Cost-Based Optimization
    details: Calibratable cost models with cardinality estimation and statistics management

  - icon: 🧬
    title: Equality Saturation
    details: Explores all equivalent plans simultaneously via e-graphs to find the optimal execution strategy

  - icon: 🚀
    title: Performance Shortcuts
    details: MIN/MAX metadata lookups, COUNT(*) shortcuts, covering indexes, and bitmap scans

  - icon: 🌐
    title: Distributed Execution
    details: Partition-aware optimization, co-location awareness, and minimal data movement across nodes

  - icon: 📁
    title: Columnar Format Support
    details: Parquet predicate pushdown, row group filtering, and column pruning for analytical workloads

  - icon: 🔬
    title: Formal Verification
    details: Mathematically proven correctness of transformation rules using SMT solvers
---

## Quick Example

Transform and optimize your SQL queries:

```bash
# Optimize a query
ra optimize \
  "SELECT * FROM orders WHERE amount > 1000 AND status = 'active'"

# Dialect translation moved to a separate repo: https://codeberg.org/gregburd/ra-lab
```

## Architecture Highlights

- **26 Rust crates** with clear separation of concerns
- **Literate rules** in `.rra` format combining metadata, docs, algebra, and tests
- **Differential dataflow** for incremental computation
- **Property testing** via quickcheck for correctness verification
- **SMT integration** using Z3 for formal rule verification

## Performance

No end-to-end performance comparison against native PostgreSQL is published
yet. Planning-time-only speedups were removed because they were measured with
statistics disabled on the RA side and did not measure plan quality. End-to-end
(plan + execute) numbers will replace this section once correctness parity is
reached.

## Recent Additions

Five major RFCs recently implemented:

- **RFC 0051**: Materialized View Matching and Rewriting
- **RFC 0052**: Progressive Re-Optimization
- **RFC 0058**: Isolation-Aware Query Planning
- **RFC 0059**: Bayesian Adaptive Search Space Pruning
- **RFC 0060**: Genetic Query Fingerprinting and Plan Cache

See [RFCs Index](/maintainers/rfcs/) for details.
