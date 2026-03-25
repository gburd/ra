# PostgreSQL Extension Integration Test Report

**Date:** 2026-03-24
**Branch:** main (88ed8f1a)
**Extension:** ra-pg-extension (pgrx 0.17.0, targeting PG17)

## Executive Summary

Code review of the ra-pg-extension confirms a well-structured integration
between the Ra optimizer and PostgreSQL's planner infrastructure. The
extension hooks PostgreSQL's `planner_hook` to intercept SELECT queries,
converts them to Ra `RelExpr` trees, optimizes via the e-graph engine,
and applies advice through GUC-based cost manipulation.

Live testing against the IMDB dataset on the nuc server was not completed
in this session due to SSH access restrictions. This report documents the
code review findings, expected JOB query behavior, and integration status.

## Architecture Review

### Extension Pipeline (planner_hook.rs)

The planner hook follows a 7-stage pipeline:

1. **Guard:** Check `ra_planner.enabled` GUC (fast path when disabled)
2. **Filter:** Skip non-SELECT queries and utility statements
3. **Threshold:** Count rtable entries, bail if above `ra_planner.max_relations` (default: 12)
4. **Statistics:** Gather stats from `pg_class`/`pg_statistic` via syscache (no SPI)
5. **Optimize:** Convert `Query` -> `RelExpr`, run e-graph optimizer
6. **Confidence:** Score plan quality (40% stats coverage + 30% improvement + 30% table coverage)
7. **Apply:** If confidence >= `ra_planner.min_confidence` (default: 0.9), inject via cost manipulation

### Statistics Bridge (stats_bridge.rs)

Statistics gathering uses direct syscache lookups instead of SPI, which
is critical for safety inside planner hooks (SPI opens nested connections
that PostgreSQL forbids). Key capabilities:

- **Table stats:** `pg_class.reltuples`, `pg_class.relpages` -> `Statistics.row_count`, `total_size`
- **Column stats:** `pg_statistic` slots for ndistinct, null_fraction, avg_width, MCV, histogram, correlation
- **Index stats:** `RelationGetIndexList` + `pg_index` syscache for index metadata
- **Foreign keys:** Sequential scan of `pg_constraint` for `contype = 'f'` constraints
- **MVCC stats:** Bloat estimation from `relpages` vs expected size, dead tuple ratio from visibility map

### Query Parser (query_parser.rs)

Handles the full SQL feature surface relevant to JOB queries:

- FROM clause with implicit cross-joins (JOB style) and explicit JoinExpr
- WHERE clause predicates (equality, comparison, AND/OR)
- Aggregates (MIN, MAX, COUNT, SUM, AVG)
- GROUP BY, HAVING, ORDER BY, LIMIT, DISTINCT
- CTEs (recursive and non-recursive with cycle detection)
- Window functions (ROW_NUMBER, RANK, LAG, LEAD, etc.)
- Set operations (UNION/INTERSECT/EXCEPT)
- Subqueries (EXISTS, ANY, ALL, scalar)
- Expression types: Var, Const, OpExpr, BoolExpr, FuncExpr, CaseExpr, etc.

### Plan Converter (plan_converter.rs)

Uses an **advice-based approach** rather than direct `PlannedStmt` construction:

1. Extracts `PlanAdviceSet` from the optimized `RelExpr` (join order, join methods, scan strategies)
2. Saves current GUC settings (`enable_hashjoin`, `enable_mergejoin`, etc.)
3. Adjusts GUCs to favor the RA-optimized plan
4. Calls `standard_planner` with modified costs
5. Restores GUC settings (RAII via `SavedPlannerGucs::drop`)

This is more robust than direct plan node construction and maintains
compatibility across PostgreSQL versions.

## JOB Query Analysis

### Query 1a (5 tables, 4 joins)
```sql
-- Tables: company_type, info_type, movie_companies, movie_info_idx, title
-- Join graph: ct-mc-t-mi_idx, ct.id=mc.company_type_id, mc.movie_id=t.id,
--             t.id=mi_idx.movie_id, mi_idx.info_type_id=it.id
-- Selectivity filters: ct.kind='production companies', it.info='top 250 rank'
-- Aggregate: MIN(mc.note), MIN(t.title), MIN(t.production_year)
```
**Expected Ra behavior:** Small lookup tables (company_type, info_type) used
as filters first, then hash join with title as the hub. Ra should detect the
star-join pattern with title at the center. Within the 12-relation limit.

### Query 5a (5 tables, 4 joins)
```sql
-- Tables: company_type, info_type, movie_companies, movie_info, title
-- Selectivity filters: ct.kind='production companies', t.production_year>2005
-- Aggregate: MIN(t.title)
```
**Expected Ra behavior:** Similar star pattern. The `production_year > 2005`
filter on title should be pushed down before joining. Ra's cost model should
prefer hash joins for the large movie_info table (~14.8M rows).

