# Ra Evaluation — 2026-05-29

This records what was measured, the hard blocker that prevents the
requested PostgreSQL 19-devel comparison, and the realistic path
to the full comparison. **No results here are fabricated**; every
number is from an actual run, and the comparisons that could not
be run are explicitly marked as not-run.

## Hard blocker: Ra cannot run inside PostgreSQL 19-devel

The request is to run Ra as the parser/planner/optimizer inside a
patched PostgreSQL 19-devel (today's HEAD, `878839ba…`). This is
**not possible today**:

- Ra's PostgreSQL extension (`crates/ra-pg-extension`) is built
  with **pgrx `=0.17.0`**, whose feature set is **pg13 … pg18
  only**. There is no `pg19` feature, because PG 19 is unreleased
  and its catalog/struct layout (the ABI pgrx binds to) is not
  stable on HEAD.
- The alternative C-ABI hook path (`patches/postgres-ra-hook.patch`
  + `include/ra_planner_hook.h`) does not bypass this: the Rust
  side still uses `pgrx::pg_sys` types throughout (only
  `ra_rust_invalidate_table` is a bare `extern "C"` callback).

So hosting Ra in PG 19-devel requires (a) pgrx to add pg19
support upstream — which won't happen until PG 19 stabilizes —
and (b) porting Ra to that pgrx. Neither exists now.

**Achievable substitute:** PostgreSQL **18** is the highest major
pgrx 0.17 supports and is what `ra-pg-extension` already targets
(`--features pg18`). A full patched-vs-unpatched-vs-GEQO
comparison is feasible on PG 18 but requires a multi-hour build
(see "Path to the full comparison"). This substitution should be
confirmed before spending the build time.

## What was measured (real, today)

Ra-side parse + optimize coverage and planning effort over the
120-query planner-comparison suite (9 categories), via
`cargo run --release --bin planner_comparison_runner`.

### Coverage

| Dimension | Result |
|-----------|--------|
| Parsed | **116 / 120 (96.7%)** |
| Optimized (of parsed) | **114 / 120 (98.3%)** |

All 8 mainstream categories — simple, basic_joins, complex_joins,
aggregations, subqueries, ctes, set_operations, advanced — are
**100% parsed and optimized**. Every failure is in the
intentionally-hard `unsupported` category:

| Query | Failure |
|-------|---------|
| PIVOT | parse: grammar has no `PIVOT` |
| XMLTABLE | parse: `PASSING` not in grammar |
| MERGE | parse: `MERGE` not in grammar |
| MATCH_RECOGNIZE | parse: `{` quantifier unhandled |
| multi-table UPDATE | optimize: no plan extracted from e-graph |
| multi-table DELETE | optimize: **"Subquery expressions are not yet supported in the e-graph representation"** |

### Planning effort / speed (Ra-side only)

Median plan time is **~1.7–2.0 ms** across categories, but the
**tail is the story**:

| Query | Plan time |
|-------|-----------|
| ctes_01_simple | **234 ms** |
| complex_joins_20_diamond | 73 ms |
| complex_joins_03_six_table | 72 ms |
| complex_joins_02_snowflake | 61 ms |
| complex_joins_12_five_table_agg | 58 ms |
| complex_joins_01_star_schema | 57 ms |
| subqueries_20_anti_join_complex | 52 ms |
| complex_joins_19_seven_table | 19 ms |
| (everything else) | ≤ ~2.4 ms |

There is a sharp cliff: ~9 queries take 18–234 ms; the other ~111
take under 2.5 ms.

## Where Ra is good

- **Mainstream SQL coverage is strong**: 100% parse+optimize on
  simple/joins/aggregations/subqueries/CTEs/set-ops/advanced.
- **Median planning is fast** (~1.7–2 ms in release) — consistent
  with the project's headline for small queries.
- **Graceful degradation is now correct** (commit `77ccb014`):
  any parse/optimize/build failure falls back to PG's planner
  rather than failing the query.

## Where Ra needs work

1. **E-graph saturation blowup is the #1 performance issue.**
   Complex multi-table joins (5–7 tables, snowflake/star/diamond)
   take 50–73 ms and one "simple" CTE takes **234 ms** — orders
   of magnitude over the median and almost certainly far slower
   than PG would plan them. Prior measurement (memory,
   2026-05-11) confirmed Ra loses to PG on 6+ table joins and the
   planning overhead can exceed PG's entire execution time.
   Likely fixes: tighter node/iteration caps for high-arity
   joins, better speculative routing to the left-deep heuristic,
   investigating why `ctes_01_simple` blows up.
2. **Subqueries are not representable in the e-graph.** The
   multi-table DELETE failure surfaces a known limitation: scalar
   / correlated subquery expressions can't be converted to the
   e-graph. Decorrelation handles IN/EXISTS, but not all forms.
3. **Parser gaps vs PG**: PIVOT, XMLTABLE, MERGE, MATCH_RECOGNIZE
   are not in the Lime grammar. These are real PG features; "Ra
   parses anything PG can" is not yet true. (Mitigated by the
   fallback-to-PG path, so they don't break queries — they just
   aren't optimized by Ra.)

## Not measured (requires the patched-PG build — not run)

The following dimensions from the request **could not be measured**
because they need Ra running inside a live patched PostgreSQL,
which is blocked (PG 19) / pending a multi-hour build (PG 18). They
are listed here as explicitly outstanding, not estimated:

- Planning speed **vs** PG's own planner (and vs GEQO) on
  identical hardware/stats.
- Plan-shape equivalence (identical-or-better than PG).
- **Result accuracy** (100% identical result sets) — the
  `ra-difftest` crate exists for exactly this (Ra-PG vs
  native-PG result diffing) but needs two live servers.
- Statistics-usage parity (Ra consuming `pg_statistic` to the
  same extent as PG).
- In-server query-cache behavior under real workloads.

## Path to the full comparison (PG 18, with go-ahead)

1. `cargo install cargo-pgrx` (not installed).
2. `brew install bison llvm` — macOS ships bison 2.3 (too old for
   reliable modern PG) and no `llvm-config` (needed for the
   required `--with-llvm` JIT build).
3. Clone PG `REL_18_STABLE`; `./configure --with-llvm
   --enable-debug …`; build **unpatched** baseline.
4. Port `patches/postgres-ra-hook.patch` (written for REL_17) to
   18; build the **patched** server. GEQO is a runtime GUC
   (`geqo=on`, `geqo_threshold`) on the unpatched server — no
   third build needed, just a third configuration.
5. `cargo pgrx install --features pg18` to load `ra_planner` into
   the patched server.
6. Load the test-world data (TPC-H/JOB/TPROC-C schemas exist in
   `scripts/`), then run the differential harness
   (`ra-difftest`, `ra-bench --features live-comparison`,
   `benchmarks/ra-vs-postgres-comprehensive.sh`) across all three
   configs (Ra-patched, PG-unpatched, PG-GEQO), capturing planning
   time, plan shape, result-set equality, buffer/stat usage, and
   cache hit behavior.

Estimated build+run time: several hours, with real risk at the
patch-port and extension-load steps.

## Verification of the numbers above

- Coverage/timing: `benchmarks/planner_comparison/results/COMPARISON_REPORT.md`
  (regenerated this session).
- pgrx version: `crates/ra-pg-extension/Cargo.toml` (`pgrx = "=0.17.0"`,
  features pg13–pg18).
- PG 19 HEAD reachable and is `878839ba…` (today), confirming the
  target exists but is unsupported by the toolchain.
