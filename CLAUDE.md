# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RA is a relational algebra query optimization system. It codifies 1,327+ database transformation rules (from PostgreSQL, MySQL, DuckDB, SQLite, etc.) into literate `.rra` files, then uses equality saturation (egg e-graphs) and differential dataflow to explore and extract optimal query plans.

## Build & Test Commands

```bash
# Build (excludes ra-pg-extension which needs PostgreSQL headers)
cargo build
cargo build --release

# Run all tests
cargo test --all-features

# Run a single crate's tests
cargo test -p ra-engine
cargo test -p ra-parser

# Run a specific test
cargo test -p ra-engine test_filter_pushdown

# Run a specific integration test file
cargo test --test exasol_rules_test

# Lint (zero warnings required, CI enforced)
cargo clippy --all-targets --all-features -- -D warnings

# Format
cargo fmt

# Check formatting without modifying
cargo fmt -- --check

# Run benchmarks
cargo bench --package ra-engine

# Validate .rra rule files
cargo run --bin ra-cli -- validate rules/

# Build docs (VitePress)
cd docs && npx vitepress build
# Or: cargo xtask docs --serve

# Nix dev environment (provides all tooling)
nix develop
```

## Architecture

### Optimization Pipeline

```
SQL string
  → ra-parser (SQL → RelExpr AST)
  → ra-compiler (loads .rra rules into registry)
  → ra-engine (converts RelExpr → egg e-graph, runs equality saturation, extracts lowest-cost plan)
  → Optimized RelExpr
```

### Core Crate Dependency Layers

**Foundation:** `ra-core` — defines `RelExpr`, `Expr`, `Rule` trait, `Cost`, `Statistics`, `Pattern`; no dependencies on other workspace crates.

**Parsing:** `ra-parser` → `ra-core`. Handles both SQL-to-`RelExpr` conversion (`sql_to_relexpr.rs`, the largest file at ~102KB) and `.rra` literate rule file parsing (`rule_file_parser.rs`).

**Compilation:** `ra-compiler` → `ra-core`, `ra-parser`. Builds rule indices and the registry.

**Engine:** `ra-engine` → `ra-core`, `ra-parser`, plus `egg` for e-graph equality saturation. Key files:
- `egraph.rs` — e-graph construction, `RelLang` s-expression language definition, optimizer loop
- `rewrite.rs` — rule registry, rewrite application
- `extract.rs` — cost-based plan extraction
- `analysis.rs` — per-e-class property tracking (tables, cardinality)

**Interfaces:** `ra-cli`, `ra-web` (Rocket), `ra-tui` (ratatui), `ra-wasm` sit on top of everything.

### Key Types (all in `ra-core`)

- **`RelExpr`** — relational expression AST: `Scan`, `Filter`, `Project`, `Join`, `Aggregate`, `Sort`, `Limit`, `Union`, `CTE`, `RecursiveCTE`, `Window`, `Values`, etc.
- **`Expr`** — scalar expression: `Column`, `Const`, `BinOp`, `Function`, `Case`, `Cast`, `SubQuery`, etc.
- **`Rule` trait** — `metadata()`, `pattern()`, `matches(&RelExpr)`, `apply(&RelExpr) -> Option<RelExpr>`
- **`Cost`** — separates startup vs. total costs: `{cpu, io, network, memory, startup_cpu, startup_io, startup_network}` (follows PostgreSQL's approach for LIMIT optimization)
- **`Statistics`** — `row_count`, `avg_row_size`, `columns: HashMap<String, ColumnStats>` with histograms, NDV, MCV, correlation
- **`Pattern`** — rule matching patterns: `Wildcard`, `Scan`, `Filter`, `Join`, etc.

### .rra Rule Format

Literate markdown with YAML frontmatter:
```
---
id: rule-identifier
name: Human Name
category: logical/predicate-pushdown
databases: [postgresql, mysql, duckdb]
---
## Description  (prose)
## Relational Algebra  (formal notation)
## Implementation  (Rust code)
## Test Cases  (SQL examples)
## References  (citations)
```

Parsed by `ra-parser::rule_file_parser` into `RuleFile { metadata, description, algebra_notation, implementation, test_cases, references }`.

### Feature Flags

- **`ra-engine` defaults:** `metadata`, `streaming` (timely/differential-dataflow), `file-discovery`
- **`timeline`** — feature-gated across `ra-stats`, `ra-engine`, `ra-tui`, `ra-test-utils`; enables timeline snapshots/replay
- **`ra-core` `parquet`** — Parquet file support
- **`ra-pg-extension`** — excluded from workspace; requires `pg_config` and PostgreSQL headers (built via pgrx)

## Clippy & Lint Configuration

Workspace clippy lints (in root `Cargo.toml` under `[workspace.lints.clippy]`):
- **Denied:** `unwrap_used`, `panic`, `panic_in_result_fn`, `unimplemented`, `dbg_macro`, `todo`, `print_stdout`, `print_stderr`, `allow_attributes`, `exit`, `mem_forget`, `await_holding_lock`, `large_futures`
- **Warned:** `expect_used`, `pedantic` (with relaxations for `module_name_repetitions`, `similar_names`, and cast precision/sign)

Use `anyhow`/`thiserror` for error handling; avoid `.unwrap()` and `.expect()` (use `?` or explicit matching).

## Project Layout

```
rules/           — 1,327+ .rra rule files (logical/, physical/, hardware/, distributed/, multi-model/)
crates/          — 32+ Rust crates (see layers above)
tests/           — workspace-level integration tests
benchmarks/      — JOB and TPC-H benchmark suites
docs/            — VitePress documentation site (Node.js 20, npm)
web/             — Preact web explorer frontend
tla/             — TLA+ formal verification specs
rfcs/            — design documents (RFC process for major features)
scripts/         — shell utilities (docker, validation, benchmarks, TLA+)
xtask/           — cargo xtask build automation (docs build/serve)
```

## SQL Parser

Uses a custom fork at `crates/sqlparser-ra` (based on sqlparser 0.52) referenced as a path dependency. Not the upstream `sqlparser` crate.

## Rust Version

Minimum: 1.88.0 (set in `workspace.package.rust-version`). Edition 2021.