### Query 10a (7 tables, 6 joins)
```sql
-- Tables: cast_info, char_name, company_name, company_type, movie_companies, role_type, title
-- Selectivity filters: cn.country_code='[ru]', rt.role='actor', t.production_year>2005
-- Aggregate: MIN(chn.name), MIN(t.title)
```
**Expected Ra behavior:** The highly selective `country_code='[ru]'` filter
should be applied early. Cast_info is the largest table (~36M rows) and should
be joined last or with index support. 7 tables is well within the max_relations=12 limit.

### Query 17a (7 tables, 6 joins)
```sql
-- Tables: cast_info, company_name, keyword, movie_companies, movie_keyword, name, title
-- Selectivity filters: cn.country_code='[us]', k.keyword='character-name-in-title'
-- Aggregate: MIN(n.name)
```
**Expected Ra behavior:** The `country_code='[us]'` filter is less selective than
`[ru]` (more US companies). Keyword filter is highly selective. Ra should place
keyword and movie_keyword early in the join order, followed by title as the hub.

### Query 25a (9 tables, 8 joins)
```sql
-- Tables: cast_info, info_type(x2), keyword, movie_info, movie_info_idx, movie_keyword, name, title
-- Selectivity filters: it1.info='genres', it2.info='votes', k.keyword='murder', n.gender='m'
-- Aggregate: MIN(mi.info), MIN(mi_idx.info), MIN(n.name), MIN(t.title)
```
**Expected Ra behavior:** 9 tables is the most complex query in this set. Multiple
selective filters should be pushed down. The dual info_type references and keyword
filter create good opportunities for join reordering. Within the 12-relation limit.

## IMDB Schema Compatibility

The 21-table IMDB schema is fully compatible with the extension:

| Feature | Status | Notes |
|---------|--------|-------|
| Primary keys | Supported | All 21 tables have integer PKs |
| Foreign key indexes | Supported | 23 FK indexes defined in schema.sql |
| Cross-join syntax | Supported | JOB queries use implicit comma joins |
| Equality predicates | Supported | `=` on int, varchar columns |
| Range predicates | Supported | `>` on production_year |
| String equality | Supported | `=` on varchar/text with constants |
| MIN aggregate | Supported | All test queries use MIN |
| No subqueries | N/A | JOB 1a-25a have no subqueries |
| No CTEs | N/A | JOB 1a-25a have no CTEs |
| No window functions | N/A | JOB 1a-25a have no window functions |

### Expected Table Row Counts (IMDB May 2013 snapshot)

| Table | Expected Rows | Notes |
|-------|--------------|-------|
| aka_name | 901,343 | |
| aka_title | 361,472 | |
| cast_info | 36,244,344 | Largest table |
| char_name | 3,140,339 | |
| comp_cast_type | 4 | |
| company_name | 234,997 | |
| company_type | 4 | |
| complete_cast | 135,086 | |
| info_type | 113 | |
| keyword | 134,170 | |
| kind_type | 7 | |
| link_type | 18 | |
| movie_companies | 2,609,129 | |
| movie_info | 14,835,720 | Second largest |
| movie_info_idx | 1,380,035 | |
| movie_keyword | 4,523,930 | |
| movie_link | 29,997 | |
| name | 4,167,491 | |
| person_info | 2,963,664 | |
| role_type | 12 | |
| title | 2,528,312 | Hub table |

## Integration Points Verified (Code Review)

### Statistics Gathering
- [x] `gather_table_stats` reads `pg_class.reltuples`, `pg_class.relpages`
- [x] `read_column_stats` reads ndistinct, null_frac, avg_width from `pg_statistic`
- [x] `read_stat_slots` extracts MCV and histogram from stakind slots
- [x] `gather_index_stats` reads `pg_index` via `RelationGetIndexList`
- [x] `gather_foreign_keys` scans `pg_constraint` for FK relationships
- [x] `interpret_n_distinct` handles both positive (absolute) and negative (fraction) values
- [x] `create_equidepth_histogram` converts PG histogram_bounds to Ra format
- [x] No SPI usage (safe inside planner hooks)

### Query Parsing
- [x] Implicit cross-joins (comma-separated FROM) produce nested `Join::Cross` nodes
- [x] WHERE clause predicates become `Filter` nodes
- [x] Aggregates extracted from targetList `Aggref` nodes
- [x] Operator OID mapping covers int4/int8/float8/text/numeric comparisons
- [x] Constant extraction handles bool, int2/4/8, float4/8, text, varchar, date

### Plan Advice
- [x] Join order extracted via DFS traversal of RelExpr tree
- [x] Join methods mapped: Inner/Left/Right/Full -> Hash, Semi/Anti -> NestedLoop
- [x] Scan methods: Sequential, Index, BitmapHeap
- [x] GUC manipulation saves/restores settings via RAII
- [x] Hardware-aware `random_page_cost` adjustment (SSD vs HDD detection)

### Confidence Scoring
- [x] Stats coverage: fraction of tables with column-level stats
- [x] Table coverage: fraction of referenced tables with any stats
- [x] Improvement ratio: (1 - optimized/original) cost ratio
- [x] Combined: 40% stats_coverage + 30% improvement + 30% table_coverage

