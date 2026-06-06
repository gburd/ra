# Ra test-campaign report — 2026-06-06 (PG19devel, Rust-parser default)

Ran Ra against the existing test infrastructure: PostgreSQL's own regression
suite, the sqllogictest/benchmark corpora, and the parse/optimize coverage
harness. Goal: know for certain that Ra can plan any query shape PG can, and
quantify correctness + overhead + planning/execution time.

## Headline

**The dominant, blocking finding is crash-safety, not plan quality.** Ra
hard-crashes the backend (SIGABRT) on ordinary error-path queries, the crash is
*not* contained by the `catch_unwind` fallback, and a single crash reinitializes
the postmaster — cascading failure across every in-flight and subsequent query.
This makes the broad suites (full regression, HammerDB-style soak) unrunnable
until fixed. Everything else (coverage gaps, the wrong-result bugs) is secondary
to this.

## 1. Crash-safety (CRITICAL — fix first)

**Repro (clean server, one statement):**
```
SELECT '  tru e '::text::boolean AS invalid;   -- should raise a user ERROR
→ server closed the connection unexpectedly      (backend SIGABRT)
```
Also crashes: `VALUES ('{"too long"}'::int[])`-style bad array literals — i.e.
queries whose evaluation raises an error.

**Server-log signature:**
```
ra_planner: inner panic, falling back to native planner:
    pfree called with invalid pointer 0x… (header 0x7f7f7f7f7f7f7f7f)
…
fatal runtime error: failed to initiate panic, error 5, aborting
client backend (PID …) was terminated by signal 6: Abort trap: 6
LOG: all server processes terminated; reinitializing
```

**Root cause (chain):** `0x7f7f7f7f7f7f7f7f` is PG's `CLOBBER_FREED_MEMORY`
poison — Ra `pfree`s a chunk that is already freed (double-free / use-after-
free). The query is an **error path**: the cast/array evaluation raises an
error, PG resets the per-query memory context, and Ra's cleanup (executor-end /
invalidation hook, or plan nodes Ra allocated outside the right context) then
frees pointers PG already reclaimed. PG's `pfree` poison check turns this into
an `elog(ERROR)` while Ra is already unwinding → **panic-during-panic across the
FFI boundary → "failed to initiate panic" → `abort()`**. `catch_unwind` cannot
catch an `abort()`, so the documented "contained by catch_unwind" assumption is
false for this path.

**Two compounding facts:**
- `ra_planner.enabled = off` does **not** make Ra safe: the crash still happens
  because the extension is loaded via `shared_preload_libraries` and its
  executor/invalidation hooks run regardless of the planner switch. Only fully
  removing it from `shared_preload_libraries` (and restarting) is safe — boolean
  passes on pure PG, crashes with Ra preloaded even when "disabled".
- The crash is **not isolated**: a backend SIGABRT makes the postmaster
  reinitialize, so the next several queries get `FATAL: the database system is
  in recovery mode`. In a test suite this turns one crashing query into a whole
  run of failures (see §3).

**Recommended fix direction:** ensure every plan node / expression Ra builds is
allocated in the planner's `CurrentMemoryContext` (so PG owns and frees it
exactly once); remove any Ra-side `pfree`/free of PG-owned memory; and make the
executor-end / relcache-invalidation hooks no-ops when Ra did not plan the
query. Add an assertion build run under the regression suite as a gate.

## 2. Parse/optimize coverage (ra-sqltest, in-process)

`cargo test -p ra-sqltest` (parse + optimize only, no execution):
- **sqllogictest `.slt`**: PASS for basic, ctes, joins, select1–5, subqueries.
  FAIL: `tpch.slt` (subquery → "Subquery expressions not yet supported in the
  e-graph"); `dml.slt` (a statement expected to fail parses successfully — test
  expectation nit).
- **benchmark corpus**: TPC-H 1 fail, Book 2, **TPC-DS 40**, **JOB 71** — all
  `failed to extract plan from e-graph: unexpected relational node:
  HashJoinOp([...])` or the subquery-in-e-graph gap. The physical join
  operators added for cost-driven lowering (RFC 0090 chunk-1) leak into the
  extracted `RecExpr` and `from_rec` cannot convert them back, so complex
  multi-table joins fail to optimize.

