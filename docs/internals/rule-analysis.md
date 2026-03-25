# Rewrite Rule Analysis and Recommendations

Analysis of the query rewrite rule set in `ra-engine`, covering rule
inventory, effectiveness, gap analysis, rule interactions, and
prioritized recommendations.

## 1. Rule Inventory

### Summary

| Source module | Rule count | Categories |
|---|---|---|
| `null_simplification.rs` | 37 | NULL propagation (3VL) |
| `rewrite.rs` (predicate pushdown) | 9 | Filter pushdown |
| `rewrite.rs` (join reordering) | 7 | Join commutativity/associativity, outer-to-inner |
| `rewrite.rs` (projection pushdown) | 1 | Project merging |
| `rewrite.rs` (boolean simplification) | 18 | Constant folding, De Morgan, idempotent |
| `rewrite.rs` (arithmetic simplification) | 8 | Identity/zero element |
| `rewrite.rs` (commutativity) | 10 | Canonical ordering |
| `rewrite.rs` (join elimination) | 1 | Cross join + LIMIT 1 |
| `rewrite.rs` (aggregate optimization) | 2 | Filter pushdown, double-aggregate |
| `rewrite.rs` (limit/sort) | 3 | Limit pushdown/merge, sort elimination |
| `rewrite.rs` (set operations) | 5 | Union/intersect/except simplification |
| `rewrite.rs` (subquery) | 2 | Semi/anti join filter merge |
| `rewrite.rs` (DuckDB-inspired) | 10 | Comparison inversion, limit pushdown, sort elimination |
| `rewrite.rs` (SQLite-inspired) | 7 | Range-to-eq, transitive closure, constant propagation |
| `rewrite.rs` (runtime filters) | 3 | Sideways information passing |
| `consensus_rules.rs` | 24 | Equijoin extraction, null key filter, empty propagation |
| `join_transformations.rs` | 15 | Outer-to-inner (6 ops x 2 sides + AND + self-join) |
| `parquet_pushdown.rs` | 1 | Conjunctive split for row group pruning |
| `count_metadata.rs` | 1 | COUNT(*) -> metadata lookup |
| `covering_index.rs` | 2 | Index-only scan (bidirectional) |
| `shortcuts/min_max_index.rs` | 4 | MIN/MAX -> index scan (bare + filtered) |
| **Total active** | **~170** | |

### Commented-out (not yet integrated)

| Module | Rule count (approx.) | Status |
|---|---|---|
| `redundant_join.rs` | ~10 | Implemented, disabled |
| `functional_deps.rs` | ~8 | Implemented, disabled |
| `semi_join.rs` | ~10 | Implemented, disabled |
| `column_pruning.rs` | ~8 | Implemented, disabled |
| **Total disabled** | **~36** | |

Additionally, `mv_rewrite.rs` defines 4 materialized view rewrite
rules but they are loaded separately via the `MvCatalog` API rather
than through `all_rules()`.

## 2. Rule Categorization

### By optimization class

**Logical rewrites (structural transformations):**
- Predicate pushdown: 9 rules (filter through join/project/union/intersect/except)
- Join reordering: 7 rules (commutativity, associativity, cartesian-to-join, outer-to-inner)
- Join transformations: 15 rules (comprehensive outer-to-inner conversion)
- Projection pushdown: 1 rule (project merge)
- Aggregate optimization: 2 rules
- Limit/sort optimization: 3 rules
- Set operation simplification: 5 rules
- Subquery decorrelation: 2 rules
- Empty relation propagation: 20 rules (consensus)
- DuckDB-inspired: 10 rules
- SQLite-inspired: 7 rules

**Expression simplification (algebraic identities):**
- Boolean simplification: 18 rules
- Arithmetic simplification: 8 rules
- NULL simplification: 37 rules
- Commutativity/canonicalization: 10 rules

