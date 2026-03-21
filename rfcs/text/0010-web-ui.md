# RFC 0010: Web-Based Query Comparison UI

- **Status:** Implemented
- **Type:** Retroactive
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

A browser-based interactive UI (inspired by Compiler Explorer /
Godbolt) for entering SQL queries, viewing optimizer transformations
in real time, comparing plans across databases and hardware
configurations, and running interactive demonstrations.

## Motivation

Command-line output is insufficient for understanding complex plan
transformations. A visual interface allows:

- Side-by-side comparison of original and optimized plans
- Interactive exploration of rule application order
- Demonstrations of how statistics and hardware affect plan choice
- Educational use in database courses

The "Godbolt for SQL optimization" concept makes the optimizer
accessible to users who are not comfortable with CLI tools.

## What Was Built

### Core Features

- **Query editor** with SQL syntax highlighting
- **Plan visualization** with tree layout and diff highlighting
- **Rule browser** showing which rules fired and their effects
- **Database selector** to compare plans across PostgreSQL, MySQL,
  SQLite, DuckDB
- **Hardware selector** to see how plans change across 12+ hardware
  profiles

### Interactive Demonstrations

10 built-in demonstrations (documented in `docs/demonstrations.md`):

1. Statistics staleness impact
2. Hardware-specific plans
3. Join algorithm selection
4. Aggregation strategy selection
5. Index selection
6. Predicate pushdown visualization
7. Join reordering
8. Subquery unnesting
9. CTE optimization
10. Distributed query planning

Each demo has interactive controls (sliders, toggles, dropdowns)
that update the plan visualization in real time.

### Technology Stack

- TypeScript frontend
- WASM-compiled RA optimizer (RFC 0009)
- WASM databases for live query execution
- No server backend required (static site deployment)

### Plan Diff Formats

Four diff modes match the CLI (RFC-independent feature):

- Colored inline diff
- Plain text diff
- Side-by-side comparison
- Compact summary

## Key Design Decisions

- Static site architecture chosen for zero-infrastructure deployment
  (GitHub Pages, Netlify, etc.)
- WASM compilation of the Rust optimizer enables running the full
  rule set in the browser
- Demonstrations use a TypeScript simulation of cost models for
  responsiveness, with optional WASM validation
- Plan diff is shared between CLI and web UI to avoid divergence

## Prior Art

- Compiler Explorer (godbolt.org) -- inspiration for the UI concept
- pgMustard -- PostgreSQL EXPLAIN visualization
- Dalibo's explain.depesz.com -- PostgreSQL plan analysis
- Apache Calcite's web-based rule explorer

## References

- `docs/demonstrations.md` -- interactive demo documentation
- `docs/plan-visualization.md` -- plan diff format documentation
- `web/` -- frontend source code
- `crates/ra-wasm/` -- WASM compilation target
