---
layout: home

hero:
  name: "Ra"
  text: "Query Optimizer"
  tagline: "1,327+ transformation rules for optimal SQL execution plans"
  actions:
    - theme: brand
      text: Get Started
      link: /getting-started
    - theme: alt
      text: View on Codeberg
      link: https://codeberg.org/gregburd/ra

features:
  - icon: ⚡
    title: 1,327+ Transformation Rules
    details: Comprehensive rule library covering logical, physical, hardware, distributed, and multi-model optimizations

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
ra-cli optimize \
  "SELECT * FROM orders WHERE amount > 1000 AND status = 'active'"

# Translate between databases
ra-cli translate --from postgres --to mysql \
  "SELECT * FROM orders WHERE created_at > NOW() - INTERVAL '7 days'"
```

## Architecture Highlights

- **26 Rust crates** with clear separation of concerns
- **Literate rules** in `.rra` format combining metadata, docs, algebra, and tests
- **Differential dataflow** for incremental computation
- **Property testing** via quickcheck for correctness verification
- **SMT integration** using Z3 for formal rule verification

## Performance Highlights

- Up to **1000x speedup** on complex analytical queries
- **85% I/O reduction** with automatic covering index detection
- **O(1)** MIN/MAX/COUNT operations on billion-row tables
- **95% data skip** with Parquet row group filtering
- **10-100x improvement** on star schema queries with join reordering

## Recent Additions

Five major RFCs recently implemented:

- **RFC 0051**: Materialized View Matching and Rewriting
- **RFC 0052**: Progressive Re-Optimization
- **RFC 0058**: Isolation-Aware Query Planning
- **RFC 0059**: Bayesian Adaptive Search Space Pruning
- **RFC 0060**: Genetic Query Fingerprinting and Plan Cache

See [RFCs Index](/maintainers/rfcs/) for details.
