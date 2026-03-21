# pg_plan_advice: PostgreSQL v19 External Optimizer Integration

**Date:** 2026-03-21 (updated)
**Status:** COMMITTED to PostgreSQL 19 (March 12, 2026)
**Primary Author:** Robert Haas (robertmhaas@gmail.com)

---

## Executive Summary

`pg_plan_advice` is a contrib module **committed to PostgreSQL 19**
(March 12, 2026) that allows external tools to influence query planning
decisions without replacing the planner. It introduces a declarative
mini-language for describing plan shapes (join order, join methods,
scan types, parallelism) that can be captured from existing plans and
replayed to constrain future planning.

The module builds on foundational infrastructure committed to
PostgreSQL 19 in October 2025: extendable planner state
(`extendplan.h`), new planner lifecycle hooks (`planner_setup_hook`,
`planner_shutdown_hook`), and `ExplainState` extensibility.

**Key finding (updated 2026-03-21):** `pg_plan_advice` IS NOW
COMMITTED. Robert Haas committed the initial module on March 12,
2026 (`5883ff30`), followed by multiple bug fixes and test
infrastructure through March 19, 2026. The module is in active
stabilization with contributions from Robert Haas, Michael Paquier,
and Tom Lane.

---

## 1. Foundational Infrastructure (Committed to v19)

These changes are already in PostgreSQL master and provide the
extension points RA can use regardless of `pg_plan_advice` status.

### 1.1 Extendable Planner State (extendplan.h)

**Commit:** `0132ddd` (Robert Haas, 2025-10-07)
**Reviewers:** Andrei Lepikhov, Melanie Plageman, Tom Lane

Extensions can store private state in three core planner structures:

```c
// Get a stable numeric ID for your extension
int ext_id = GetPlannerExtensionId("ra_optimizer");

// Store/retrieve state in PlannerGlobal (per-query)
SetPlannerGlobalExtensionState(glob, ext_id, my_state);
void *state = GetPlannerGlobalExtensionState(glob, ext_id);

// Store/retrieve state in PlannerInfo (per-subquery)
SetPlannerInfoExtensionState(root, ext_id, my_state);
void *state = GetPlannerInfoExtensionState(root, ext_id);

// Store/retrieve state in RelOptInfo (per-relation)
SetRelOptInfoExtensionState(rel, ext_id, my_state);
void *state = GetRelOptInfoExtensionState(rel, ext_id);
```

This enables inter-hook communication: state set in one hook callback
is visible in subsequent callbacks for the same planning cycle.

### 1.2 Planner Lifecycle Hooks

**Commit:** `94f3ad3` (2025-10-08)

Two new hooks bracket the planning lifecycle:

```c
// Called after PlannerGlobal is fully initialized
typedef void (*planner_setup_hook_type)(
    PlannerGlobal *glob,
    Query *parse,
    const char *query_string,
    int cursorOptions,
    double *tuple_fraction,
    ExplainState *es
);

// Called before PlannerGlobal is destroyed, with final plan
typedef void (*planner_shutdown_hook_type)(
    PlannerGlobal *glob,
    Query *parse,
    const char *query_string,
    PlannedStmt *pstmt
);
```

### 1.3 ExplainState in Planner

**Commit:** `c83ac02` (2025-10-08)

`planner()` and `pg_plan_query()` now accept an `ExplainState *es`
parameter. When non-NULL, extensions can access custom EXPLAIN options
and store state that persists from EXPLAIN through planning.

### 1.4 ExplainState Extension Mechanism

**Commit:** `c65bc2e` (earlier)

Extensions can register custom EXPLAIN options and store private state
in `ExplainState`:

```c
int ext_id = GetExplainExtensionId("ra_optimizer");
SetExplainExtensionState(es, ext_id, my_state);
void *state = GetExplainExtensionState(es, ext_id);

// Register custom EXPLAIN options
RegisterExtensionExplainOption("ra_optimizer", "ra_detail",
    EXPLAIN_OPT_BOOL, ...);
```

### 1.5 Subquery Naming

**Commit:** `8c49a48` (Robert Haas, Tom Lane, 2025-10-07)

Subqueries receive stable, unique names *before* planning, enabling
correlation of plan advice with specific subqueries across planning
cycles.