## Known Limitations

1. **Schema assumption:** `extract_rtable_names` maps all tables to `public` schema.
   The IMDB dataset must be in the `public` schema for stats gathering to work.

2. **Cross-join reconstruction:** JOB queries use implicit comma joins, which the parser
   converts to nested `Join::Cross` nodes. The WHERE clause predicates are applied as a
   separate `Filter` node. The optimizer must push predicates into joins (predicate pushdown)
   for efficient execution.

3. **Sublink support:** Subqueries (EXISTS, ANY, ALL) are represented as function
   placeholders (`__sublink_exists`, etc.) rather than being fully inlined into the
   RelExpr tree. This does not affect JOB queries 1a-25a which have no subqueries.

4. **NUMERIC constants:** `pg_const` conversion for NUMERIC type OID (1700) produces
   `Const::Float(0.0)` as a placeholder. This does not affect JOB queries which only
   use integer and string constants.

5. **Memory leak in `resolve_rel_name`:** The function calls `pg_sys::get_rel_name()`
   which returns palloc'd memory, but does not call `pfree()`. The `get_rel_name_safe`
   function in stats_bridge.rs does free the memory correctly. The planner_hook.rs
   version at line 582 leaks. This is minor since it occurs within a memory context
   that is freed after planning.

## Recommended Next Steps

1. **SSH access:** Resolve nuc server access to run live IMDB tests
2. **Run EXPLAIN ANALYZE:** Execute JOB queries 1a, 5a, 10a, 17a, 25a with and
   without the extension to measure actual speedup
3. **Compare join orders:** Log RA-optimized join orders vs PostgreSQL defaults
4. **Measure confidence scores:** Check if the 0.9 threshold is met for analyzed IMDB tables
5. **Validate statistics quality:** Run `ANALYZE` on all tables before testing, verify
   that `pg_statistic` has MCV and histogram data for key columns
6. **Test max_relations boundary:** Query 25a (9 tables) is within limit;
   test with 33-series queries (up to 17 tables) which may exceed default max_relations=12

## Test Commands (for nuc server)

```bash
# Verify IMDB data
ssh nuc "psql -d imdb -c '\dt+'"
ssh nuc "psql -d imdb -c 'SELECT relname, reltuples::bigint, relpages FROM pg_class WHERE relkind = '\\''r'\\'' AND relnamespace = (SELECT oid FROM pg_namespace WHERE nspname = '\\''public'\\''') ORDER BY reltuples DESC;'"

# Run ANALYZE on all tables
ssh nuc "psql -d imdb -c 'ANALYZE;'"

# Test query 1a without extension
ssh nuc "psql -d imdb -c 'SET ra_planner.enabled = off; EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) SELECT MIN(mc.note) AS production_note, MIN(t.title) AS movie_title, MIN(t.production_year) AS movie_year FROM company_type AS ct, info_type AS it, movie_companies AS mc, movie_info_idx AS mi_idx, title AS t WHERE ct.kind = '\\''production companies'\\'' AND it.info = '\\''top 250 rank'\\'' AND mc.company_type_id = ct.id AND mc.movie_id = t.id AND t.id = mi_idx.movie_id AND mi_idx.info_type_id = it.id;'"

# Test query 1a with extension
ssh nuc "psql -d imdb -c 'SET ra_planner.enabled = on; SET ra_planner.log_decisions = on; EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) SELECT MIN(mc.note) AS production_note, MIN(t.title) AS movie_title, MIN(t.production_year) AS movie_year FROM company_type AS ct, info_type AS it, movie_companies AS mc, movie_info_idx AS mi_idx, title AS t WHERE ct.kind = '\\''production companies'\\'' AND it.info = '\\''top 250 rank'\\'' AND mc.company_type_id = ct.id AND mc.movie_id = t.id AND t.id = mi_idx.movie_id AND mi_idx.info_type_id = it.id;'"
```

## Unit Test Status

The extension has comprehensive unit tests for all pure-function helpers:

| Module | Tests | Coverage |
|--------|-------|----------|
| stats_bridge | 24 | n_distinct, pg_array parsing, histogram, index type, MVCC, FK, avg_row_size |
| planner_hook | 14 | truncation, confidence scoring, stats coverage, avg_row_size |
| query_parser | 37 | operator/aggregate/join/window OID mapping, sort direction, CTE wrapping, null safety |
| plan_converter | 13 | advice extraction, join order, scan methods, hint formatting, relation counting |
| cost_mapper | 7 | calibration, cost conversion, sample recording |
| pg_constants | 4 | constant relationships |
| integration_tests | 12 | CTE+window, recursive CTE, set ops, FK detection, combined features |

**Total: 111 tests across the extension crate.**

These tests verify the pure-function logic. The `#[pg_test]` integration tests
in integration_tests.rs require a running PostgreSQL instance (managed by pgrx)
and test end-to-end SQL execution through the extension.
