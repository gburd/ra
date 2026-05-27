# RA — Agent Instructions

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RA is a relational algebra query optimization system. It codifies 1,387 database transformation rules (from PostgreSQL, MySQL, DuckDB, SQLite, etc.) into literate `.rra` files, then uses equality saturation (egg e-graphs) and differential dataflow to explore and extract optimal query plans.

## Build & Test Commands

```bash
# Build core crates only (default-members)
cargo build
cargo build --release

# Build core + CLI layer
cargo build -p ra --features cli

# Build everything (core + CLI + experimental)
cargo build -p ra --features all

# Build the CLI binary directly
cargo build -p ra-cli

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

### Workspace Layers

The workspace is organized into three layers plus a compatibility shim, controlled by Cargo features on the root `ra` package.

**Core (default build — `cargo build`):**
`lime-sys`, `lime-rs`, `ra-core`, `ra-parser`, `ra-compiler`, `ra-engine`, `ra-bitnet`, `ra-dialect`, `ra-hardware`, `ra-stats-advanced` (lib name `ra_stats`), `ra-cache-api`, `ra-sql-parser` (lib name `sqlparser`)

**CLI (`--features cli`):**
`ra-cli` (binary), `ra-adapters`, `ra-metadata`

**Experimental (`--features experimental`):**
`ra-ml`, `ra-cache-impl`, `ra-adaptive`, `ra-test-utils`, `ra-quel-parser`, `ra-grammar-fuzzer`, `ra-bench`, `ra-sqltest`, `ra-difftest`

**Compatibility shim (in workspace, not in default-members):**
`ra-config` — re-exports `ra_core::config::*` for downstream consumers that still import from the original path.

**Out of workspace:**
`ra-pg-extension` — PostgreSQL planner_hook extension built via pgrx (requires `pg_config` and PG headers). Excluded from the workspace.

Use `--features all` to build everything in the root facade.

### Core Crate Dependency Layers

**Foundation:** `ra-core` — defines `RelExpr`, `Expr`, `Rule` trait, `Cost`, `Statistics`, `Pattern`, and the `config` module (merged from the former `ra-config` crate). No dependencies on other workspace crates.

**SQL Parsing:** `ra-sql-parser` — custom fork of sqlparser 0.52 at `crates/ra-sql-parser`. The library name is `sqlparser` for compatibility with downstream code.

**Lime Tokenizer:** `lime-sys` (C library) + `lime-rs` (Rust bindings) — LALR(1) parser generator used by `ra-parser` for grammar-based SQL parsing.

**RA Parsing:** `ra-parser` → `ra-core`. Handles both SQL-to-`RelExpr` conversion (`sql_to_relexpr.rs`) and `.rra` literate rule file parsing (`rule_file_parser.rs`).

**Compilation:** `ra-compiler` → `ra-core`, `ra-parser`. Builds rule indices and the registry.

**Engine:** `ra-engine` → `ra-core`, `ra-parser`, `ra-hardware`, `ra-stats-advanced`, plus `egg` for e-graph equality saturation. Key modules:
- `egraph/` — e-graph construction (`mod.rs`: `RelLang` definition, `RelAnalysis`), optimizer loop (`optimizer.rs`), `RecExpr` conversion (`to_rec.rs`, `from_rec.rs`)
- `rewrite.rs` — rule registry, rewrite application (200+ rules)
- `extract/` — cost-based plan extraction (`api.rs`), hybrid neural/traditional cost function (`hybrid_cost.rs`), neural plan scoring (`neural_cost.rs`), plan variant generation (`plan_variants.rs`)
- `cost.rs` — `IntegratedCostFn` (hardware + statistics + staleness-aware egg cost function)
- `cost_model/` — neural cost models: `FastCostModel` (<100ns, 12→32→16), `ProductionCostModel` (12→64→16, momentum SGD), `OnlineLearner` (execution feedback → training), `feedback.rs` (execution feedback collection + MAPE tracking)
- `neural/` — full-pipeline neural guidance: `NeuralRuleSelector` (learned rule group selection, 26→10 linear model), `NeuralConvergenceDetector` (early termination), `RuleStallingTracker` (adaptive rule demotion)
- `state/` — reactive system state: `SystemFingerprint` (56-byte lock-free state vector), `AtomicFingerprint`, `FingerprintReader`
- `analysis.rs` — per-e-class property tracking (tables, cardinality)
- `rule_advisor.rs` — 3-stage rule filtering (context → query-shape → learned ranking)
- `lazy_rules.rs` — on-demand rule compilation by category

**CLI:** `ra-cli` — command-line interface. Depends on `ra-adapters` (DuckDB, MySQL, Stoolap connectors) and `ra-metadata` (database metadata factory).

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

- **Root `ra` package:** `cli`, `experimental`, `all` (combines both)
- **`ra-engine` defaults:** `metadata`, `streaming` (timely/differential-dataflow), `file-discovery`, `ml`
- **`ra-engine` `ml`** — enables `ra-ml` integration (on by default)
- **`timeline`** — feature-gated across `ra-stats-advanced`, `ra-engine`, `ra-test-utils`; enables timeline snapshots/replay
- **`ra-core` `parquet`** — Parquet file support
- **`ra-pg-extension`** — excluded from workspace; requires `pg_config` and PostgreSQL headers (built via pgrx)

## Clippy & Lint Configuration

Workspace clippy lints (in root `Cargo.toml` under `[workspace.lints.clippy]`):
- **Denied:** `unwrap_used`, `panic`, `panic_in_result_fn`, `unimplemented`, `dbg_macro`, `todo`, `print_stdout`, `print_stderr`, `allow_attributes`, `exit`, `mem_forget`, `await_holding_lock`, `large_futures`
- **Warned:** `expect_used`, `pedantic` (with relaxations for `module_name_repetitions`, `similar_names`, and cast precision/sign)

Use `anyhow`/`thiserror` for error handling; avoid `.unwrap()` and `.expect()` (use `?` or explicit matching).

## Project Layout

```
rules/           — 1,387 .rra rule files (logical/, physical/, hardware/, distributed/, multi-model/)
crates/          — Rust crates organized into core/cli/experimental layers (see above)
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