**Physical rewrites (access path selection):**
- Parquet pushdown: 1 rule
- Covering index: 2 rules
- MIN/MAX index: 4 rules
- COUNT(*) metadata: 1 rule
- Runtime filters: 3 rules
- Equijoin extraction: 2 rules
- Null join key filtering: 2 rules

### By complexity class

From `rule_priority.rs`, the default priority annotations cover 110+
rules. Distribution:

| Complexity | Count | Example |
|---|---|---|
| O(1) | ~85 | Boolean simplification, filter-merge, commutativity |
| O(n) | ~22 | Filter pushdown through join, null key filtering, transitive closure |
| O(n^2) | ~3 | Join associativity (left/right), MV rewrite |
| O(exp) | 0 | None currently |

## 3. Top 10 Highest-Priority Rules

Ranked by `benefit / complexity_weight` score:

| Rank | Rule | Score | Complexity | Benefit |
|---|---|---|---|---|
| 1 | `filter-true` | 0.80 | O(1) | 0.6-1.0 |
| 2 | `cartesian-to-join` | 0.85 | O(1) | 0.7-1.0 |
| 3 | `count-star-to-metadata` | 0.85 | O(1) | 0.7-1.0 |
| 4 | `and-false-left/right` | 0.70 | O(1) | 0.5-0.9 |
| 5 | `or-true-left/right` | 0.70 | O(1) | 0.5-0.9 |
| 6 | `cross-join-single-row-right` | 0.70 | O(1) | 0.5-0.9 |
| 7 | `min-to-index-scan` | 0.70 | O(1) | 0.5-0.9 |
| 8 | `max-to-index-scan` | 0.70 | O(1) | 0.5-0.9 |
| 9 | `extract-equijoin-from-and-*` | 0.70 | O(1) | 0.5-0.9 |
| 10 | `empty-*` (propagation) | 0.70 | O(1) | 0.5-0.9 |

## 4. Gap Analysis: Missing Optimizations

Comparison with PostgreSQL, Calcite, Volcano/Cascades, and DuckDB
reveals the following gaps:

### High-value missing rules

**1. DISTINCT elimination (functional dependency based)**
- Rule: Remove DISTINCT when output columns are functionally
  determined by a unique key
- Already implemented in `functional_deps.rs` but disabled
- PostgreSQL equivalent: `remove_useless_groupby_columns()`
- Impact: Eliminates unnecessary sort/hash in many ORM-generated queries

**2. Semi-join reduction (EXISTS/IN decorrelation)**
- Rule: Convert correlated EXISTS subqueries to semi-joins
- Already implemented in `semi_join.rs` but disabled
- PostgreSQL equivalent: Part of subquery flattening in
  `pull_up_subqueries()`
- Impact: Order-of-magnitude improvement for correlated subqueries

**3. Window function optimization**
- Rules missing entirely:
  - Push filters below window functions when the filter references
    only partition keys
  - Merge window functions with the same PARTITION BY and ORDER BY
    into a single sort
  - Convert `ROW_NUMBER() ... = 1` to `DISTINCT ON` / `LIMIT 1`
    per group
- PostgreSQL equivalent: `create_one_window_path()` window clause
  merging
- Impact: Window-heavy analytics queries (common in reporting)

**4. Predicate inference / constant propagation**
- The SQLite `sqlite-eq-transitive` rule exists but is limited.
  Missing:
  - Implied predicate generation from equijoins: if `a.x = b.y` and
    `a.x > 10`, infer `b.y > 10`
  - BETWEEN range tightening using transitivity
  - CHECK constraint integration (e.g., if `status IN ('A','B','C')`
    is a CHECK, `status = 'D'` can be eliminated)
- PostgreSQL equivalent: `generate_implied_equalities()`
- Impact: Multi-table join queries with range predicates

**5. Aggregate pushdown through join**
- Rule: Push GROUP BY + aggregate below a join when grouping columns
  come from one side
- Known as "eager aggregation" in Cascades
- PostgreSQL equivalent: Not in PG; present in Calcite
  (`AggregateJoinTransposeRule`)
