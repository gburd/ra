# RFC 0003: pg_plan_advice Integration

- **Status:** Accepted
- **Type:** Prospective
- **Author:** RA Contributors
- **Date:** 2026-03-20
- **Tracking:** Phase 5 of deployment plan

---

## Summary

Integrate the RA optimizer with PostgreSQL's `pg_plan_advice`
mechanism (available in PostgreSQL v19+) to supply optimizer hints
without replacing the planner. RA analyzes queries, identifies
suboptimal plan choices, and injects advice that steers PostgreSQL's
native planner toward better plans.

## Motivation

Full planner replacement (RFC 0002) carries risk: incorrect plan
conversion can cause query failures. `pg_plan_advice` offers a
safer alternative where RA supplies *hints* -- join order
suggestions, scan method preferences, parallelism settings -- and
PostgreSQL's planner makes the final decision.

This approach:

- Reduces the blast radius of optimizer bugs (PG validates all plans)
- Works alongside other extensions that modify planning
- Provides a migration path from advisory mode to full replacement
- Leverages PostgreSQL's built-in plan validation

## Guide-Level Explanation

```sql
-- Load the extension
CREATE EXTENSION ra_advisor;

-- RA analyzes the query and registers plan advice
SELECT ra_advisor.advise($$
  SELECT * FROM orders o
  JOIN customers c ON o.customer_id = c.id
  WHERE o.total > 1000
$$);

-- PostgreSQL uses the advice during planning
EXPLAIN SELECT * FROM orders o
  JOIN customers c ON o.customer_id = c.id
  WHERE o.total > 1000;
```

The `advise()` function:

1. Parses the query through RA
2. Optimizes using the full rule set
3. Extracts plan decisions (join order, scan methods, etc.)
4. Registers them as `pg_plan_advice` entries
5. Returns a summary of advice given

For transparent operation, a background worker can advise all
queries automatically:

```sql
SET ra_advisor.auto_advise = on;
```

## Reference-Level Explanation

### Advice Categories

The integration produces advice in the `pg_plan_advice` mini-language
format. RA maps its optimized `RelExpr` tree to these advice types:

| Category | pg_plan_advice format | RA source |
|----------|----------------------|-----------|
| Join order | `JOIN_ORDER(o c)` | RelExpr join tree order |
| Hash join | `HASH_JOIN(c)` | PhysicalHashJoin node |
| Merge join | `MERGE_JOIN(c)` | PhysicalMergeJoin node |
| Nested loop | `NESTED_LOOP(c)` | PhysicalNestedLoop node |
| Seq scan | `SEQ_SCAN(o)` | PhysicalTableScan node |
| Index scan | `INDEX_SCAN(o idx)` | PhysicalIndexScan node |
| No gather | `NO_GATHER(o c)` | Serial execution preference |
| Parallelism | `PARALLEL(o 4)` | Parallel scan degree |

### Architecture

```
Query --> RA Parser --> RA Optimizer --> Advice Extractor
                                            |
                                            v
                                   pg_plan_advice API
                                            |
                                            v
                                   PG Native Planner
                                   (with advice applied)
```

### Advice Extraction

After RA optimization, the `AdviceExtractor` walks the optimized
`RelExpr` tree and emits `PlanAdvice` entries:

```rust
pub struct PlanAdvice {
    pub query_hash: u64,
    pub advice_type: AdviceType,
    pub target: String,
    pub recommendation: String,
    pub confidence: f64,
}
```

Only advice with confidence above a configurable threshold is
registered (default: 0.8).

### Feedback Integration

When `EXPLAIN ANALYZE` data is available, the extension compares
RA's predictions with actual execution:

- If RA's advice improved performance: increase confidence
- If RA's advice degraded performance: decrease confidence and
  potentially withdraw the advice
- Feedback is stored in a local table for trend analysis

### Background Worker

An optional background worker monitors `pg_stat_statements` and
proactively advises queries that exceed a cost threshold. This
enables transparent optimization without per-query function calls.

## Drawbacks

- Requires PostgreSQL v19+ (not yet released as of 2026-03)
- `pg_plan_advice` API may change before PostgreSQL v19 GA
- Advice is coarser than full plan replacement -- some optimizations
  cannot be expressed as hints
- Background worker adds overhead to query monitoring

## Rationale and Alternatives

**Alternative: pg_hint_plan.** Works on current PostgreSQL versions
but uses a non-standard hint syntax and requires embedding hints in
SQL comments. `pg_plan_advice` is the official PostgreSQL mechanism.

**Alternative: Full planner replacement (RFC 0002).** More powerful
but higher risk. The two approaches are complementary: start with
advice, graduate to full replacement for validated workloads.

## Prior Art

- **Oracle SQL Plan Baselines** -- stores known-good plans and
  prevents plan regressions; similar advisory concept
- **SQL Server Plan Guides** -- attaches hints to queries by
  matching query text
- **pg_hint_plan** -- community extension for PostgreSQL hint
  injection

## Unresolved Questions

- `pg_plan_advice` is not yet committed to PostgreSQL v19. The
  foundational hooks it depends on (planner_setup_hook,
  planner_shutdown_hook, extendable planner state) ARE committed.
  Should RA target the committed hooks directly as a fallback?
- The advice mini-language syntax may change before final commit.
  RA should generate advice through an abstraction layer.
- How should advice be scoped (session, database, cluster)?
  `pg_stash_advice` provides cluster-wide scoping via DSM, but
  RA may need finer-grained control.
- How to handle parameterized queries where optimal advice depends
  on parameter values?
- Should RA also support `pg_hint_plan` for PostgreSQL 15-18 users?
  The hint categories map closely to `pg_plan_advice` advice types.

## Research Findings (2026-03-20)

See `research/pg_plan_advice-v19.md` for detailed findings. Key
points:

- **Committed to v19:** `extendplan.h` (private state in
  PlannerGlobal, PlannerInfo, RelOptInfo), `planner_setup_hook`,
  `planner_shutdown_hook`, ExplainState extensibility, subquery
  naming.
- **Proposed (under review):** `pg_plan_advice`, `pg_collect_advice`,
  `pg_stash_advice` contrib modules by Robert Haas. 178+ messages
  on pgsql-hackers, multiple patch revisions (v1-v4+).
- **Advice format:** Declarative mini-language -- `JOIN_ORDER(a b)`,
  `HASH_JOIN(rel)`, `SEQ_SCAN(rel)`, `INDEX_SCAN(rel idx)`,
  `NO_GATHER(rel)`, `PARALLEL(rel N)`.
- **Integration mechanism:** GUC `pg_plan_advice.advice` for
  per-session, `pg_stash_advice` for per-query-id cluster-wide.

## Future Possibilities

- Workload-aware advice that considers query interactions
- Automatic plan regression detection and rollback
- Advice sharing across PostgreSQL replicas
- Integration with query regression detection (RFC 0013)
- Direct hook integration (bypassing pg_plan_advice) using
  committed v19 infrastructure for maximum control