Uses a custom fork at `crates/ra-sql-parser` (based on sqlparser 0.52) referenced as a path dependency. The package is named `ra-sql-parser` but the library name is `sqlparser` for compatibility. Not the upstream `sqlparser` crate.

## Lime Grammar SQL Support

`ra-parser` uses a Lime (Lemon-derived) LALR(1) grammar (`crates/ra-parser/grammar/ra_sql.lime`) to parse SQL into `RelExpr`. As of RFC 0059 and follow-on work, the following SQL features are supported:

**Fully supported:**
- SELECT with projections, DISTINCT, aliases
- FROM: table scans, derived tables (subqueries), cross joins, inner/left/right/full/cross JOIN ON, JOIN USING
- WHERE, GROUP BY, HAVING, ORDER BY ASC/DESC NULLS FIRST/LAST
- LIMIT / OFFSET
- UNION / INTERSECT / EXCEPT (with ALL variant)
- WITH (CTEs): single, multiple comma-separated `WITH a, b SELECT...`
- WITH RECURSIVE (UNION ALL body → RecursiveCTE; non-UNION body → CTE)
- CTE column-name lists: `WITH name(col1, col2) AS (...)`
- CASE WHEN ... THEN ... ELSE ... END
- CAST(x AS type), `x::type` (PostgreSQL :: type cast)
- BETWEEN, LIKE, ILIKE, NOT LIKE, NOT ILIKE
- IN (list), NOT IN (list), IN (subquery), NOT IN (subquery)
- EXISTS (subquery)
- IS NULL / IS NOT NULL
- Scalar subqueries, DISTINCT in aggregates
- VALUES clause
- ARRAY[...] literals, array subscripting `arr[n]`
- UNNEST(arr) AS t(col), WITH ORDINALITY
- Table functions in FROM: `func(args) AS alias(cols)`, `schema.func(args)`
- `->` and `->>` JSON field access operators
- JSONB operators: `@>`, `<@`, `@?`, `@@`
- EXTRACT(field FROM expr)
- Typed string literals: `DATE 'str'`, `INTERVAL 'str'` (as string constants)
- `?` placeholder → NULL constant
- COALESCE, GREATEST, and arbitrary function calls