- Impact: Star-schema queries (fact-dimension joins with aggregation)

**6. LIKE/pattern optimization**
- Rules missing:
  - `LIKE 'prefix%'` -> range predicate (`col >= 'prefix' AND col <
    'prefiy'`)
  - `LIKE '%suffix'` with reverse index
  - Constant LIKE patterns resolved at optimization time
- PostgreSQL equivalent: `expand_indexqual_conditions()` for LIKE
- Impact: String-heavy filtering workloads

**7. ORDER BY elimination**
- Rules missing:
  - Remove ORDER BY in subquery when outer query doesn't depend on it
  - Remove ORDER BY when output feeds into an aggregate (partially
    covered by `duckdb-sort-below-aggregate`)
  - Remove redundant ORDER BY when input is already sorted by an index
- PostgreSQL equivalent: `remove_useless_orderby()`
- Impact: Views and CTEs that include unnecessary ORDER BY

**8. Common sub-expression elimination (CSE)**
- Rule: When the same expression appears in multiple places (e.g.,
  `WHERE f(x) > 10 AND f(x) < 20`), compute once
- The e-graph naturally deduplicates via sharing, but explicit CSE for
  expensive expressions (UDFs, JSON extraction) would help the cost
  model
- Impact: Complex computed-column queries

**9. Outer join simplification**
- Rules partially present but missing:
  - Full outer join -> left outer when right side has no NULLs
  - Left outer join to inner when join key is NOT NULL
    (constraint-based, not just filter-based)
  - Merge cascaded outer joins
- PostgreSQL equivalent: `reduce_outer_joins()`
- Impact: Multi-level outer join queries (common in BI tools)

**10. UNION ALL flattening and merge**
- Rules missing:
  - Flatten nested UNION ALL: `(A UNION ALL B) UNION ALL C` ->
    single 3-way UNION ALL
  - Merge adjacent UNION ALL scans on the same table with different
    predicates into a single scan with OR predicate
- DuckDB equivalent: `FlattenUnion`
- Impact: Partitioned table access patterns, ETL queries

## 5. Rule Interaction Analysis

### Rule chains (rules that enable other rules)

1. **filter-split-and -> filter-through-join-left/right**: Splitting a
   conjunctive filter creates individual predicates that can each be
   pushed to the appropriate join side. This is the primary enablement
   chain for predicate pushdown.

2. **extract-equijoin-from-and -> filter-null-join-key**: After
   extracting an equijoin predicate, the remaining `(join inner (eq
   ?lk ?rk) ...)` pattern matches the null key filtering rule. These
   two should always fire together.

3. **cartesian-to-join -> join-commutativity/associativity**: Converting
   a cartesian product + filter to an inner join enables the full suite
   of join reordering rules.

4. **left-outer-to-inner-* -> join-commutativity/associativity**: Once
   outer joins are converted to inner joins, they become eligible for
   join reordering, which was previously blocked.

5. **parquet-filter-split-for-pushdown -> filter-merge (reverse)**: The
   parquet rule splits AND predicates at the scan level, but
   `filter-merge` immediately re-combines them. These rules create an
   infinite loop that is bounded only by the e-graph iteration limit.

### Potential rule conflicts

1. **filter-merge vs filter-split-and**: These rules are inverses of
   each other. In an equality saturation framework this is intentional
   (both forms exist in the e-graph), but it increases e-graph size
   without bound. Currently bounded by the 50,000 node limit.

2. **Commutativity rule explosion**: The 10 commutativity rules
   (`add-commutative`, `mul-commutative`, `eq-commutative`, etc.)
   create symmetric duplicates in every e-class. Combined with
   `join-commutativity`, this causes quadratic e-graph growth in
   expressions with many operands.

3. **runtime-filter-hash-to-semi**: This rule introduces a new
   semi-join node that then interacts with `filter-semi-join-merge`
   and the empty propagation rules. On complex queries with many
   joins, this can cause excessive e-graph expansion. The
   `runtime-filter-hash-to-semi` rule is recursive (the output
   still contains a `join inner` that matches the input pattern).