**Impact:** these are *coverage* gaps, not wrong results — in the extension they
become native-planner fallbacks (correct, but no Ra benefit on exactly the
complex analytic queries where a better plan matters most).

## 3. PostgreSQL regression suite under Ra

Setup: pg-src is PG19devel and matches the running server; pg_regress at
`/Volumes/scratch/ra/pg-src/build/.../pg_regress`; Ra active via
`shared_preload_libraries`.

**Methodology note (important):** the committed `expected/*.out` do not match
the installed server build (version/commit skew → spurious diffs even on pure
PG). The valid comparison is **Ra `results/` vs pure-PG `results/`** captured
against the *same* server. Pure-PG baseline captured by temporarily removing Ra
from `shared_preload_libraries`.

Core subset (13 tests) Ra-results vs PG-results:
- `boolean`, `arrays`: **CRASH** (the error-path SIGABRT above).
- The other 11 show as large diffs, but those are mostly **crash-cascade
  collateral**: once `boolean` (test 1) crashed and the postmaster reinit'd, the
  serially-following tests produced empty output (recovery mode). `join`, run
  late after recovery settled, produced near-complete output (7546/7641 lines).

**Conclusion:** the suite cannot be scored under Ra until crash-safety is fixed;
the crash-cascade dominates the signal. This is itself the answer to "can Ra
plan any shape PG can": not yet safely — it aborts on error-path queries.

## 4. Correctness (from the 2026-06-06 A/B catalog, methodology re-confirmed)

Independently of crashes, four shapes return **wrong rows without falling back**
(see `docs/ra-vs-pg-correctness-findings-2026-06-06.md`): LEFT/CROSS join with
an outer-table WHERE (filter applied to the wrong relation / dropped), scalar
subquery in WHERE (returns empty), and `IN(subquery) AND <pred>` (quals
dropped). ~33/40 hand-picked shapes are row-identical.

## 5. EXPLAIN / plan-advice parity

For shapes Ra plans without crashing, **EXPLAIN is identical in form to PG** —
because Ra emits a real `PlannedStmt` that PG's own EXPLAIN machinery renders:
- text `EXPLAIN (COSTS OFF)` of a filtered scan is byte-identical to PG;
- `EXPLAIN (FORMAT JSON)` is well-formed;
- `EXPLAIN ANALYZE` actual-row counts are correct (`actual rows=299`,
  `Rows Removed by Filter: 99701`).
The plan *content* differs where Ra chooses a different plan (by design). Caveat
from the prior session: an `EXPLAIN` issued behind a `SET …;` prefix in one
`psql -c` makes Ra fall back and renders PG's plan — always send EXPLAIN as its
own statement (or via PGOPTIONS) to see Ra's plan.

## 6. What did not run, and why

- **HammerDB**: not installed in this environment. `ra-bench benchmark-oltp`
  (TPROC-C query set) is the in-repo substitute, but live OLTP soak is moot
  until crash-safety is fixed (a soak would crash within seconds on any
  error-raising statement).
- **Full 233-file regression / SQLite TCL suite**: blocked by the crash-cascade
  (and SQLite's TCL harness targets SQLite, not PG; the SQLite-derived
  sqllogictest `.slt` files in §2 are the applicable subset).
- **Planning/execution-time benchmarking at scale**: deferred — timing wrong or
  crashing plans is not meaningful. Spot timings remain valid (e.g. filtered
  scan, simple joins) and Ra is competitive there per prior sessions.

## Priority order

1. **Crash-safety** (§1): the double-free / panic-across-FFI abort. Nothing else
   can be measured at scale until this is fixed, and it is the worst possible
   behavior for a drop-in (crashing a query PG handles fine).
2. **Wrong-result fallback gates** (§4): widen gates so the four wrong-result
   shapes defer to PG (correctness > coverage) pending real fixes.
3. **e-graph extraction of physical join operators** (§2): unblocks complex
   joins (TPC-DS/JOB) from falling back.
4. Then re-run this campaign end-to-end and add the at-scale timing tables.