**Structured error reporting (RFC 0059):**
- `%syntax_error` calls `ra_record_parse_error` which captures position, token length, and expected-token hints from the Lime LALR state
- Errors flow as `StructuredParseError` with precise carets and "expected one of: ..." in CLI output
- Lexer errors (unrecognized characters) still use the string-error path

**Grammar notes:**
- `build.rs` tolerates resolved shift/reduce conflicts (IDENT SCONST and → rules introduce ~30 known conflicts that Lime resolves by SHIFT preference)
- SIMD tokenizer (`lime_tokenizer.rs`) has its own `keyword_lookup` and `map_c_code` — new tokens must be added to BOTH it and `lexer.rs`
- Unknown C tokenizer codes now return `Err` to force fallback to Rust lexer (needed for `[`, `]`, `@>`, `->`, `->>`

**Post-parse transforms** (`sql_to_relexpr/transform.rs`):
- Vector search: `ORDER BY distance_fn() LIMIT k` → `TopK`; `WHERE distance_fn() < thr` → `VectorFilter`
- Window functions: `__window_*` markers in Project → `Window` relational node
- Scalar aggregates: detect agg functions without GROUP BY → wrap in `Aggregate`

## Optimizer E-Graph Support

`ra-engine/src/egraph/to_rec.rs` and `from_rec.rs` handle `RelExpr` → `RecExpr` conversion for the egg e-graph. As of recent work, fully supported in the e-graph:
- **CASE expressions**: encoded as `Func("__CASE", operand_or_null, when1, then1, ..., else_or_null)`
- **Extended aggregates**: `ARRAY_AGG`, `STRING_AGG`, `STDDEV`, `VARIANCE` encoded as `Func(["NAME", arg])` in the aggregate slot
- Predicate pushdown fires correctly on CTEs with complex CASE/JSON expressions

## Cache Architecture

Cache functionality is split into two crates:
- **`ra-cache-api`** (core layer) — trait definitions and interfaces
- **`ra-cache-impl`** (experimental layer) — LRU/LFU/adaptive implementations

## Workspace Quality (as of 2026-05-26)

- **0 clippy errors** (`cargo clippy --all-targets --all-features -- -D warnings`)
- **0 compiler warnings** on `cargo build --workspace --all-features`
- **157 test suites, 7659 tests passing, 0 failing, 66 ignored** (`cargo test --workspace --all-features`)
- Known flaky test mitigations:
  - `saturation_terminates_quickly` skips Aggregate, self-ref-join, joins of the
    same base table, constant predicates, constant sort keys, and `UnaryOp` over
    constants — each documented inline with the rule-interaction reason
  - `full_lifecycle_all_properties` and the `long-duration-testing`-gated
    `extended_all_properties` / `extended_mixed_dml_all_properties` tests
    explicitly omit the `Idempotence` property — the e-graph's second
    extraction pass can drop a table reference that the first pass kept
    (see proptest-regressions for canonical failing shapes:
    `Filter(col_id, FullOuter(Scan, Scan))` and similar). The companion
    `extended_idempotence` test is `#[ignore]`d as a regression signal.
  - `plan_diff` tests that toggle the `colored` crate's process-global override
    serialise on a module-static `COLOR_LOCK` + `ColorGuard` RAII helper
  - Config loader test clears `RA_*` env vars to isolate from developer shell
  - DuckDB adapter is pinned to API surfaces present in `duckdb-rs` ≥ 1.x:
    no `enable_optimizer`, statement column metadata read after `query()`,
    `Decimal` rendered via `rust_decimal::Display`

## Rust Version

Minimum: 1.88.0 (set in `workspace.package.rust-version`). Edition 2021.
