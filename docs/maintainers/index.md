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

<script setup>
const CB = 'https://codeberg.org/gregburd/ra/src/tag/v0.1.0'

const treeData = [
  {
    name: 'crates',
    desc: 'Rust workspace crates (31 components)',
    href: `${CB}/crates`,
    children: [
      { name: 'ra-core', cat: 'core', desc: 'Core types: RelExpr, Statistics, Facts', href: `${CB}/crates/ra-core`, anchor: '#ra-core' },
      { name: 'ra-engine', cat: 'engine', desc: 'Optimization engine: e-graph, cost model, rule application', href: `${CB}/crates/ra-engine`, anchor: '#ra-engine' },
      { name: 'ra-parser', cat: 'parser', desc: 'SQL parser: multi-dialect parsing, query to RelExpr', href: `${CB}/crates/ra-parser`, anchor: '#ra-parser' },
      { name: 'ra-compiler', cat: 'parser', desc: 'SQL-to-RelExpr compiler pipeline', href: `${CB}/crates/ra-compiler` },
      { name: 'ra-dialect', cat: 'parser', desc: 'SQL dialect translation for cross-database compatibility', href: `${CB}/crates/ra-dialect`, anchor: '#ra-dialect' },
      { name: 'ra-stats', cat: 'stats', desc: 'Statistics system: timeline, interpolation, staleness modeling', href: `${CB}/crates/ra-stats`, anchor: '#ra-stats' },
      { name: 'ra-hardware', cat: 'hardware', desc: 'Hardware detection: GPU/FPGA/SIMD cost models', href: `${CB}/crates/ra-hardware`, anchor: '#ra-hardware' },
      { name: 'ra-config', cat: 'config', desc: 'Configuration management (TOML-based)', href: `${CB}/crates/ra-config` },
      { name: 'ra-metadata', cat: 'catalog', desc: 'Database metadata and schema information', href: `${CB}/crates/ra-metadata` },
      { name: 'ra-catalog', cat: 'catalog', desc: 'Function catalog for SQL query optimization', href: `${CB}/crates/ra-catalog` },
      { name: 'ra-adapters', cat: 'adapter', desc: 'Database adapter backends (PostgreSQL, Stoolap)', href: `${CB}/crates/ra-adapters` },
      { name: 'ra-adaptive', cat: 'engine', desc: 'Adaptive query execution with runtime reoptimization', href: `${CB}/crates/ra-adaptive` },
      { name: 'ra-advisor', cat: 'engine', desc: 'Automatic index advisor for workload analysis', href: `${CB}/crates/ra-advisor` },
      { name: 'ra-cache', cat: 'engine', desc: 'Plan cache with LRU/LFU/adaptive eviction', href: `${CB}/crates/ra-cache` },
      { name: 'ra-codegen', cat: 'engine', desc: 'Cranelift-based code generation', href: `${CB}/crates/ra-codegen` },
      { name: 'ra-discovery', cat: 'engine', desc: 'Automatic rule discovery from execution logs', href: `${CB}/crates/ra-discovery` },
      { name: 'ra-isolation', cat: 'test', desc: 'Cross-database isolation testing framework', href: `${CB}/crates/ra-isolation` },
      { name: 'ra-ml', cat: 'ml', desc: 'ML-based cardinality estimation', href: `${CB}/crates/ra-ml` },
      { name: 'ra-multimodel', cat: 'engine', desc: 'Multi-model optimization (graph, document, time-series)', href: `${CB}/crates/ra-multimodel` },
      { name: 'ra-synthesis', cat: 'engine', desc: 'Natural language to relational algebra synthesis', href: `${CB}/crates/ra-synthesis` },
      { name: 'ra-cli', cat: 'ui', desc: 'Command-line interface', href: `${CB}/crates/ra-cli` },
      { name: 'ra-tui', cat: 'ui', desc: 'Terminal UI', href: `${CB}/crates/ra-tui` },
      { name: 'ra-web', cat: 'ui', desc: 'Web UI server', href: `${CB}/crates/ra-web` },
      { name: 'ra-pg-extension', cat: 'pg', desc: 'PostgreSQL pgrx extension', href: `${CB}/crates/ra-pg-extension` },
      { name: 'ra-pg-advisor', cat: 'pg', desc: 'PostgreSQL plan advisor (pg_plan_advice hints)', href: `${CB}/crates/ra-pg-advisor` },
      { name: 'ra-pg-monitor', cat: 'pg', desc: 'PostgreSQL monitoring and schema analysis', href: `${CB}/crates/ra-pg-monitor` },
      { name: 'ra-wasm', cat: 'wasm', desc: 'WASM database adapters for browser-based SQL', href: `${CB}/crates/ra-wasm` },
      { name: 'ra-wasm-docs', cat: 'wasm', desc: 'WASM wrapper for documentation interactive examples', href: `${CB}/crates/ra-wasm-docs` },
      { name: 'ra-regression', cat: 'test', desc: 'Regression testing against DataFusion and SQLite', href: `${CB}/crates/ra-regression` },
      { name: 'ra-test-utils', cat: 'test', desc: 'Shared test utilities and fixtures', href: `${CB}/crates/ra-test-utils` },
      { name: 'sparsemap', cat: 'core', desc: 'Sparse bitmap for efficient set operations', href: `${CB}/crates/sparsemap` },
    ]
  },
  { name: 'rules', desc: '1,354+ optimization rules (cost-models, database-specific, distributed, ...)', href: `${CB}/rules`, children: [] },
  { name: 'rfcs', desc: 'Request for Comments (45+ design proposals)', href: `${CB}/rfcs`, children: [] },
  { name: 'docs', desc: 'Documentation site (VitePress)', href: `${CB}/docs`, children: [] },
  { name: 'tests', desc: 'Integration, isolation, and real-world query tests', href: `${CB}/tests`, children: [] },
  { name: 'research', desc: 'Research notes and gap analysis', href: `${CB}/research`, children: [] },
  { name: 'benchmarks', desc: 'Performance benchmarks', href: `${CB}/benchmarks`, children: [] },
  { name: 'web', desc: 'Web UI frontend (separate from ra-web server)', href: `${CB}/web`, children: [] },
  { name: 'scripts', desc: 'Build and maintenance scripts', href: `${CB}/scripts`, children: [] },
  { name: 'timelines', desc: 'Statistics timeline data files', href: `${CB}/timelines`, children: [] },
  { name: 'tla', desc: 'TLA+ formal specifications', href: `${CB}/tla`, children: [] },
  { name: 'xtask', desc: 'Cargo xtask automation', href: `${CB}/xtask`, children: [] },
]
</script>

