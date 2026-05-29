# Ra Security Review — 2026-05-29

Reviewer: gregburd (with AI assistance). Scope: the full Ra
workspace, with emphasis on the attack surfaces that matter for a
query optimizer that runs **in-process inside a PostgreSQL
backend** via `planner_hook` / `raw_parser` hooks and that accepts
untrusted input (SQL text, the `ra_planner.plan_advice` GUC,
database connection URLs, on-disk model/config files).

This document records what was reviewed, what was found, what was
fixed, and — honestly — what was *not* fixed and why.

## Threat model

Ra's most security-relevant property is that, as a `planner_hook`
extension, **a bug in Ra runs inside the PostgreSQL backend
process**. The consequences of a memory-safety bug or an uncaught
panic are therefore a backend crash (denial of service) at
minimum. The primary untrusted inputs are:

1. **SQL query text** — parsed by Ra's Lime parser before/around
   PG's own parser.
2. **`ra_planner.plan_advice` GUC** — an arbitrary string any
   connected role can `SET`, parsed by `ra-plan-advice`.
3. **Database connection URLs** — supplied to the CLI; carry
   credentials.
4. **Model / schema / timeline files** — loaded from disk by the
   CLI and the extension.

## Findings and fixes

### 1. HIGH — Stack-overflow DoS via nested plan-advice (fixed)

`ra-plan-advice`'s recursive-descent parser descended into
`(...)` sublists with **no depth limit**. Because the
`plan_advice` GUC is an untrusted string that any role can set,
a value such as `JOIN_ORDER((((…))))` with thousands of nested
parens caused unbounded recursion → stack overflow → **backend
process abort**. A stack overflow is *not* a catchable panic, so
the planner hook's `catch_unwind` would not have contained it.

**Fix:** `MAX_NESTING_DEPTH = 64` guard threaded through the
recursive sublist parsers (`enter_sublist` / `exit_sublist`).
Input exceeding the cap returns a normal parse error. 64 is far
beyond any legitimate advice (real nesting is one or two levels)
while keeping recursion safely shallow. Regression tests:
`deeply_nested_sublists_rejected_not_stack_overflow` (5000
levels → `Err`, no crash) and `legitimate_nesting_still_parses`.

### 2. MODERATE — Uncaught panic in the parser hook (fixed)

`ra_raw_parser_hook` (`extern "C-unwind"`) called the Lime parser
directly with no `catch_unwind`. A parser panic would unwind into
PostgreSQL's C frames and `abort()` the backend. The planner hook
already had this protection; the parser hook did not.

**Fix:** wrapped `parse_statement` in `catch_unwind`. On panic
(or parse failure) the hook returns `NULL`, so PostgreSQL's own
parser handles the query — Ra never crashes a backend over a
query PG can parse.

### 3. MODERATE — Uncaught panic in the EXPLAIN per-plan hook (fixed)

`plan_advice_per_plan_hook` (`extern "C-unwind"`) ran advice
parsing, classification, and rendering with no `catch_unwind`; a
panic would abort the backend during `EXPLAIN`.

**Fix:** extracted the rendering into `render_plan_advice_block`
and wrapped the call in `catch_unwind`. On panic the advice block
is simply omitted — EXPLAIN output is diagnostic and never worth
crashing a session over. Panic containment is now consistent
across all three PG-facing hooks (planner, parser, explain).

### 4. MODERATE — Database password disclosure in CLI errors (fixed)

CLI error contexts embedded the full connection URL, e.g.
`connecting to database: postgresql://user:secret@host/db`. A
failed connection therefore printed the password to the terminal
and any logs.

**Fix:** `ra_metadata::redact_url` masks the password component
(`user:****@host`) while preserving the rest of the URL for
diagnostics. Applied at every CLI site that formats a URL into a
message (`helpers.rs`, `commands/{gather_metadata,compare,optimize}.rs`).
URLs without credentials, bare file paths, and `:memory:` pass
through unchanged. Unit tests cover password masking (including
passwords containing `@`/`:`) and the credential-free cases.

## Reviewed and found sound (no change needed)

- **Planner-hook panic containment** — already wrapped in
  `catch_unwind`; a Rust panic surfaces as a PG `ERROR`, not a
  crash. The earlier robustness pass (commit `77ccb014`) also made
  parse/optimize/build *failures* fall back to PG's planner.
- **Deserialization** (`serde_json` model/schema/timeline loads)
  — serde is memory-safe; non-test paths handle errors
  gracefully (`map_err`/`?`); inputs are operator-controlled local
  files, not network data. No code-execution surface.
- **FFI pointer handling** in the recently-added plan-builder
  paths (merge-join, bitmap, TID scan) and the EXPLAIN hooks —
  PG-supplied pointers are null-checked before deref; list
  iteration is bounded by the list `length`; allocation failures
  return `Err`. The plan-builder degrades to PG on any build
  error.
- **SQL-injection surface** — the optimizer produces plan *trees*,
  not SQL strings sent to a database. There is no string-concat
  query construction in the core optimizer path. (FDW deparse,
  RFC 0088, is unimplemented and out of scope.)
- **Supply-chain policy** — `deny.toml` denies unknown
  registries/git sources and uses a license allowlist.

## Known residual items (not fixed; honest disclosure)

- **SQL-parser deep nesting.** Ra re-parses raw SQL text in the
  parser hook. Extremely deep expression nesting could in
  principle overflow `sql_to_relexpr`'s tree recursion. A stack
  overflow is not caught by the hook's `catch_unwind`. In
  practice this is mitigated because PostgreSQL also parses the
  query (bounded by `max_stack_depth`) and the Lime grammar layer
  is table-driven (bounded stack). Recommended follow-up: a
  nesting-depth guard in `sql_to_relexpr`, mirroring the
  plan-advice fix. Severity: low (gated by PG's own limits).
- **Adapter stub URL echo.** `ra-adapters` stub connectors echo a
  connection string into an error only when it begins with the
  literal `invalid://` (a non-credential sentinel). Left as-is;
  not a real credential path. Recommended: redact there too for
  defense in depth once `ra-adapters` depends on `ra-metadata`.
- **`cargo deny` not executed here.** `cargo-deny` is not
  installed in this environment, so the advisory/license/ban
  checks were not run. The `deny.toml` policy is sound; this
  check should run in CI. Recommended: add `cargo deny check` to
  the CI pipeline.
- **Full `unsafe` line-by-line audit.** ~250 `unsafe` lines in
  `ra-pg-extension` and ~380 in `ra-parser` (Lime FFI) were not
  each individually audited; review focused on the highest-risk
  recently-added code and the systemic issues above. A dedicated
  FFI audit pass remains valuable future work.

## Verification

- `cargo clippy --all-targets --all-features -- -D warnings`: 0
  errors (workspace) and 0 errors (out-of-workspace
  `ra-pg-extension`).
- `cargo test --workspace --all-features`: 7837 passing, 0
  failing, 58 ignored across 169 suites.