### 1.6 Existing Path-Level Hooks (Pre-v19)

These hooks existed before v19 and remain available:

```c
// Inject custom paths for base relations
set_rel_pathlist_hook_type set_rel_pathlist_hook;

// Inject custom paths for joins
set_join_pathlist_hook_type set_join_pathlist_hook;

// Replace the join search algorithm entirely
join_search_hook_type join_search_hook;

// Intercept upper-level path creation
create_upper_paths_hook_type create_upper_paths_hook;

// Replace the entire planner
planner_hook_type planner_hook;
```

---

## 2. pg_plan_advice Module (Committed March 12, 2026)

### 2.1 Overview

The patch set introduces three contrib modules:

| Module | Purpose |
|--------|---------|
| `pg_plan_advice` | Generate and apply plan advice strings |
| `pg_collect_advice` | Extended advice collection mechanisms |
| `pg_stash_advice` | Store advice in DSM, auto-apply by query ID |

### 2.2 Advice Mini-Language

Plan advice is expressed as declarative constraints on planner
decisions:

```sql
-- Capture advice from an existing plan
EXPLAIN (COSTS OFF, PLAN_ADVICE) SELECT ...;
-- Output includes: Advice: JOIN_ORDER(f d) HASH_JOIN(d) SEQ_SCAN(f d)

-- Apply advice to constrain future planning
SET pg_plan_advice.advice = 'HASH_JOIN(d)';
SELECT ...;  -- planner prefers hash join for relation 'd'
```

Known advice types:

| Advice | Meaning |
|--------|---------|
| `JOIN_ORDER(a b c)` | Force specific join sequence |
| `HASH_JOIN(rel)` | Prefer hash join for relation |
| `MERGE_JOIN(rel)` | Prefer merge join for relation |
| `NESTED_LOOP(rel)` | Prefer nested loop for relation |
| `SEQ_SCAN(rel ...)` | Prefer sequential scan |
| `INDEX_SCAN(rel idx)` | Prefer specific index scan |
| `NO_GATHER(rel ...)` | Suppress Gather/Gather Merge |
| `PARALLEL(rel N)` | Set parallel workers |

Advice strings are composable: you can specify join order while
leaving join method selection to the planner.

### 2.3 Relation Identifier System

A novel contribution is the relation identifier system that
unambiguously references query components:

- Handles aliased tables, self-joins, subqueries
- Provides round-trip safety (capture -> modify -> replay)
- Could benefit other planner control modules

### 2.4 pg_stash_advice: Persistent Advice

```sql
-- Create a named stash in shared memory
SELECT pg_create_advice_stash();

-- Associate advice with a query by query_id
SELECT pg_set_stashed_advice('my_stash', query_id, advice_string);

-- Auto-apply stashed advice cluster-wide
ALTER SYSTEM SET shared_preload_libraries = 'pg_stash_advice';
ALTER SYSTEM SET pg_stash_advice.stash_name = 'my_stash';
```

Precedence: session-level `pg_plan_advice.advice` overrides stashed
advice.

### 2.5 Design Philosophy

Robert Haas emphasizes "separation of mechanism from policy":

- `pg_plan_advice` provides the mechanism (advice parsing, planner
  constraint application)
- Policy (which advice to apply, when, for which queries) is
  intentionally left to extensions like `pg_stash_advice` or
  external tools like RA
- The modules are designed to be replaceable -- alternative
  implementations can use the same infrastructure

### 2.6 Commit History and Status

- **Initial proposal:** October 30, 2025
- **Versions:** v1 through v4+ on pgsql-hackers
- **Mailing list:** 184+ messages on pgsql-hackers
- **Reviewers:** Greg Burd, Jacob Champion, Jakub Wartak,
  Lukas Fittl, Alastair Turner, and eight additional contributors
- **Blog post:** March 4, 2026 by Robert Haas
- **Status: COMMITTED to PostgreSQL 19**

Commit timeline:

| Date | Hash | Author | Description |
|------|------|--------|-------------|
| 2026-03-12 | `5883ff30` | Robert Haas | Add pg_plan_advice contrib module |
| 2026-03-17 | `5e72ce24` | Robert Haas | Fix failures to accept identifier keywords |
| 2026-03-17 | `7560995a` | Robert Haas | Fix variable type confusion |
| 2026-03-18 | `59dcc19b` | Robert Haas | Always install pg_plan_advice.h |
| 2026-03-18 | `e0e4c132` | Robert Haas | Test pg_plan_advice using test_plan_advice module |
| 2026-03-18 | `01b02c0e` | Robert Haas | Avoid a crash under GEQO |
| 2026-03-18 | `ab697307` | Robert Haas | test_plan_advice: Add .gitignore |
| 2026-03-18 | `8df3c7a8` | Michael Paquier | Exclude pgpa_parser.h from headerscheck |
| 2026-03-19 | `b335fe56` | Tom Lane | Fix multiple copy-and-paste-errors |
| 2026-03-19 | `12444183` | Robert Haas | Set TAP test priority 50 in meson |

### 2.7 External Advisor Hook API (NEW -- Critical for RA)

The committed module exports a public C API for external plugins to
supply advice programmatically, bypassing the GUC mechanism:

```c
/* pg_plan_advice.h -- installed header, available to extensions */

/* Hook type: return an advice string for a query, or NULL to defer */
typedef char *(*pg_plan_advice_advisor_hook) (
    PlannerGlobal *glob,
    Query *parse,
    const char *query_string,
    int cursorOptions,
    ExplainState *es
);

/* Register/unregister an advisor hook (PGDLLEXPORT) */
extern PGDLLEXPORT void pg_plan_advice_add_advisor(
    pg_plan_advice_advisor_hook hook);
extern PGDLLEXPORT void pg_plan_advice_remove_advisor(
    pg_plan_advice_advisor_hook hook);

/* Request that pg_plan_advice always generate advice strings */
extern PGDLLEXPORT void pg_plan_advice_request_advice_generation(
    bool activate);
```

**Integration pattern from test_plan_advice.c:**

```c
void _PG_init(void) {
    void (*add_advisor_fn)(pg_plan_advice_advisor_hook hook);
    add_advisor_fn = load_external_function(
        "pg_plan_advice",
        "pg_plan_advice_add_advisor", true, NULL);
    (*add_advisor_fn)(my_advisor_callback);
}

static char *my_advisor_callback(
    PlannerGlobal *glob, Query *parse,
    const char *query_string, int cursorOptions,
    ExplainState *es)
{
    /* Analyze the query, produce advice string */
    /* Return NULL to defer to next advisor or GUC */
    return "HASH_JOIN(t1) INDEX_SCAN(t2 idx_foo)";
}
```

Multiple advisors can be registered; the first to return non-NULL wins.
If all advisors return NULL, the `pg_plan_advice.advice` GUC is used.

### 2.8 Planner Hook Architecture

The module installs five planner hooks:

```c
void pgpa_planner_install_hooks(void) {
    planner_setup_hook    = pgpa_planner_setup;
    planner_shutdown_hook = pgpa_planner_shutdown;
    build_simple_rel_hook = pgpa_build_simple_rel;
    joinrel_setup_hook    = pgpa_joinrel_setup;
    join_path_setup_hook  = pgpa_join_path_setup;
}
```

- `planner_setup_hook`: Parses advice strings into a "trove"
  (indexed advice structure) before planning begins
- `build_simple_rel_hook`: Applies scan advice by modifying
  `rel->pgs_mask` to constrain available scan methods
- `joinrel_setup_hook`: Enforces GATHER, NO_GATHER,
  PARTITIONWISE directives via `joinrel->pgs_mask`
- `join_path_setup_hook`: Enforces join method restrictions
  (HASH_JOIN, MERGE_JOIN, NESTED_LOOP) via `extra->pgs_mask`
- `planner_shutdown_hook`: Post-planning analysis generating
  advice strings and feedback

### 2.9 Complete Advice Tag Taxonomy

From the committed `pgpa_ast.h`, 19 advice tags are supported:

**Scan advice:**
- `SEQ_SCAN(rel)` -- Force sequential scan
- `INDEX_SCAN(rel idx)` -- Force specific index scan
- `INDEX_ONLY_SCAN(rel idx)` -- Force index-only scan
- `BITMAP_HEAP_SCAN(rel)` -- Force bitmap heap scan
- `TID_SCAN(rel)` -- Force TID scan
- `FOREIGN_SCAN(rel)` -- Force foreign scan