<ProjectTree :data="treeData" />

## Major Components {#major-components}

### Core Library (`ra-core`) {#ra-core}
- **RelExpr**: Relational algebra expression tree
- **Statistics**: Table and column statistics
- **Facts**: Database metadata (indexes, constraints)
- **Algebra operations**: Visitors, transformers, validators

### Optimization Engine (`ra-engine`) {#ra-engine}
- **e-graph**: Equality saturation for query optimization
- **Cost model**: Estimates query plan cost
- **Rule application**: Applies 1,354+ transformation rules
- **Plan extraction**: Extracts lowest-cost plan from e-graph

### SQL Parser (`ra-parser`) {#ra-parser}
- **Multi-dialect parsing**: PostgreSQL, MySQL, SQLite, etc.
- **Query to RelExpr**: Converts SQL to relational algebra
- **Test case parser**: Parses `.rra` rule test files

### Dialect Translation (`ra-dialect`) {#ra-dialect}
- **Cross-database SQL**: Translates between PostgreSQL, MySQL, SQLite, etc.
- **Syntax normalization**: Unifies dialect-specific syntax
- **Output generation**: Produces target-dialect SQL from RelExpr

### Statistics System (`ra-stats`) {#ra-stats}
- **Timeline support**: Statistics evolution over time
- **Interpolation**: Estimates statistics between snapshots
- **Formats**: TOML, JSON, binary

### Hardware Detection (`ra-hardware`) {#ra-hardware}
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
   ra-cli optimize "SELECT * FROM users WHERE age > 18"
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

See [contributing.md](../contributing.md) for detailed contribution guidelines.

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
