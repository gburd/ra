# Maintainer's Guide

Welcome to the RA optimizer maintainer documentation. This section is for contributors who want to understand how RA works internally, build it from source, and contribute code.

## Quick Links

- **[Build & Install](./build.md)** - Compile from source, run tests
- **[CLI Reference](./cli-reference.md)** - Complete ra-cli command reference
- **[Component APIs](./components.md)** - How major subsystems interact
- **[RFCs](./rfcs/)** - Formal proposals for major features
- **[Chores & Tasks](./chores.md)** - Small tasks organized by priority
- **[Bugs & Issues](./bugs.md)** - Bug tracking and resolution
- **[Release Process](./release.md)** - How to cut a new release

## Project Structure

```
ra/
|---- crates/              # Rust crates (20+ components)
|   |---- ra-core/         # Core types (RelExpr, Statistics)
|   |---- ra-engine/       # Optimization engine (e-graph)
|   |---- ra-parser/       # SQL parser
|   |---- ra-dialect/      # Multi-database SQL translation
|   |---- ra-stats/        # Statistics system
|   |---- ra-hardware/     # Hardware detection
|   |---- ra-cli/          # Command-line interface
|   |---- ra-tui/          # Terminal UI
|   |---- ra-web/          # Web UI
|   |---- ra-pg-extension/ # PostgreSQL pgrx extension
|   `---- ...
|---- rules/               # 1,354+ optimization rules
|---- docs/                # Documentation (VitePress)
|---- rfcs/                # Request for Comments (45+ RFCs)
|---- research/            # Research notes and gap analysis
|---- examples/            # Example queries and applications
`---- tests/               # Integration and regression tests
```

## Major Components

### Core Library (`ra-core`)
- **RelExpr**: Relational algebra expression tree
- **Statistics**: Table and column statistics
- **Facts**: Database metadata (indexes, constraints)
- **Algebra operations**: Visitors, transformers, validators

### Optimization Engine (`ra-engine`)
- **e-graph**: Equality saturation for query optimization
- **Cost model**: Estimates query plan cost
- **Rule application**: Applies 1,354+ transformation rules
- **Plan extraction**: Extracts lowest-cost plan from e-graph

### SQL Parser (`ra-parser`)
- **Multi-dialect parsing**: PostgreSQL, MySQL, SQLite, etc.
- **Query to RelExpr**: Converts SQL to relational algebra
- **Test case parser**: Parses `.rra` rule test files

### Statistics System (`ra-stats`)
- **Timeline support**: Statistics evolution over time
- **Interpolation**: Estimates statistics between snapshots
- **Formats**: TOML, JSON, binary

### Hardware Detection (`ra-hardware`)
- **Auto-detection**: CPU, memory, storage type
- **Profiles**: Laptop, server, cloud VM, custom
- **Cost calibration**: Adjusts costs based on hardware

## Development Workflow

1. **Clone repository**
   ```bash
   git clone https://codeberg.org/gregburd/ra.git
   cd ra
   ```

2. **Build from source**
   ```bash
   cargo build --release
   cargo test
   ```

3. **Run CLI**
   ```bash
   cargo run --bin ra-cli -- optimize "SELECT * FROM users WHERE age > 18"
   ```

4. **Run web UI**
   ```bash
   cd web && npm install && npm run dev
   ```

5. **Run TUI**
   ```bash
   cargo run --bin ra-tui
   ```

## Contributing

See [CONTRIBUTING.md](../../CONTRIBUTING.md) for detailed contribution guidelines.

**Quick summary:**
1. Find an issue or chore to work on
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Make changes, add tests
4. Run tests: `cargo test`
5. Commit: `git commit -m "feat: Add feature X"`
6. Push and create PR

## Getting Help

- **Documentation**: [docs.ra-optimizer.org](https://docs.ra-optimizer.org)
- **Issues**: [Codeberg Issues](https://codeberg.org/gregburd/ra/issues)
- **Discussions**: [Codeberg Discussions](https://codeberg.org/gregburd/ra/discussions)

## Next Steps

- **New to the codebase?** Start with [Build & Install](./build.md)
- **Want to add a feature?** Check [RFCs](./rfcs/) and [Chores](./chores.md)
- **Found a bug?** See [Bugs & Issues](./bugs.md)
- **Ready to contribute?** Read [Component APIs](./components.md)