4. **project-merge vs duckdb-project-pushdown**: Both rules match
   `(project ?c1 (project ?c2 ?input))` and produce the same output.
   One is a duplicate of the other. The `duckdb-project-pushdown` rule
   should be removed.

5. **sqlite-eq-transitive creates unbounded growth**: The rule
   `(and (eq ?a ?b) (eq ?b ?c))` ->
   `(and (and (eq ?a ?b) (eq ?b ?c)) (eq ?a ?c))`
   grows the expression by one conjunct each iteration. On chains of
   equalities (a=b, b=c, c=d, ...), this generates O(n^2) new
   equality terms. The iteration limit bounds this, but it consumes a
   large fraction of the node budget on such queries.

### Duplicate rules

- `project-merge` in `rewrite.rs` line 194 and `duckdb-project-pushdown`
  in `rewrite.rs` line 426 are identical rewrites.
- `project-merge` appears again in the disabled `column_pruning.rs`.
- The `left-outer-to-inner-with-filter` in `rewrite.rs` and the
  `left-outer-to-inner-eq` in `join_transformations.rs` overlap (the
  join_transformations version is more general).

## 6. Prioritized Recommendations

### Immediate (enable existing code)

1. **Enable `redundant_join.rs` rules** -- Already implemented and
   tested; commented out in `all_rules_unsorted()`. Addresses
   ORM-generated redundant joins. Uncomment line 57 in `rewrite.rs`.

2. **Enable `functional_deps.rs` rules** -- DISTINCT elimination
   after GROUP BY is universally safe. Uncomment line 58 in
   `rewrite.rs`.

3. **Enable `column_pruning.rs` rules** -- Projection through set
   operations is universally safe. Remove the duplicate `project-merge`
   from the column_pruning module before enabling (line 60 in
   `rewrite.rs`).

4. **Remove duplicate rules** -- Delete `duckdb-project-pushdown`
   (identical to `project-merge`) to reduce e-graph bloat.

### Short-term (new rules, moderate effort)

5. **Add window function optimization rules** -- Push filters below
   PARTITION BY, merge same-partitioned windows. Estimated 5-8 new
   rules. High value for analytics workloads.

6. **Add implied predicate generation** -- Extend `sqlite-eq-transitive`
   to generate implied range predicates across equijoins. Requires
   careful growth bounding. Estimated 3-5 new rules.

7. **Add ORDER BY elimination** -- Remove ORDER BY in subqueries
   feeding aggregates or outer queries that re-sort. Estimated 3 new
   rules.

### Medium-term (larger implementation effort)

8. **Add aggregate pushdown through join** (eager aggregation) --
   Requires column reference analysis to verify grouping columns
   come from one join side. High value for star-schema queries.

9. **Enable `semi_join.rs` rules** -- Requires the `exists` and
   `extract-join-condition` node types in the e-graph language.
   Currently blocked on e-graph language extensions.

10. **Add LIKE-to-range optimization** -- Requires string analysis
    in the rewrite conditions. Moderate implementation effort, high
    value for string-heavy workloads.

### Ongoing

11. **Bound e-graph growth** -- Add heuristic limits on commutativity
    rule application. Consider a "canonicalization pass" that runs
    commutativity rules only once rather than in the main saturation
    loop.

12. **Add priority annotations for disabled rules** -- When rules from
    recommendations 1-3 are enabled, add entries to
    `default_rule_priorities()` in `rule_priority.rs`.

## 7. Effectiveness Metrics

To measure rule effectiveness, instrument the optimizer to track:
- Per-rule fire count per query
- E-graph size contribution per rule
- Cost improvement attributable to each rule (compare extracted cost
  with and without the rule)

Rules that fire frequently but contribute minimal cost reduction are
candidates for lower priority or removal. Rules that never fire may
indicate dead patterns or missing upstream enablers.
