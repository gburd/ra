# RA — Relational Algebra Query Optimizer

## Purpose
Codifies 1,327+ database transformation rules (from PostgreSQL, MySQL, DuckDB, SQLite) into literate `.rra` files, then uses equality saturation (egg e-graphs) to explore and extract optimal query plans.

## Build & Test
```bash
cargo build                              # excludes ra-pg-extension
cargo test --all-features                # all tests
cargo test -p ra-engine                  # single crate
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt -- --check
nix develop                              # full dev environment
```

## Architecture
```
SQL → ra-parser → ra-compiler (loads .rra rules) → ra-engine (e-graph equality saturation) → optimized plan
```

**Crate layers:** `ra-core` (foundation) → `ra-parser` → `ra-compiler` → `ra-engine` → `ra-cli`

**Key types (ra-core):** `RelExpr` (relational AST), `Expr` (scalar), `Rule` trait, `Cost` (startup+total), `Statistics`, `Pattern`

## Layout
- `rules/` — 1,327+ .rra rule files
- `crates/` — 32+ Rust crates
- `docs/` — VitePress site
- `rfcs/` — design documents
- `tla/` — TLA+ formal specs

## Notes
- Custom SQL parser at `crates/ra-sql-parser` (path dependency, not upstream sqlparser)
- Minimum Rust 1.88.0, edition 2021
- Clippy: `unwrap_used`/`panic`/`todo` denied; `pedantic` warned
