# Relational Algebra Rule System

> The definitive open-source repository of relational algebra transformation rules

A comprehensive, formally verified system for database query optimization, built using literate programming, equality saturation, and differential dataflow.

## Vision

This project codifies decades of database optimization knowledge from academic research and production systems (PostgreSQL, MySQL, DuckDB, SQLite, and more) into a single, maintainable, formally verified system.

## Features

- **200+ Transformation Rules**: Comprehensive coverage from predicate pushdown to join reordering
- **Literate Programming**: Each rule is a documented `.rra` file with formal specifications
- **Equality Saturation**: Uses `egg` library for powerful e-graph-based optimization
- **Incremental Maintenance**: Differential dataflow for efficient rule updates
- **Multiple Backends**: Generate LLVM JIT, WASM, or bytecode
- **Formal Verification**: TLA+ specs and property-based testing
- **Web Explorer**: Interactive visualization and learning tool

## Quick Start

### Using Nix (Recommended)

```bash
# Enter development environment
nix develop

# Build all crates
cargo build

# Run tests
cargo test

# Validate rules
cargo run --bin ra-cli -- validate rules/

# Optimize a query
cargo run --bin ra-cli -- optimize "SELECT * FROM t1 WHERE x > 10"
```

### Without Nix

Requirements:
- Rust 1.75+ with cargo
- PostgreSQL, DuckDB, SQLite (for testing)
- TLA+ (for verification)

```bash
cargo build
cargo test
```

## Project Structure

```
ra/
├── crates/           # Rust crates
│   ├── ra-core/      # Core types and traits
│   ├── ra-parser/    # .rra format parser
│   ├── ra-compiler/  # Rule compilation
│   ├── ra-engine/    # Optimization engine (egg + differential)
│   ├── ra-codegen/   # Code generation (Cranelift, WASM)
│   ├── ra-cli/       # Command-line tool
│   └── ra-web/       # Web explorer backend
├── rules/            # Rule definitions (.rra files)
│   ├── logical/      # Logical transformations
│   ├── physical/     # Physical optimizations
│   └── database-specific/
├── web/              # Web explorer frontend (Preact)
├── tla/              # TLA+ formal specifications
├── docs/             # Documentation
└── tests/            # Integration and property tests
```

## Rule Format

Rules are written in `.rra` (Relational Rule Algebra) format, a literate markdown format:

```markdown
---
id: filter-through-join
name: Filter Pushdown Through Join
category: logical/predicate-pushdown
databases: [postgresql, mysql, duckdb]
---

# Filter Pushdown Through Join

## Description
Pushes selection predicates through join operators when the predicate only
references columns from one side of the join.

## Relational Algebra
σ[p](R ⋈[c] S) → (σ[p](R)) ⋈[c] S  where attrs(p) ⊆ attrs(R)

## Implementation
[Rust code using egg rewrite rules]

## Test Cases
[SQL examples demonstrating the transformation]
```

## Documentation

- [Architecture](docs/architecture.md) - System design and components
- [Rule Authoring Guide](docs/rule-authoring.md) - How to write `.rra` files
- [API Reference](docs/api-reference.md) - Library API documentation
- [Execution Models](docs/execution-models.md) - Query execution models (Volcano, Vectorized, Push-based, Differential, Column-at-a-time)
- [Examples](docs/examples/) - Usage examples

## Development

```bash
# Run tests with coverage
cargo test --all-features

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Format code
cargo fmt

# Run benchmarks
cargo bench

# Validate all rules
cargo run --bin ra-cli -- validate rules/

# Run property-based tests
cargo test --package ra-engine --test property_tests
```

## Contributing

We welcome contributions! Areas where help is needed:

1. **Rule Extraction**: Help extract rules from database source code
2. **Rule Writing**: Document existing optimizations in `.rra` format
3. **Testing**: Add test cases and property-based tests
4. **Verification**: Write TLA+ specifications
5. **Documentation**: Improve guides and examples

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License

This project is licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Acknowledgments

This project builds on decades of database research and open-source contributions:

- PostgreSQL optimizer team
- DuckDB developers
- Apache DataFusion community
- SQLite project
- Academic research from Selinger et al. (System R), Graefe, Volcano, and many others

## Status

🚧 **Under Active Development** - Phase 1 (Foundation) in progress

- [x] Repository structure
- [ ] Core types (ra-core)
- [ ] Rule parser (ra-parser)
- [ ] Initial 20 rules
- [ ] CLI tool
- [ ] CI/CD pipeline

See [ROADMAP.md](ROADMAP.md) for the full development plan.