**Join method advice:**
- `HASH_JOIN(rel)` -- Force hash join
- `MERGE_JOIN(rel)` -- Force merge join
- `MERGE_JOIN_MATERIALIZE(rel)` -- Force merge join with materialize
- `NESTED_LOOP(rel)` -- Force nested loop (plain)
- `NESTED_LOOP_MATERIALIZE(rel)` -- Force nested loop with materialize
- `NESTED_LOOP_MEMOIZE(rel)` -- Force nested loop with memoize

**Join order advice:**
- `JOIN_ORDER(a b c)` -- Force join sequence (parenthesized = ordered)
- `JOIN_ORDER({a b} c)` -- Braces = unordered group

**Parallelism advice:**
- `GATHER(rel ...)` -- Force Gather node
- `GATHER_MERGE(rel ...)` -- Force Gather Merge node
- `NO_GATHER(rel ...)` -- Suppress parallel execution

**Partitionwise advice:**
- `PARTITIONWISE(rel ...)` -- Enable partitionwise join

**Semijoin advice:**
- `SEMIJOIN_UNIQUE(rel)` -- Treat inner as unique for semijoin
- `SEMIJOIN_NON_UNIQUE(rel)` -- Do not treat inner as unique

### 2.10 Strategy Mask System

The module uses `uint64` strategy masks (`pgs_mask`) attached to
`RelOptInfo` and `JoinPathExtraData` to enable/disable specific
scan and join algorithms at the per-relation level. This provides
finer-grained control than the global `enable_*` GUCs.

### 2.11 Trove Data Structure

Parsed advice is stored in a "trove" indexed by advice type and
relation identifiers. Lookup results carry status flags:

- `PGPA_TE_MATCH_PARTIAL` (0x0001) -- Partial query match
- `PGPA_TE_MATCH_FULL` (0x0002) -- Exact target match
- `PGPA_TE_INAPPLICABLE` (0x0004) -- Advice doesn't apply
- `PGPA_TE_CONFLICTING` (0x0008) -- Conflicts with other advice
- `PGPA_TE_FAILED` (0x0010) -- Final plan didn't conform

These flags enable the feedback loop: after planning, the module
reports which advice was applied, which was inapplicable, and which
conflicted, visible through `EXPLAIN (PLAN_ADVICE)`.

---

## 3. Integration Guide for RA

### 3.1 Architecture Options

**Option A: Use pg_plan_advice (if committed)**

```
RA Engine --> Advice Extractor --> pg_plan_advice.advice GUC
                                        |
                                        v
                                  PG Planner (constrained)
```

**Option B: Use committed hooks directly (works regardless)**

```
RA Engine --> pgrx Extension --> planner_setup_hook
                             --> set_rel_pathlist_hook
                             --> set_join_pathlist_hook
                             --> planner_shutdown_hook
```

**Option C: Hybrid (recommended)**

Use pg_plan_advice when available, fall back to direct hooks.
Support both `pg_hint_plan` (pre-v19) and `pg_plan_advice` (v19+).

### 3.2 Hook Integration via pgrx

The pgrx extension (RFC 0002) can use the new v19 hooks:

```rust
use pgrx::prelude::*;

static mut PREV_SETUP: Option<planner_setup_hook_type> = None;
static mut PREV_SHUTDOWN: Option<planner_shutdown_hook_type> = None;

#[pg_guard]
pub unsafe extern "C" fn ra_planner_setup(
    glob: *mut PlannerGlobal,
    parse: *mut Query,
    query_string: *const c_char,
    cursor_options: c_int,
    tuple_fraction: *mut f64,
    es: *mut ExplainState,
) {
    // Chain to previous hook
    if let Some(prev) = PREV_SETUP {
        prev(glob, parse, query_string,
             cursor_options, tuple_fraction, es);
    }

    // Register RA extension state
    let ext_id = GetPlannerExtensionId(
        c"ra_optimizer".as_ptr()
    );

    // Parse query, run RA optimizer, store advice
    let ra_state = Box::new(RaOptimizerState::new(parse));
    SetPlannerGlobalExtensionState(
        glob, ext_id, Box::into_raw(ra_state) as *mut _
    );
}

#[pg_guard]
pub unsafe extern "C" fn ra_planner_shutdown(
    glob: *mut PlannerGlobal,
    parse: *mut Query,
    query_string: *const c_char,
    pstmt: *mut PlannedStmt,
) {
    // Retrieve RA state, compare predicted vs actual plan
    let ext_id = GetPlannerExtensionId(
        c"ra_optimizer".as_ptr()
    );
    let state = GetPlannerGlobalExtensionState(glob, ext_id);

    // Collect feedback for RA cost model calibration
    if !state.is_null() {
        let ra_state = &*(state as *const RaOptimizerState);
        ra_state.record_plan_feedback(pstmt);
        // Clean up
        drop(Box::from_raw(state as *mut RaOptimizerState));
    }

    // Chain to previous hook
    if let Some(prev) = PREV_SHUTDOWN {
        prev(glob, parse, query_string, pstmt);
    }
}
```

