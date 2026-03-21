# RFC 0002: pgrx PostgreSQL Extension

- **Status:** Accepted
- **Type:** Prospective
- **Author:** RA Contributors
- **Date:** 2026-03-20
- **Tracking:** Phase 4 of deployment plan

---

## Summary

Build a native PostgreSQL extension using the pgrx framework that
embeds the RA optimizer directly inside PostgreSQL. The extension
intercepts query plans via the planner hook, applies RA optimization
rules, and returns improved plans to the executor.

## Motivation

The RA optimizer currently runs as an external tool. Users must
manually export queries, run optimization, and interpret results.
A PostgreSQL extension eliminates this friction by optimizing queries
transparently at plan time.

PostgreSQL's planner hook API allows extensions to replace or augment
the built-in optimizer. By using pgrx (a Rust framework for writing
PostgreSQL extensions), we can reuse the existing Rust codebase
without rewriting in C.

Target use cases:

- Transparent query optimization for production PostgreSQL workloads
- A/B testing RA plans against PostgreSQL's native optimizer
- Collecting execution feedback to calibrate RA cost models

## Guide-Level Explanation

After installing the extension:

```sql
CREATE EXTENSION ra_planner;

-- Enable for the current session
SET ra_planner.enabled = on;

-- Run a query; RA optimizes it transparently
EXPLAIN SELECT * FROM orders JOIN customers USING (customer_id)
  WHERE order_date > '2025-01-01';
```

The extension exposes GUC variables for configuration:

- `ra_planner.enabled` -- toggle optimization (default: off)
- `ra_planner.log_level` -- control logging verbosity
- `ra_planner.rules` -- comma-separated list of rule categories
- `ra_planner.cost_threshold` -- minimum cost improvement to apply

A `ra_planner.explain()` function returns the RA plan tree alongside
the PostgreSQL plan for comparison.

## Reference-Level Explanation

### Crate Structure

A new crate `crates/ra-pgrx/` depends on `pgrx` and links against
`ra-core`, `ra-engine`, `ra-parser`, and `ra-dialect`.

### Planner Hook

```rust
#[pg_guard]
pub unsafe extern "C" fn ra_planner_hook(
    parse: *mut Query,
    query_string: *const c_char,
    cursor_options: c_int,
    bound_params: ParamListInfo,
) -> *mut PlannedStmt {
    // 1. Convert PG parse tree to RA RelExpr
    // 2. Run optimizer with configured rules
    // 3. Convert optimized RelExpr back to PG plan
    // 4. If cost improved beyond threshold, return RA plan
    // 5. Otherwise fall through to standard_planner
}
```

### Plan Conversion

The `ra-dialect` crate already supports PostgreSQL SQL. The extension
adds bidirectional conversion between PostgreSQL's internal `Plan`
nodes and RA's `RelExpr`:

- `pg_plan_to_relexpr()` -- extracts the logical plan
- `relexpr_to_pg_plan()` -- reconstructs physical plan nodes

### Feedback Loop

After query execution, the extension captures `EXPLAIN ANALYZE`
data and feeds it back to calibrate the RA cost model:

- Actual vs estimated row counts
- Actual vs estimated execution time
- Buffer hit/miss ratios

### Build and Packaging

- Build with `cargo pgrx package`
- Targets PostgreSQL 15, 16, 17
- RPM/DEB packaging via CI
- Dockerized test environment with `pg_regress`

## Drawbacks

- pgrx adds a build dependency on the PostgreSQL development headers
- Plan conversion between PG internal nodes and RA is fragile across
  PG major versions
- Incorrect plan conversion could cause query failures in production
- Extension must be installed with superuser privileges

## Rationale and Alternatives

**Alternative: FDW-based approach.** A Foreign Data Wrapper could
proxy queries through RA. This avoids the planner hook but adds
latency and cannot optimize plans for local tables.

**Alternative: External advisor only.** Keep RA as a standalone tool
and output `pg_hint_plan` hints. Simpler but requires manual
intervention.

The planner hook approach was chosen because it provides transparent
optimization with the lowest user friction.

## Prior Art

- **pg_hint_plan** -- injects optimizer hints; does not replace the
  planner but demonstrates the extension pattern
- **Citus** -- uses planner hooks for distributed query planning
- **pg_plan_advice** -- PostgreSQL v19 plan advice mechanism for
  supplying external optimizer hints
- **Apache Calcite** -- external optimizer used by Hive, Drill, and
  Flink; similar concept but as a separate service

## Unresolved Questions

- Which PostgreSQL versions to support at launch (15+ or 16+ only)?
- How to handle plan conversion for PostgreSQL-specific features
  not modeled in RA (e.g., custom scan providers)?
- Should the extension support read-only mode (advise but don't
  replace plans)?

## Future Possibilities

- Integration with `pg_plan_advice` (RFC 0003) for hint-based
  optimization instead of full plan replacement
- Workload-level optimization across multiple queries
- Automatic index recommendation based on observed query patterns
- Extension marketplace distribution via PGXN or trunk
