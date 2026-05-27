# Porting `pg_plan_advice` to Ra

## Source under study

- `~/src/postgres/contrib/pg_plan_advice/` — 7,568 lines of C across
  9 .c files, 8 .h files, plus a Bison/Flex parser pair
  (`pgpa_parser.y`, `pgpa_scanner.l`).
- `~/src/postgres/src/test/modules/test_plan_advice/` — meta-test
  module that re-plans every query through generated advice to catch
  round-trip regressions during the buildfarm runs.
- `~/src/postgres/doc/src/sgml/pgplanadvice.sgml` — 825-line user
  manual.
- `~/src/postgres/src/include/nodes/pathnodes.h` — `PGS_*`
  constants and the `default_pgs_mask` / `pgs_mask` fields that
  pg_plan_advice manipulates.
- `~/src/postgres/src/include/optimizer/planner.h` and
  `extendplan.h` — the extension hook surface.

## What pg_plan_advice actually is

A contrib module (not core, not loaded by default) that adds a
**plan-advice mini-language** for round-trip-safe steering of the
PostgreSQL planner. Three jobs in one package:

1. **Generation.** Walk a finished plan and emit an advice string
   that reproduces it.
2. **Application.** Parse a supplied advice string and constrain the
   planner so it can only choose from plans that match.
3. **Feedback.** Tell the user whether the advice was matched,
   partially matched, not matched, inapplicable, conflicting, or
   failed.

The framing is "plan stability" but the implementation is broader:
join-order constraints, scan-method overrides, parallel-query
control, partitionwise opt-in/out, and semijoin uniqueness all flow
through the same primitive. PG's `test_plan_advice` test module
re-plans every query in `make check` through generated advice as a
correctness oracle — if generated advice fails to reproduce its own
plan, the whole test rig breaks the buildfarm.

The mini-language design choices are deliberate:

- **Round-trip safe.** Generate from any plan, feed back later, get
  the same plan (assuming no DDL changed underneath).
- **No inference from absence.** Removing one tag relaxes the
  constraint; tags must be explicit in both directions (e.g.
  `SEMIJOIN_UNIQUE` vs `SEMIJOIN_NON_UNIQUE`).
- **Imperative for the user, declarative for the planner.** Users
  write "do X." The planner reads "X is allowed; everything else is
  forbidden." Advice never causes planner failure — disabled-but-
  selected plans are flagged in EXPLAIN as `Disabled: true`.