### 3.3 Path Injection via set_rel_pathlist_hook

For scan method advice:

```rust
#[pg_guard]
pub unsafe extern "C" fn ra_set_rel_pathlist(
    root: *mut PlannerInfo,
    rel: *mut RelOptInfo,
    rti: Index,
    rte: *mut RangeTblEntry,
) {
    // Chain to previous hook
    if let Some(prev) = PREV_REL_PATHLIST {
        prev(root, rel, rti, rte);
    }

    // Check if RA has advice for this relation
    let ext_id = GetPlannerExtensionId(
        c"ra_optimizer".as_ptr()
    );
    let state = GetRelOptInfoExtensionState(rel, ext_id);

    // Modify path costs or add custom paths based on
    // RA optimization results
}
```

### 3.4 pg_plan_advice Integration (When Available)

If `pg_plan_advice` is committed, RA can generate advice strings:

```rust
pub fn generate_advice(
    optimized: &RelExpr,
    query_info: &QueryInfo,
) -> String {
    let mut advice = Vec::new();

    // Extract join order
    let join_order = extract_join_order(optimized);
    advice.push(format!("JOIN_ORDER({})",
        join_order.join(" ")));

    // Extract join methods
    for join in extract_joins(optimized) {
        let method = match join.join_type {
            JoinType::Hash => "HASH_JOIN",
            JoinType::Merge => "MERGE_JOIN",
            JoinType::NestedLoop => "NESTED_LOOP",
        };
        advice.push(format!("{}({})",
            method, join.relation_id));
    }

    // Extract scan methods
    for scan in extract_scans(optimized) {
        match scan.scan_type {
            ScanType::Sequential =>
                advice.push(format!("SEQ_SCAN({})",
                    scan.relation_id)),
            ScanType::Index(idx) =>
                advice.push(format!("INDEX_SCAN({} {})",
                    scan.relation_id, idx)),
            _ => {}
        }
    }

    advice.join(" ")
}
```

### 3.5 Workflow Diagram

```
  +------------------+
  |  pg_stat_stmts   |   Monitor slow queries
  +--------+---------+
           |
           v
  +------------------+
  |  RA Background   |   Background worker polls for
  |  Worker          |   queries exceeding cost threshold
  +--------+---------+
           |
           v
  +------------------+
  |  RA Parser       |   Parse SQL to RA IR
  +--------+---------+
           |
           v
  +------------------+
  |  RA Optimizer    |   Apply optimization rules
  |  (rule engine)   |   (join reorder, predicate pushdown, etc.)
  +--------+---------+
           |
           v
  +------------------+
  |  Advice          |   Convert optimized plan to advice string
  |  Extractor       |   or path modifications
  +--------+---------+
           |
     +-----+------+
     |            |
     v            v
  [Option A]   [Option B]
  pg_plan_     Direct hook
  advice GUC   path injection
     |            |
     v            v
  +------------------+
  |  PG Planner      |   Plans query with RA guidance
  +--------+---------+
           |
           v
  +------------------+
  |  PG Executor     |   Executes plan
  +--------+---------+
           |
           v
  +------------------+
  |  Feedback Loop   |   Compare predicted vs actual
  |  (shutdown hook) |   Calibrate RA cost model
  +------------------+
```

---

## 4. Existing Alternatives

### 4.1 pg_hint_plan (Works Today, PG 9.6-17)

- Mature extension with wide adoption
- Hint syntax embedded in SQL comments: `/*+ SeqScan(a) */`
- Supports: scan hints, join hints, leading (join order),
  row count correction, parallel control, GUC setting
