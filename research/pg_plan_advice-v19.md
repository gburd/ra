# pg_plan_advice: PostgreSQL v19 External Optimizer Integration

**Date:** 2026-03-20
**Status:** Active development (not yet committed to PostgreSQL 19)
**Primary Author:** Robert Haas (robertmhaas@gmail.com)

---

## Executive Summary

`pg_plan_advice` is a proposed contrib module for PostgreSQL 19 that
allows external tools to influence query planning decisions without
replacing the planner. It introduces a declarative mini-language for
describing plan shapes (join order, join methods, scan types,
parallelism) that can be captured from existing plans and replayed to
constrain future planning.

The patch set builds on foundational infrastructure committed to
PostgreSQL 19 in October 2025: extendable planner state
(`extendplan.h`), new planner lifecycle hooks (`planner_setup_hook`,
`planner_shutdown_hook`), and `ExplainState` extensibility. These
pieces are already in PostgreSQL master and will ship with v19
regardless of whether `pg_plan_advice` itself is accepted.

**Key finding:** `pg_plan_advice` is NOT yet committed. It is an
active proposal (178+ messages on pgsql-hackers, multiple patch
revisions). The foundational hooks it depends on ARE committed.

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

## 2. pg_plan_advice Proposal (Not Yet Committed)

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

### 2.6 Patch Set Status

- **Initial proposal:** October 30, 2025
- **Versions:** v1 through v4+ (ongoing)
- **Mailing list:** 178+ messages on pgsql-hackers
- **Reviewers:** Jakub Wartak, Alastair Turner, Hannu Krosing,
  John Naylor, Matheus Alcantara, Jacob Champion, and others
- **Blog post:** March 4, 2026 by Robert Haas
- **Status:** Under review, not committed. May ship with v19 or
  be deferred.

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

### Proposed (Under Review)

5. **pg_plan_advice patch set**
   - Thread: pgsql-hackers, starting 2025-10-30
   - Message-ID: `CA+TgmoZ-Jh1T6QyWoCODMVQdhTUPYkaZjWztzP1En4=ZHoKPzw@mail.gmail.com`
   - Blog post: https://rhaas.blogspot.com/2026/03/pgplanadvice-plan-stability-and-user.html

### Related Projects

6. **pg_hint_plan** -- https://github.com/ossc-db/pg_hint_plan
7. **Custom Scan Provider** -- https://www.postgresql.org/docs/devel/custom-scan.html
8. **PostgreSQL Planner Hooks** -- `src/include/optimizer/planner.h`,
   `src/include/optimizer/paths.h`

---

## 6. Recommendations for RA

1. **Immediate:** Build the pgrx extension (RFC 0002) targeting the
   committed v19 hooks (`planner_setup_hook`, `planner_shutdown_hook`,
   `set_rel_pathlist_hook`, `set_join_pathlist_hook`, extendable
   planner state). These are stable API.

2. **Short-term:** Implement advice string generation that outputs
   `pg_plan_advice` format. Even if the module is not committed,
   the format is well-documented and can be used with `pg_hint_plan`
   translation or direct hook injection.

3. **Medium-term:** If `pg_plan_advice` ships with v19, implement
   the `pg_stash_advice` integration path for automatic advice
   application by query ID. This is the lowest-friction deployment
   model.

4. **Fallback:** Support `pg_hint_plan` for PostgreSQL 15-18 users.
   The hint categories map closely to `pg_plan_advice` advice types.
   Build an abstraction layer that emits either format.