- **Constrains, never replaces.** No advice can force the planner
  to consider a plan it never had on its menu (semantic correctness
  remains the planner's job).

## How it integrates with PG's planner

### The strategy mask (`pgs_mask`)

PG19 added a `uint64 pgs_mask` field on every `RelOptInfo` (base
and join), plus a `default_pgs_mask` on `PlannerGlobal`
(`pathnodes.h:263, 1039, 3626`). 23 bits are defined
(`pathnodes.h:66-86`):

| Bit | Meaning |
|---|---|
| `PGS_SEQSCAN` (0x1) | Allow seq scans |
| `PGS_INDEXSCAN` (0x2) | Allow index scans |
| `PGS_INDEXONLYSCAN` (0x4) | Allow index-only scans |
| `PGS_BITMAPSCAN` (0x8) | Allow bitmap heap scans |
| `PGS_TIDSCAN` (0x10) | Allow TID scans |
| `PGS_FOREIGNJOIN` (0x20) | Allow foreign-table join pushdown |
| `PGS_MERGEJOIN_PLAIN` (0x40) | Allow merge join, no materialize |
| `PGS_MERGEJOIN_MATERIALIZE` (0x80) | Allow merge join with materialize |
| `PGS_NESTLOOP_PLAIN` (0x100) | Allow nestloop, no materialize/memoize |
| `PGS_NESTLOOP_MATERIALIZE` (0x200) | Allow nestloop + materialize |
| `PGS_NESTLOOP_MEMOIZE` (0x400) | Allow nestloop + memoize |
| `PGS_HASHJOIN` (0x800) | Allow hash joins |
| `PGS_APPEND` (0x1000) | Allow Append plan |
| `PGS_MERGE_APPEND` (0x2000) | Allow Merge Append |
| `PGS_GATHER` (0x4000) | Allow Gather node |
| `PGS_GATHER_MERGE` (0x8000) | Allow Gather Merge |
| `PGS_CONSIDER_INDEXONLY` (0x10000) | Permit considering index-only |
| `PGS_CONSIDER_PARTITIONWISE` (0x20000) | Permit partitionwise join |
| `PGS_CONSIDER_NONPARTIAL` (0x40000) | Permit non-partial paths |

`costsize.c` reads these bits during `cost_seqscan`, `cost_indexscan`,
etc. — when a forbidden strategy is costed, the function adds
`disable_cost` (a large constant), so the path becomes
unattractive but is not removed from the tree. This is what
produces `Disabled: true` in EXPLAIN output when supplied advice
forces an impossible plan: the planner picks the disabled plan
because nothing else was permitted.

### The hook surface

pg_plan_advice installs five hooks (all defined in
`extendplan.h` and `planner.h`):

| Hook | Fires | Purpose for advice |
|---|---|---|
| `planner_setup_hook` | Once at planner entry | Parse supplied advice into a "trove"; allocate per-query state; record whether to generate advice/feedback |
| `build_simple_rel_hook` | Each base `RelOptInfo` | Look up rel-targeting advice; clear `PGS_*` bits to enforce scan-method choice |
| `joinrel_setup_hook` | Each join `RelOptInfo` | Apply join-order and join-method advice; clear `PGS_*` join bits |
| `join_path_setup_hook` | Each candidate `JoinPath` | Final filter on join-method choice with full inner/outer info |
| `planner_shutdown_hook` | Once at planner exit | Walk the produced `PlannedStmt`, generate advice string, compute feedback flags, attach both to `PlannedStmt->extension_state` |

### Storage path through `PlannedStmt`

Output flows back to EXPLAIN via `PlannedStmt->extension_state`,
which is a `List *DefElem`. pg_plan_advice appends one `DefElem`
keyed `"pg_plan_advice"` whose value is itself a list with up to
two entries:

- `("advice_string" . StringValue)` — generated advice text
- `("feedback" . List)` — per-target feedback flags

EXPLAIN integration is via two PG18+ APIs:

- `RegisterExtensionExplainOption("plan_advice", handler, checker)`
  registers a custom EXPLAIN option that handles
  `EXPLAIN (PLAN_ADVICE) ...`.
- `explain_per_plan_hook` lets the module render its block
  alongside the plan tree.

### Identifiers

A relation reference in an advice string must uniquely identify a
single `RangeTblEntry` in the produced plan. PG uses:

```text
alias_name#occurrence_number/partition_schema.partition_name@plan_name
```

Components after `alias_name` are optional, included only when
needed (multiple occurrences in one subquery; partitioned children;
non-top-level subplan). Generated advice always includes whatever
disambiguation is required; user-written advice may omit the
schema for partitions and the plan name for the top-level subquery.

This is implemented in `pgpa_identifier.c` (481 lines) — the trove
keys on these identifiers.

## What "honor it" means for Ra

Three integration directions, in dependency order:

### Direction 1: emit advice from Ra-produced plans (the easiest)

After `Optimizer::optimize` returns a `RelExpr`, walk it and
produce a string compatible with PG's `pgpa` grammar. This is
useful *before* Ra has any consume/honor support: a Ra-produced
plan can be exported as advice, fed to a PG instance running
pg_plan_advice, and used to verify that PG would have produced the
same plan if Ra wasn't in the picture. It's a one-way diagnostic
oracle.

### Direction 2: consume advice in Ra's planner (the hard middle)

Parse a pg_plan_advice string and constrain Ra's optimizer so the
plan it extracts respects the advice. This requires:

- A faithful parser for the pgpa grammar
- A trove-equivalent lookup structure
- A `PgsMask`-equivalent bit field on Ra's `RelExpr` or carried in
  optimizer state
- Rewrite-rule and extraction-time gates that read the mask
- Feedback computation by walking the extracted `RelExpr`

### Direction 3: honor it as a planner/parser limit (the broadest)

This is the piece the user emphasised. pg_plan_advice's central
trick is that it doesn't replace the planner — it constrains the
planner's *menu*. Ra has the same shape: the rewrite rule set is
the menu, and the extracted plan is the dish. To honor advice:

- **At the parser level**, refuse to even parse advice that
  references identifiers not present in the query (defensive — PG
  parses freely and reports `inapplicable` later, but Ra can be
  stricter because mismatched identifiers are almost always a
  user error).
- **At the rule-advisor level**, demote rules whose application
  would violate any advice constraint (e.g. join-reorder rules
  when JOIN_ORDER is supplied).
- **At the cost extraction level**, score plans that violate
  advice with a large penalty so they only get extracted as a
  fallback (mirrors PG's `disable_cost` behaviour).
- **At the speculative router**, add a "honor advice" route that
  bypasses the saturation budget and goes straight to advice-
  constrained construction.

## The full task list

Tasks are sized to ~1 day of focused work each unless noted.
Numbering is a suggested implementation order: each task uses
output from earlier ones.

### Phase 0 — Audit and design

#### Task 0.1 — Read PG sources end-to-end

Walk through, in order: `pg_plan_advice.c` (entry/init),
`pgpa_planner.c` (hooks and mask manipulation), `pgpa_walker.c`
(plan-tree walk for advice generation), `pgpa_trove.c` (lookup
structure), `pgpa_identifier.c` (identifier matching),
`pgpa_ast.h` (the typed AST), `pgpa_parser.y` and `pgpa_scanner.l`
(grammar). Write a 2-page summary in
`docs/research/pg-plan-advice-internals.md` capturing the data
flow, the trove structure, and the mask-manipulation rules. *Done
once, referenced everywhere downstream.*

#### Task 0.2 — Decide on identifier syntax compatibility

Ra's parser is Lime, not Bison; Ra's rule names look different
from PG's table aliases; Ra has no notion of a "subplan name."
Pick one of three options and document the choice in
`docs/research/pg-plan-advice-internals.md`:

(a) **Strict compatibility.** Ra accepts the exact PG syntax
    `alias#n/schema.name@plan`. Every concept on the PG side has
    a Ra equivalent or a documented degeneration. Best for
    interop.

(b) **Subset compatibility.** Ra accepts the simple identifier
    form (`alias_name` only, no `#`/`/`/`@`); the disambiguators
    are deferred. Cheaper to implement, but advice generated by
    Ra cannot round-trip to PG and vice versa for any non-
    trivial query.

(c) **Superset compatibility.** Ra accepts PG syntax plus
    Ra-specific extensions (e.g. `RULE_GROUP(name)` for
    rule-advisor demotion). Most powerful, hardest to validate
    against `test_plan_advice`-style oracles.

**Recommendation: (a) for v1, with stub fields for (c) extensions
to be added in v2.** Round-trip safety is the design property that
makes the test_plan_advice-style oracle work; throwing it away in
v1 makes everything later harder.

### Phase 1 — Parser, AST, identifiers (~1 week)

#### Task 1.1 — `pgpa_ast` Rust types

Port `pgpa_ast.h` to a new crate `ra-plan-advice` (experimental
layer). Types to mirror, in `crates/ra-plan-advice/src/ast.rs`:

```rust
pub enum AdviceTag {
    BitmapHeapScan, DoNotScan, ForeignJoin, Gather, GatherMerge,
    HashJoin, IndexOnlyScan, IndexScan, JoinOrder,
    MergeJoinMaterialize, MergeJoinPlain,
    NestedLoopMaterialize, NestedLoopMemoize, NestedLoopPlain,
    NoGather, Partitionwise, SemijoinNonUnique, SemijoinUnique,
    SeqScan, TidScan,
}

pub enum AdviceTargetKind {
    Identifier(RelationIdentifier),
    OrderedList(Vec<AdviceTarget>),    // ( a b c )
    UnorderedList(Vec<AdviceTarget>),  // { a b c }
}

pub struct RelationIdentifier {
    pub alias: String,
    pub occurrence: Option<u32>,
    pub partition_schema: Option<String>,
    pub partition_name: Option<String>,
    pub plan_name: Option<String>,
}

pub struct AdviceTarget {
    pub kind: AdviceTargetKind,
    pub index: Option<IndexTarget>,    // for INDEX_SCAN/INDEX_ONLY_SCAN
}

pub struct IndexTarget { pub schema: Option<String>, pub name: String }

pub struct AdviceItem { pub tag: AdviceTag, pub targets: Vec<AdviceTarget> }
pub type Advice = Vec<AdviceItem>;
```

Bit-for-bit identical semantics to the C structs.

#### Task 1.2 — Parser

Reuse Ra's existing Lime grammar pipeline. Add a
new grammar file `crates/ra-plan-advice/grammar/pgpa.lime`
implementing the pgpa syntax. The grammar is simple — no
operator precedence, no left-recursion ambiguities; the
PG flex/bison version is ~200 lines combined. Reachable bound:
roughly 250 lines of Lime + 100 lines of post-parse semantic
checks (verify metavariables, check that `INDEX_SCAN` carries an
index, etc.).

Output: `pub fn parse_advice(input: &str) -> Result<Advice,
ParseError>` in `crates/ra-plan-advice/src/parser.rs`. Match
PG's error-message phrasing where reasonable so error log
filtering between PG and Ra stays compatible.

Tests: every test in `~/src/postgres/contrib/pg_plan_advice/sql/syntax.sql`
plus the canonical examples from `pgplanadvice.sgml`.

#### Task 1.3 — `RelationIdentifier` resolution against `RelExpr`

Implement `pub fn resolve_identifier(id: &RelationIdentifier, expr:
&RelExpr) -> Option<RelExprPath>` where `RelExprPath` is a stable
description of which `RelExpr::Scan` (or join, etc.) the
identifier picks out. This is the equivalent of
`pgpa_identifier.c`. Critical pieces:

- Walk RelExpr collecting (alias, occurrence, partition?, plan?)
  tuples per scan.
- Match by alias first, disambiguate by occurrence, then partition,
  then plan.
- Return `None` for an identifier that doesn't match anything in
  the current query (PG's `inapplicable` feedback case).

Ra has no first-class subplan concept, so the `@plan` component
maps to either a CTE name or a derived-table alias depending on
context. Document this mapping in
`docs/research/pg-plan-advice-internals.md` Task 0.1.

### Phase 2 — Trove (~3 days)

#### Task 2.1 — Trove data structure

Port `pgpa_trove.c` (518 lines) to
`crates/ra-plan-advice/src/trove.rs`. The trove's job: given a
parsed `Advice` and a `RelationIdentifier`, return all advice items
whose targets include that identifier. PG's implementation uses
hash buckets keyed by alias name with secondary linear filters;
Ra can use the same shape (`HashMap<String, Vec<AdviceMatch>>`
where `AdviceMatch` carries the tag, the matched-position
in the targets, and any sibling identifiers that must also match).

Operations to provide:

```rust
impl Trove {
    pub fn build(advice: &Advice) -> Self;
    pub fn lookup_scan(&self, id: &RelationIdentifier) -> Vec<&AdviceItem>;
    pub fn lookup_rel(&self, id: &RelationIdentifier) -> Vec<&AdviceItem>;
    pub fn lookup_join(&self, ids: &[RelationIdentifier]) -> Vec<&AdviceItem>;
}
```

Tests: every PG test file's `LOAD 'pg_plan_advice'; SET advice = '...'`
case translated to a unit test of the trove API.

#### Task 2.2 — Feedback flags

Port `PGPA_FB_*` flags from `pg_plan_advice.h:42-46`:

```rust
bitflags::bitflags! {
    pub struct FeedbackFlags: u8 {
        const MATCH_PARTIAL  = 0x01;
        const MATCH_FULL     = 0x02;
        const INAPPLICABLE   = 0x04;
        const CONFLICTING    = 0x08;
        const FAILED         = 0x10;
    }
}
```

Rendering to user strings (`"matched"` / `"partially matched"` /
`"not matched"` / `"inapplicable"` / `"conflicting"` / `"failed"`):
match PG's exact wording per `pgpa_output.c`.

### Phase 3 — Strategy mask in Ra (~1 week, the hard part)

Ra's `RelExpr` is logical and untyped; it has no strategy bits.
There are two approaches.

#### Task 3.1 — Approach A: parallel mask map (recommended)

Add `OptimizerContext` (new struct, lives for one optimization)
that carries `HashMap<RelExprPath, PgsMask>` — the analogue of
`RelOptInfo.pgs_mask`. The mask is consulted in two places:

- The rule advisor (`crates/ra-engine/src/rule_advisor.rs`):
  before applying a join-method rule, check the mask permits
  that method.
- The cost function (`crates/ra-engine/src/cost.rs`): if the
  extracted plan would violate the mask, add a large penalty
  (`Cost::DISABLE_PENALTY = 1e10` or similar — mirrors PG's
  `disable_cost`).

Pros: no `RelExpr` schema change; backward compatible.
Cons: requires the rule advisor and cost function to thread
through `OptimizerContext`.

#### Task 3.2 — Approach B: physical RelExpr variants

Add `RelExpr::IndexScan`, `RelExpr::HashJoin`, etc. and have the
optimizer choose between them. This is bigger but matches what PG
does — the `Path` types are pre-typed. **Out of scope for this
RFC; keep approach A for v1.**

#### Task 3.3 — Define `PgsMask` constants

```rust
// crates/ra-plan-advice/src/mask.rs
pub struct PgsMask(u64);
impl PgsMask {
    pub const SEQ_SCAN:           u64 = 1 << 0;
    pub const INDEX_SCAN:         u64 = 1 << 1;
    pub const INDEX_ONLY_SCAN:    u64 = 1 << 2;
    pub const BITMAP_SCAN:        u64 = 1 << 3;
    pub const TID_SCAN:           u64 = 1 << 4;
    pub const FOREIGN_JOIN:       u64 = 1 << 5;
    pub const MERGE_JOIN_PLAIN:   u64 = 1 << 6;
    pub const MERGE_JOIN_MAT:     u64 = 1 << 7;
    pub const NESTLOOP_PLAIN:     u64 = 1 << 8;
    pub const NESTLOOP_MAT:       u64 = 1 << 9;
    pub const NESTLOOP_MEMOIZE:   u64 = 1 << 10;
    pub const HASH_JOIN:          u64 = 1 << 11;
    pub const APPEND:             u64 = 1 << 12;
    pub const MERGE_APPEND:       u64 = 1 << 13;
    pub const GATHER:             u64 = 1 << 14;
    pub const GATHER_MERGE:       u64 = 1 << 15;
    pub const CONSIDER_INDEX_ONLY: u64 = 1 << 16;
    pub const CONSIDER_PARTITIONWISE: u64 = 1 << 17;
    pub const CONSIDER_NONPARTIAL: u64 = 1 << 18;

    pub const SCAN_ANY: u64 = Self::SEQ_SCAN | Self::INDEX_SCAN
        | Self::INDEX_ONLY_SCAN | Self::BITMAP_SCAN | Self::TID_SCAN;
    pub const ALL: u64 = (1u64 << 19) - 1;
}
```

Bit values **must match PG's** so generated advice is portable.
Layouts are pinned in `pathnodes.h:66-86`.

#### Task 3.4 — Apply advice to mask map

`pub fn apply_advice_to_masks(advice: &Advice, expr: &RelExpr) ->
HashMap<RelExprPath, PgsMask>`. For each item in the advice:

- Resolve targets to `RelExprPath`s.
- Compute the new mask: clear bits the advice forbids.

Translation table — PG to Ra mask edits:

| Advice | PG action (pgpa_planner.c) | Mask edit |
|---|---|---|
| `SEQ_SCAN(t)` | clear all but `PGS_SEQSCAN` | `mask &= !SCAN_ANY; mask \|= SEQ_SCAN` |
| `INDEX_SCAN(t i)` | restrict to one index | `mask = INDEX_SCAN; record index oid` |
| `INDEX_ONLY_SCAN(t i)` | similar | `mask = INDEX_ONLY_SCAN \| CONSIDER_INDEX_ONLY` |
| `BITMAP_HEAP_SCAN(t)` | restrict to bitmap | `mask = BITMAP_SCAN` |
| `TID_SCAN(t)` | restrict to TID | `mask = TID_SCAN` |
| `DO_NOT_SCAN(t)` | clear all scan bits | `mask &= !SCAN_ANY` |
| `HASH_JOIN(t)` | inner side hash | inner-rel mask = `HASH_JOIN`; implies join-order |
| `MERGE_JOIN_*` | inner side merge | inner-rel mask = MERGE_JOIN_* |
| `NESTED_LOOP_*` | inner side nl | inner-rel mask = NESTLOOP_* |
| `JOIN_ORDER(...)` | constrain joinrel build | record allowed joinrel structure |
| `PARTITIONWISE(...)` | toggle PARTITIONWISE bit | `mask &= !CONSIDER_PARTITIONWISE` outside the named set |
| `GATHER(t)` | place Gather above | mask = `GATHER` for that path |
| `NO_GATHER(t)` | forbid Gather | `mask &= !(GATHER \| GATHER_MERGE)` |
| `SEMIJOIN_UNIQUE(t)` | force unique-then-inner-join | tag the e-class for unique-join rewrite |
| `SEMIJOIN_NON_UNIQUE(t)` | forbid unique-then-inner-join | inverse |
| `FOREIGN_JOIN((t1 t2))` | push down to FDW | mask the joinrel `mask = FOREIGN_JOIN` |

### Phase 4 — Honor advice in the optimizer (~1 week)

#### Task 4.1 — Rule advisor integration

Extend `RuleSelectionBehavior` (`crates/ra-engine/src/resource_budget.rs`)
with `pub advice: Option<Arc<Trove>>`. The advisor's stage 2
(query-shape filter) consults the trove: if any item targets a
relation present in the query and the rule about to be considered
would violate it, demote the rule.

This pairs cleanly with the existing `JoinGraphShape` advisory
filter from the GEQO-lessons work: shape + advice both produce
"rule should not fire here" decisions, and the same plumbing
applies.

#### Task 4.2 — Cost-extraction penalty

In `crates/ra-engine/src/extract/api.rs`, before returning the
chosen plan, walk it and compare against the trove. For every
violation, add `Cost::DISABLE_PENALTY` (define as `1e10` to
mirror PG's `disable_cost`). The plan still extracts — it just
becomes very expensive — which is exactly the "Disabled: true"
fallback PG produces when advice forces an impossible plan.

#### Task 4.3 — Speculative-router awareness

When advice is supplied, the speculative router's job changes:
the goal is no longer "produce the lowest-cost plan in budget";
it's "produce a plan that satisfies the advice." Add a new
`OptRoute::AdviceConstrained` variant that:

- Skips the BitNet feature predict (advice supersedes ML
  decisions).
- Goes straight to e-graph saturation with `EGraphHigh` budget
  multiplied by 2 (advice may force a less-optimal-by-cost plan
  that takes more iterations to reach).
- Sets the advice trove on `OptimizerContext`.
- Fingerprints the resulting plan for `PlanProvenance` so
  follow-up debugging can see which advice was honored.

### Phase 5 — Generate advice from a Ra plan (~3 days)

#### Task 5.1 — Plan walker

Port `pgpa_walker.c` (1,174 lines, the bulkiest single file) to
`crates/ra-plan-advice/src/walker.rs`. The walker visits the
extracted `RelExpr` and emits one `AdviceItem` per decision Ra
made:

- For each `Scan`: emit `SEQ_SCAN(t)` / `INDEX_SCAN(t i)` etc.
  based on the chosen physical strategy.
- For each `Join`: emit `JOIN_ORDER(outer inner)` plus the
  appropriate `*_JOIN(inner)` tag.
- For each parallel section: emit `GATHER` / `NO_GATHER`.
- For partitionwise joins: `PARTITIONWISE`.

Output a syntactically-valid pgpa string using the AST-to-string
renderer from Task 5.2.

#### Task 5.2 — AST-to-string renderer

Port `pgpa_output.c` (606 lines) to
`crates/ra-plan-advice/src/render.rs`. Pure function from
`Advice` to `String`. Round-trip property: `parse_advice(render(parse_advice(s)?))?
== parse_advice(s)?`. Tested via proptest.

### Phase 6 — Surface (~3 days)

#### Task 6.1 — `OptimizerConfig` and CLI

```rust
pub struct OptimizerConfig {
    // ... existing fields ...
    /// Plan-advice string honored during optimization. None or
    /// empty means no advice is applied. Format matches
    /// PostgreSQL's `pg_plan_advice.advice` GUC.
    pub plan_advice: Option<String>,
    /// Generate plan advice for produced plans (the equivalent of
    /// PG's `pg_plan_advice.always_store_advice_details`).
    pub generate_plan_advice: bool,
    /// Warn when supplied advice does not apply cleanly (PG's
    /// `pg_plan_advice.feedback_warnings`).
    pub plan_advice_feedback_warnings: bool,
}
```

CLI: `ra-cli explain --plan-advice "JOIN_ORDER(a b) HASH_JOIN(b)"`
and `ra-cli explain --generate-plan-advice`. Render generated and
supplied advice in the existing `Provenance:` block (added in the
GEQO-lessons work) under `Plan Advice:`.

#### Task 6.2 — PG extension surface

Mirror the PG GUC surface in `ra-pg-extension`:

- `ra.plan_advice` — string GUC; setter validates by calling
  `ra-plan-advice`'s parser; on parse error logs detailed message.
- `ra.always_explain_supplied_advice` — bool GUC.
- `ra.always_store_advice_details` — bool GUC.
- `ra.plan_advice_feedback_warnings` — bool GUC.
- `ra.plan_advice_trace_mask` — bool GUC.

EXPLAIN integration: register `EXPLAIN (PLAN_ADVICE)` via the
same `RegisterExtensionExplainOption` infrastructure. The TODO
from Lesson (ii) of the GEQO work already noted this integration
point in `crates/ra-pg-extension/src/planner_hook.rs`. Render
"Generated Plan Advice:" and "Supplied Plan Advice:" blocks
with feedback flags identically to PG.

#### Task 6.3 — Direction 1 (emit-only) — ra-bench advice export

In `crates/ra-bench/src/compare.rs` (the existing PG-vs-Ra
comparison subcommand), add a new mode `--export-advice` that:

1. Runs Ra optimization on each query.
2. Generates plan advice via Task 5.1.
3. Writes the advice + query to `<workload>.pgpa`.
4. (Optional) replays the advice on the running PG instance via
   `SET pg_plan_advice.advice = '...'` and confirms PG produces
   the same plan structure.

This is the diagnostic oracle: any difference between Ra's chosen
plan and the plan PG produces under Ra's advice is a finding (Ra
considered something PG doesn't, or Ra's advice generation has a
bug, or the cost models genuinely disagree).

### Phase 7 — Test as oracle (~3 days, mirrors test_plan_advice)

#### Task 7.1 — `test_plan_advice` Rust port

Port the loop in
`~/src/postgres/src/test/modules/test_plan_advice/test_plan_advice.c`
to a Ra-side test mode: every query in the existing test corpus
gets optimized twice, the second time through generated advice
from the first. Both plans must be structurally identical.

This is the most important deliverable. It's the test that
tells you whether Direction 1 + Direction 2 actually round-trip
within Ra. PG's `test_plan_advice` is wired into the buildfarm
and catches regressions even when `make installcheck` would pass.

Implementation: new file
`crates/ra-engine/tests/plan_advice_roundtrip.rs`. Iterate every
TPC-H query (already in `crates/ra-bench/src/compare.rs`):

```rust
#[test]
fn every_tpch_query_round_trips_through_advice() {
    for (name, sql) in tpch_queries() {
        let expr = parse(sql);
        let opt1 = Optimizer::new().optimize_bounded(&expr).unwrap();
        let advice = generate_advice(&opt1.plan);
        let mut cfg = OptimizerConfig::default();
        cfg.plan_advice = Some(advice.clone());
        let opt2 = Optimizer::with_config(cfg)
            .optimize_bounded(&expr).unwrap();
        assert_plans_equivalent(&opt1.plan, &opt2.plan,
            "round-trip failed for {name}: advice = {advice}");
    }
}
```

#### Task 7.2 — PG cross-oracle test

Take PG's own
`~/src/postgres/contrib/pg_plan_advice/sql/*.sql` test files,
extract the `SET pg_plan_advice.advice = '...'` lines, feed each
advice string into Ra's parser, and verify Ra parses without
error. Run a subset against a query Ra can plan and verify the
plan matches what PG produced for that advice.

This is feature-coverage proof: PG's own test suite should be
fully consumable by Ra.

#### Task 7.3 — Proptest round-trip

Property: for every legal `Advice` AST, `parse(render(advice)) ==
advice`. This catches subtle whitespace / quoting / identifier-
escaping bugs in the renderer that Tasks 7.1 and 7.2 won't
necessarily expose.

### Phase 8 — Documentation (~2 days)

#### Task 8.1 — `docs/integrations/plan-advice.md`

User-facing documentation that mirrors `pgplanadvice.sgml` in
structure: Getting Started, How It Works, Advice Targets, Advice
Tags (one subsection per tag class), Feedback, Configuration
Parameters, Limitations. Cross-link to PG's documentation so
users can see this is a faithful port.

#### Task 8.2 — RFC
`rfcs/text/0087-plan-advice-integration.md` — the design doc
following the project's RFC template, motivating the work and
documenting design decisions (especially Task 0.2 on identifier
syntax compatibility).

### Phase 9 — Future-work / out-of-scope notes

Document explicitly what is **not** in v1, mirroring PG's
"Limitations" section:

- Aggregate strategy advice (sort vs hash, eager vs lazy,
  partitionwise vs not). PG's pgpa_planner.c says "XXX: This needs
  some study to determine how large a problem it is."
- Set-operation advice (UNION, INTERSECT, EXCEPT). PG punts on
  this too.
- Advice over Ra-specific concepts that have no PG analogue:
  speculative-router route choice, BitNet cost-model snapshot id,
  rule-advisor demoted-set. These are good `RULE_GROUP(name)`-style
  extensions for v2.
- Advice as input to the GA-fallback (RFC 0035, currently
  Proposed). The PG README explicitly says "XXX Need to investigate
  whether and how well supplying advice works with GEQO" — a
  warning to take seriously.

## Total estimate

| Phase | Days | Cumulative |
|---|---|---|
| 0. Audit and design | 2 | 2 |
| 1. Parser, AST, identifiers | 5 | 7 |
| 2. Trove | 3 | 10 |
| 3. Strategy mask | 5 | 15 |
| 4. Honor advice in optimizer | 5 | 20 |
| 5. Generate advice | 3 | 23 |
| 6. Surface (CLI + PG ext) | 3 | 26 |
| 7. Test oracle | 3 | 29 |
| 8. Documentation | 2 | 31 |

**~6 weeks** of focused work for a single engineer with the
existing Ra codebase context. Phases 1-3 are blockers for everything
else; Phase 7 (the round-trip test) is the deliverable that proves
correctness.

## What this gets us

- **Plan-stability** in Ra: a workload that performed well last
  week can be pinned via stored advice, so unrelated changes
  (statistics drift, model updates, new rules, hardware changes)
  can't silently degrade it.
- **Round-trip equivalence with PostgreSQL**: any query Ra plans
  becomes inspectable and reproducible by PG, and any PG plan can
  be loaded into Ra. This is a strong differentiator for
  Postgres-compatibility positioning.
- **A regression oracle**: `test_plan_advice`-style replanning
  catches regressions in rule transformations, cost-model drift,
  and rule-advisor decisions that random query-corpus testing
  would miss.
- **A debugging surface**: `EXPLAIN (PLAN_ADVICE)` plus the
  existing `--provenance` flag together answer "why this plan?"
  with an actionable, replayable artifact.