- Uses `post_parse_analyze_hook` and `planner_hook`
- Limitation: requires modifying SQL text

### 4.2 Custom Scan Provider API (Works Today)

- Extension modules can add custom scan types
- Three-phase: path creation -> plan conversion -> execution
- Full C API with callbacks for planning, execution, EXPLAIN
- Supports parallel execution
- Limitation: designed for alternative scan implementations,
  not general plan advice

### 4.3 GUC Parameters (Works Today)

- `enable_seqscan`, `enable_indexscan`, `enable_hashjoin`, etc.
- Blunt instruments: affect all tables in a query
- No per-relation or per-join granularity
- Useful for quick experiments, not production tuning

### 4.4 Injection Points (v17+)

- General mechanism for extensions to attach callbacks at
  specific server code points
- Requires `--enable-injection-points` at compile time
- More suited for testing/debugging than production optimization

---

## 5. References

### Committed Code (PostgreSQL master/v19)

1. **extendplan.h** -- Extension private state in planner structures
   - Commit: `0132ddd` (Robert Haas, 2025-10-07)
   - Files: `src/include/optimizer/extendplan.h`,
     `src/backend/optimizer/util/extendplan.c`

2. **planner_setup_hook / planner_shutdown_hook**
   - Commit: `94f3ad3` (2025-10-08)
   - Files: `src/include/optimizer/planner.h`,
     `src/backend/optimizer/plan/planner.c`

3. **ExplainState in planner**
   - Commit: `c83ac02` (2025-10-08)

4. **Subquery naming**
   - Commit: `8c49a48` (Robert Haas, Tom Lane, 2025-10-07)

### Committed (March 12, 2026)

5. **pg_plan_advice contrib module**
   - Commit: `5883ff30` (Robert Haas, 2026-03-12)
   - Files: `contrib/pg_plan_advice/` (24 source files)
   - Test module: `src/test/modules/test_plan_advice/`
   - Installed header: `pg_plan_advice.h`
   - Thread: pgsql-hackers, starting 2025-10-30 (184+ messages)
   - Message-ID: `CA+TgmoZ-Jh1T6QyWoCODMVQdhTUPYkaZjWztzP1En4=ZHoKPzw@mail.gmail.com`
   - Blog post: https://rhaas.blogspot.com/2026/03/pgplanadvice-plan-stability-and-user.html

### Related Projects

6. **pg_hint_plan** -- https://github.com/ossc-db/pg_hint_plan
7. **Custom Scan Provider** -- https://www.postgresql.org/docs/devel/custom-scan.html
8. **PostgreSQL Planner Hooks** -- `src/include/optimizer/planner.h`,
   `src/include/optimizer/paths.h`

---

## 6. Recommendations for RA (Updated 2026-03-21)

Now that `pg_plan_advice` is committed, the integration path is clear:

1. **Primary path: Advisor hook.** Implement a pgrx extension that
   calls `pg_plan_advice_add_advisor()` to register RA as an advisor.
   The advisor callback receives the full query context and returns
   an advice string. This is the canonical integration mechanism --
   `test_plan_advice.c` demonstrates the exact pattern.

2. **Advice string generation.** Implement the `AdviceExtractor` that
   walks RA's optimized `RelExpr` tree and emits advice in the
   committed mini-language format (19 tag types). The advice format
   is now stable since it's committed code.

3. **Confidence-gated advice.** Only emit advice when RA's optimizer
   has high confidence. Return NULL from the advisor callback to defer
   to the GUC or other advisors. The multi-advisor chain makes this
   safe.

4. **Feedback loop.** Use `planner_shutdown_hook` to compare RA's
   predictions with the final plan. The trove status flags
   (`MATCH_FULL`, `INAPPLICABLE`, `CONFLICTING`, `FAILED`) provide
   structured feedback for cost model calibration.

5. **Deployment via pg_stash_advice.** For production, use
   `pg_stash_advice` to automatically apply RA's advice by query ID
   cluster-wide. RA's background worker writes to the stash; no
   per-query function calls needed.

6. **Fallback for PG 15-18.** Support `pg_hint_plan` with a thin
   translation layer. The hint categories map directly to
   `pg_plan_advice` tags.
