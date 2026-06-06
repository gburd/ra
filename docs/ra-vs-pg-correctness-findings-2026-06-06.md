# Ra vs PG correctness findings — 2026-06-06 (PG19, Rust parser default)

Exhaustive Ra-on vs Ra-off A/B on PG19, extension rebuilt with the native-Rust
parser as default. **40+ query shapes** compared (scan/filter/proj/case/cast/
distinct/order/limit/aggregate/group-by/having/joins/set-ops/CTE/subqueries/
window). Methodology below; raw harness in `/tmp/ab_catalog.sh`.

## Verdict

**~33 of ~40 shapes are row-identical to PG.** Four shapes return **wrong
results without falling back** — the prime-invariant violation (Ra must never
return rows different from PG; if it cannot plan correctly it must defer to the
native planner). These are the priority fixes.

### Confirmed correct (SAME as PG)
scan + filter (`<`, `BETWEEN`, `IN list`, `OR`, `IS NULL`), projection/`CASE`/
cast, `DISTINCT`, `ORDER BY [DESC]`, `LIMIT`/`OFFSET`, `count`/`sum`/`avg`/
`min`/`max`, `count(DISTINCT)`, `GROUP BY`, `HAVING`, **INNER/RIGHT/FULL/self/
indexed joins**, `UNION`/`UNION ALL`/`INTERSECT`/`EXCEPT`, CTE, CTE+join,
`EXISTS`, `NOT EXISTS`, `NOT IN`, `= ANY (subquery)`, `IN (subquery)` (alone),
`row_number`/`rank`/`sum() OVER`, scalar aggregates.

### Confirmed WRONG (Ra returns wrong rows, does NOT fall back)

1. **LEFT JOIN with a WHERE on the outer table.**
   `SELECT n.a FROM noidx n LEFT JOIN multi m ON n.a=m.a WHERE n.a<300`
   → Ra returns **100000** rows; PG returns **299**.
   Root cause (from Ra's own `EXPLAIN ANALYZE`): the outer predicate `n.a<300`
   is applied to the **inner** relation (`multi`) instead of the outer
   (`noidx`). The plan is a `Hash Left Join` whose build side (`multi`) carries
   `Filter: (a<300)` and whose probe side (`noidx`) is unfiltered, so every
   outer row survives the left join. Bug is in
   `plan_builder::build_join_node`'s `where_q` remap (the WHERE qual is
   remapped to the wrong OUTER/INNER frame for non-INNER joins). INNER joins
   are correct because they take the separate `try_index_nestloop` path
   (gated to `JoinType::Inner`).

2. **CROSS JOIN with a WHERE on the outer table.**
   `SELECT n.a FROM noidx n CROSS JOIN one o WHERE n.a<50`
   → Ra returns **100000**; PG returns **49**. Same family as (1): the WHERE
   qual is dropped on the NestLoop path.

3. **Scalar subquery in WHERE.**
   `SELECT a FROM noidx WHERE a < (SELECT avg(a) FROM multi)`
   → Ra returns **0** rows (empty); PG returns **100000**. The scalar SubPlan
   result is not wired, so the comparison is always false. This was previously
   gated to fallback (`first_unsupported_op` /
   `expr_has_scalar_subquery`); the gate is **not firing** on the current
   parse/optimize path — Ra plans it and returns empty.

4. **`IN (subquery)` conjoined with another predicate.**
   `SELECT a FROM noidx WHERE a IN (SELECT a FROM multi WHERE a<150) AND a<300`
   → Ra returns **100000**; PG returns 149. `IN (subquery)` *alone* is correct
   (149 rows); the failure appears when the IN-subquery is `AND`-combined with
   a base predicate — the conjunction's quals are dropped.

## Severity / recommended immediate action

All four are **correctness-critical**: Ra emits a plan and returns wrong rows
rather than deferring. The safety net (`first_unsupported_op` →
native-planner fallback) exists but does not cover these shapes on the current
path. **Until each is properly fixed, the conservative correct move is to widen
the fallback gates** so these shapes defer to PG:
  - reject non-INNER join when a WHERE qual must be remapped across the
    OUTER/INNER frame until the remap is fixed;
  - reject any WHERE/filter predicate that contains a scalar subquery
    (re-confirm `expr_has_scalar_subquery` fires post-parser-migration);
  - reject a filter that is a conjunction containing an `IN (subquery)` term.

Correctness > coverage: a fallback is always right; a wrong row is never right.

## Not bugs (ruled out)

- No real backend crashes. An earlier sweep showed a "crash cluster"
  (subq_*/window_*); that was an artifact of a rapid `pg_ctl restart` loop
  wedging the postmaster in a shutdown state, not query crashes. Each of those
  shapes is row-identical to PG when run after a clean restart.
- `LEFT JOIN` without a WHERE is correct (100000 = 100000). The bug is
  specifically the outer-table WHERE remap.

## Methodology (important — prior sweeps gave false results)

- Toggle Ra via **`PGOPTIONS='-c ra_planner.enabled=on|off'`** at connection
  time, then run the **bare** query as the only statement.
- **Do NOT** combine `SET ra_planner.enabled=on; <query>` in one `psql -c`:
  Ra re-parses the planner-hook `query_string`, which then begins with `SET …`,
  corrupting the parse and silently dropping clauses — this manufactures false
  DIFFs *and* false "correct via fallback" results (an `EXPLAIN` behind a `SET`
  prefix falls back to PG and shows PG's plan, not Ra's).
- Compare `2>/dev/null | sort` of both sides (no agg/`md5` wrapper — wrapping a
  join/subquery in `count(*)`/`md5(string_agg(...))` makes Ra fall back, so you
  compare PG-to-PG and miss the bug).
- Between crashing queries use `pg_ctl stop -m immediate` + `start`, not rapid
  `restart` (which can wedge the postmaster).
