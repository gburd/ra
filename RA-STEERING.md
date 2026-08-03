# Ra — Steering Document

**Status:** Directive. Supersedes prior roadmaps, RFC priorities, and release plans.
**Audience:** Everyone and everything committing to `gregburd/ra`, human or agent.
**Version:** 1.0

---

## 0. Why this document exists

Ra contains real, novel ideas. It also contains a large amount of material that
exists because it was easy to produce rather than because it was needed, and a
set of public claims that the repository itself contradicts three paragraphs
later.

The gap between what Ra says and what Ra does is now the project's largest
liability. It is not a marketing problem. It is a signal that the project has
been optimizing for the appearance of progress, and it has made the real
progress illegible — including to us.

This document redefines what "progress" means for Ra. Read Section 1 before
writing any code. Read Section 3 before making any claim in a commit message,
README, or benchmark.

---

## 1. The goal, stated once, precisely

Ra has exactly two deliverables. Everything in the repository either serves one
of them or is removed.

### Deliverable A — `ra`, the exploration tool

A single command-line binary named **`ra`** (not `ra-cli`) that makes the
planning and optimization process legible: parse, route, rewrite, cost, extract,
lower. It is a debugger and a discovery instrument for people working on query
optimization, and it is *our* primary debugging surface for Deliverable B.

### Deliverable B — the PostgreSQL front-half replacement

A PostgreSQL extension that replaces parsing, planning, and optimization, and
**never falls back to PostgreSQL's native planner.**

Subject to three invariants, in strict priority order:

1. **Correctness.** Every plan Ra emits produces results logically identical to
   the plan PostgreSQL's planner would have produced for the same query against
   the same database state. Always. No exceptions, no query classes carved out.
2. **Completeness.** Ra handles 100% of the SQL surface PostgreSQL accepts. A
   fallback is a correctness failure that has been renamed.
3. **Efficiency.** Given 1 and 2, Ra emits plans as fast as possible using the
   minimum resources required — bounded, predictable, and measurably better than
   the native planner on total (plan + execute) time.

**These are ordered.** When they conflict, the lower number wins. A faster
planner that is wrong is worth less than no planner at all, because a wrong
planner corrupts data silently and the operator has no way to notice.

### The sequencing rule that everything else follows from

We currently claim zero fallbacks while correctness is unproven. **That is the
worst possible state** — it is the state where divergence is invisible. The
resolution is not to argue about it, it is to sequence it:

> The fallback path is *scaffolding*. During development it stays, instrumented
> and counted, and every invocation of it is a bug report filed automatically.
> We earn the right to delete it by driving that counter to zero against the
> full corpus. Then we delete it. Then we claim it.

Deleting the fallback before correctness is proven does not make Ra complete.
It makes Ra's incompleteness undetectable.

---

## 2. What "correct" means

"Same results" is not specific enough to test against. This is the definition
Ra is held to. Every item below is a divergence class that has produced real
bugs in real optimizers, and several are almost certainly live in Ra today.

A Ra plan is correct with respect to a PostgreSQL plan when, for the same query
and database state, all of the following hold:

**Result equivalence**
- The output is the same *multiset* of rows — same values, same cardinality,
  same duplicate counts.
- Row order matches exactly when the query specifies an ordering (`ORDER BY`,
  and ordering implied through `LIMIT`, `DISTINCT ON`, window frames).
  Where the query specifies no order, the multiset must match; order may differ.
- Column types, typmods, collations, and output names match. `numeric` scale and
  `varchar` length are part of the result, not decoration.
- NULL semantics under three-valued logic match, including outer-join-generated
  NULLs and `IS DISTINCT FROM` behavior.

**Error and side-effect equivalence**
- If PostgreSQL raises an error, Ra raises the same error class. This is the
  trap in predicate pushdown: pushing a filter below a join or a `CASE` can
  cause a division-by-zero or cast error to fire that would not have fired, or
  suppress one that should have. Any rewrite that changes which rows an
  expression is evaluated over must prove it cannot change error behavior.
- Volatile function call counts match. `random()`, `nextval()`, `clock_timestamp()`
  duplicated by a rewrite, or eliminated by CSE, changes observable behavior.
  Volatility classes (`immutable`/`stable`/`volatile`) gate every rewrite that
  duplicates, removes, or reorders expression evaluation.
- Set-returning functions in target lists produce the same expansion.
- DML affects the same rows, fires the same triggers the same number of times,
  in the same order where PostgreSQL defines one, and returns the same
  `RETURNING` set.

**Semantic equivalence**
- Locking behavior matches: `FOR UPDATE`/`FOR SHARE`, `SKIP LOCKED`, `NOWAIT`,
  and which relations get locked at what strength.
- Row-level security policies are applied, and security-barrier views are not
  penetrated by predicate pushdown. Pushing a user predicate below a security
  barrier is an information leak, not an optimization.
- `search_path`, `session_replication_role`, and other GUC-dependent resolution
  produce the same bindings.
- Isolation and snapshot semantics are unchanged.

**Surface completeness** — every one of these must plan natively, or Ra is not
complete: views and rules, inheritance and partition expansion (including
plan-time and run-time pruning), recursive CTEs, `GROUPING SETS`/`CUBE`/`ROLLUP`,
window functions with all frame types, `LATERAL`, `MERGE`, `INSERT ... ON CONFLICT`,
`RETURNING`, generated columns, domain constraints, foreign tables and FDW
pushdown, parallel plans, custom scan providers, generic vs. custom prepared
plans, `TABLESAMPLE`, arrays, composites, ranges, JSON/JSONB paths, and every
extension type that reaches the planner.

That list is long. It is the actual size of the job. Publishing a completion
percentage against it is more useful than any performance number we can produce.

---

## 3. Truth in claims — do this first

This is a one-week task and it unblocks trust in everything after it. Nothing
else on the roadmap starts until it lands.

### 3.1 Claims to correct

| Current claim | Problem | Replace with |
|---|---|---|
| "Drop-in replacement for PostgreSQL query planner/optimizer" (repo tagline) | Not true today; sets the reader up to disbelieve everything else | "Experimental parser, planner, and optimizer for PostgreSQL. Research prototype — see correctness status." |
| "89× geo mean speedup / 21 of 21 queries won" | Measured with statistics disabled on the Ra side, and the README concedes plan quality was not measured. Ra's `SKIP` route returns the input unchanged; a planner that always skips wins every query at ~0 µs. The metric is close to vacuous. | Remove entirely until replaced by end-to-end (plan + execute) numbers per §6.2. |
| "IN subquery 785× faster (17s → 11ms)" | This is Ra fixing a 17-second pathology in Ra. It is not a comparison to PostgreSQL. Two commits also give different endpoints (22ms, 11ms). | "Fixed pathological IN-subquery planning regression (17s → 11ms)." File it as a bug fix, because it is one. |
| "~170 rewrite rules" (diagram) vs "293 rules active" (heading) vs ~157 (table minimums) vs "1,387 rule sources, ~94 compile" | Four numbers for one fact in one document | One number, emitted by the build, injected into the README by CI. Report active rules and spec-only rules separately. |
| "97.5% plan cache hit rate on OLTP" | Derived from a 5-template × 40-variation synthetic test. 39/40 is a property of the test's construction, not a measurement. | Remove until measured on a captured real workload. |
| "120 queries, 97.5% pass" | Presented as a milestone. Three wrong answers in a planner is not a 97.5%; and 120 queries is not exhaustive — PostgreSQL's own regression suite is tens of thousands of statements. | Report as "3 known correctness defects, tracked as #N, #N, #N" and put it at the top of the README, not in a commit title. |
| "Zero query-level fallbacks for DML/DQL" | Scoped to exclude DDL and utility statements; implies the surface is covered when it is not | Report the fallback counter from §5.1 against the full corpus. |
| MAPE tracker described in detail, no MAPE value published | The single most important missing number in the project. The central thesis is "a learned cost model finds better plans" and we have never published whether it predicts anything. | Publish MAPE on a held-out workload, with PostgreSQL's own cost estimates as the baseline. |

### 3.2 Standing rules on claims

- **No number without a committed harness that reproduces it.** A metric in the
  README, a commit message, or a doc must name the command that regenerates it.
- **No comparative claim without a symmetric configuration.** If PostgreSQL
  reads statistics, Ra reads statistics.
- **Benchmarks must be losable.** A benchmark we win 21/21 is measuring the wrong
  thing. Report per-query regressions against PostgreSQL and keep the losses
  visible.
- **Banned from commit messages, docs, and code comments** (enforce by CI grep):
  "comprehensive", "excellence", "production-ready", "world-class", "fully",
  "complete" (as a self-assessment), "achieve", "revolutionize", and any use of
  "✓" or "100%" not backed by a named harness. Describe what changed and what
  was measured. Nothing else.
- **Fixing our own regression is not a performance win.** Ever.
- **Version bumps require green CI.** v0.6.0 was tagged with all five CI jobs
  cancelled. That makes tags meaningless. Green CI becomes a hard release gate.

---

## 4. Shrink the blast radius

Ra is 221 MiB, 27 branches, ~94 RFCs, 1,349 commits, zero issues, one star. The
repository cannot be held in a person's head, which means it cannot be reviewed,
which means defects are invisible.

**Target:** clone, build, and test in under ten minutes, under 50 MB.

### 4.1 Move out to a separate `ra-lab` repository

None of this is bad work. None of it serves Deliverable A or B today, and all of
it competes for the attention that correctness needs.

- `rules/hardware/` — GPU, FPGA, SIMD, NUMA rules
- `rules/distributed/` — exchange, broadcast, partition pruning for distributed execution
- `rules/multi-model/` — graph, document, time-series
- `ra-quel-parser` — 1976 INGRES dialect stub
- `ra-dialect` — 20+ dialect translation
- `ra-adapters` — DuckDB, MySQL, Stoolap connectors
- `web/`, `netlify.toml`, `fly.toml` — the Preact plan visualizer and its deployment
- `ra-adaptive`, `ra-ml` — superseded experiments
- `vendor/bitnet-mlx-rs` — unless it is on the query path, which it is not

### 4.2 Delete outright

- `ChangeLog` (superseded by `CHANGELOG.md`)
- `HOWTO-RFC0059.md`, `QUICK_START.md` — fold anything live into `docs/`
- 27 branches → merge or delete; keep `main` plus active work
- The 1,293 `.rra` files that do not compile — move to `ra-lab/rules-spec/` with
  an honest note that they are unimplemented designs
- `results/`, `timelines/` — regenerable artifacts do not belong in git
- Any `.rra` rule with no test case (see §7.2)

### 4.3 RFCs

Ninety-four design documents and zero issues means design is running far ahead of
validation. Triage into three buckets: **implemented** (link to the code and its
tests), **active** (convert to a tracked issue with an owner), **parked** (move to
`ra-lab`). No new RFC until the correctness gate in §6.1 is passed.

### 4.4 Open the tracker

Zero issues on a project this size means there is no public surface for anyone to
contribute against, and no external record of what is broken. Every known defect
from §8 gets an issue. Milestones: `correctness-parity`, `zero-fallback`,
`planning-performance`.

---

## 5. The correctness machine

This is the core of the work. Everything in this section is infrastructure that
makes correctness *mechanically checkable* rather than argued about.

### 5.1 Instrument the fallback

Before removing the fallback, make it loud.

- Every fallback increments a counter tagged with a reason code and the query
  fingerprint, exposed through a view (`ra_planner.fallbacks`).
- Every fallback logs at `WARNING` with the reason and the query.
- A `ra_planner.fallback = error` GUC makes any fallback a hard error. This is
  the CI setting.
- The fallback counter, broken down by reason, is published in the README and
  updated by CI. **It is the project's headline number until it reaches zero.**

A counted fallback is a to-do list. An uncounted one is a lie with a long fuse.

### 5.2 Use PostgreSQL as the oracle — this is the highest-leverage tool we can build

PostgreSQL has already solved parse analysis correctly, and it will tell us the
answer for free. We should exploit that ruthlessly.

**`ra parse --compare-pg`** — take a SQL statement, run PostgreSQL's
`parse_analyze` to get a `Query` node, lower that `Query` into `RelExpr`, and
diff it against what Lime + `sql_to_relexpr` produced. Any structural divergence
is a parser or analysis bug, located precisely, with no human judgment required.

Point this at PostgreSQL's own regression suite corpus and it will enumerate our
parser defects exhaustively in an afternoon. This is how the name resolution,
type coercion, operator resolution, view expansion, and collation bugs get found
— and those are where wrong answers come from.

**Note the trade honestly.** Keeping our own parser means reimplementing the
largest and most subtle part of PostgreSQL. That is the stated goal, so we accept
it — but we accept it with our eyes open and with PG as a continuous oracle. The
`Query`-lowering path built for this tool is also the escape hatch if the parser
proves to be the thing standing between us and zero fallbacks.

### 5.3 The corpus, in tiers

Correctness is reported per tier. No tier is skipped, and a tier is never
declared passing at less than 100%.

- **Tier 0 — Smoke.** The existing 120 queries. Currently 3 failures. Must be 100%
  before anything else ships.
- **Tier 1 — PostgreSQL regression suite.** `src/test/regress`. The definitive
  surface test. Every statement, byte-identical output. This is the real bar.
- **Tier 2 — sqllogictest.** Millions of statements, cross-engine verified.
- **Tier 3 — Workload benchmarks.** TPC-H, TPC-DS, JOB — run for *results*, not
  timing, at this stage.
- **Tier 4 — Differential fuzzing.** `ra-grammar-fuzzer` and `ra-difftest`
  generating random valid SQL against random schemas, comparing Ra vs. native
  results continuously. This is what catches the divergences nobody thought to
  test.

### 5.4 Rewrite rules must carry their proof obligations

Every `.rra` rule declares, in machine-checkable form, what it requires:

- Null-rejection proof for any predicate pushed through an outer join
- Volatility class constraints for any rewrite that duplicates, removes, or
  reorders expression evaluation
- Error-behavior preservation for any rewrite that changes the row set an
  expression is evaluated over
- Security-barrier and RLS interaction for any pushdown
- Uniqueness/functional-dependency prerequisites for join elimination and
  distinct removal

A rule that does not declare its obligations does not load. The `.rra` format
already has slots for preconditions — this makes them mandatory and enforced.

### 5.5 Formal work, pointed at something

The TLA+ specs in `tla/` are valuable but currently disconnected from the code.
Point them at the highest-risk transformations: join reordering with outer joins,
subquery decorrelation, aggregate pushdown. A spec that models a rule we actually
ship is worth more than ten that model rules we don't.

---

## 6. Gates

Each gate is binary and mechanically checkable. No gate is passed by assertion.

### Gate 1 — Correctness parity *(blocks everything else)*

- Tier 0: 100%
- Tier 1: 100% of PostgreSQL's regression suite, byte-identical
- Tier 2: 100% of the sqllogictest corpus
- Tier 4: 72 hours of continuous differential fuzzing, zero divergences
- Fallback permitted and counted during this phase; the counter is published

### Gate 2 — Zero fallback

- Fallback counter is zero across Tiers 0–4
- `ra_planner.fallback = error` is the default, and CI runs with it
- The fallback code path is deleted, not disabled
- Every item in §2 "Surface completeness" has a passing test
- *Only after this gate does the "drop-in replacement" tagline return*

### Gate 3 — Planning performance

Correctness is locked; now make it fast. Measured with statistics loaded on both
sides, symmetric configuration:

- **Total time** (plan + execute), not planning time in isolation. Report
  per-query, including every regression against native PostgreSQL.
- TPC-H at SF=1 and SF=10. **SF=0.01 is retired** — at ~10 MB everything is
  cache-resident and every plan is fast, so plan quality is unmeasurable by
  construction. It is the scale factor at which a benchmark cannot fail.
- Join Order Benchmark, which exists specifically to stress cardinality
  estimation. This is the benchmark that tells us whether the cost model does
  anything.
- Planning latency p50/p99/p999, not means. Tail latency is what operators feel.
- **Resource budget per plan, enforced:** peak allocation, allocation count,
  e-graph node ceiling, hard wall-clock budget per route tier. A planner that is
  fast on average and occasionally allocates a gigabyte is not usable.
- No unbounded growth: e-graph saturation must terminate under budget for every
  query in the corpus, with the budget enforced rather than hoped for.

### Gate 4 — The cost model earns its place

- Publish MAPE against actual execution on a held-out workload
- Baseline: PostgreSQL's own cost estimates on the same queries
- **If the model does not beat that baseline, cut it.** Replace it with a learned
  scalar correction on top of PG-style estimation and reclaim the complexity.
  The router is a defensible use of a tiny network; twelve-dimensional cost
  prediction from sixteen features may not be.
- Report router decision quality separately: how often does each route produce
  the plan that the highest tier would have produced?

---

## 7. The `ra` command-line tool

Rename `ra-cli` → **`ra`**. The binary, the crate, the docs, the shell
completions.

### 7.1 Design principles

1. **One code path.** `ra` and the PostgreSQL extension execute the *same*
   planner code. Any behavioral divergence between them is a P0 bug. `ra` is not
   a simulator or a demo — it is the extension's planner with a different front
   door.
2. **Everything is inspectable.** Any intermediate state the optimizer computes
   can be dumped. If a stage cannot be printed, it cannot be debugged, and it
   will accumulate defects.
3. **Machine-readable by default.** Every command supports `--format json`.
   Human-readable output is a rendering of the JSON, not a separate path.
4. **Connect to a real database.** `ra --conn <libpq-url>` pulls real catalogs
   and statistics. Without a connection, commands that need statistics fail with
   a clear error rather than inventing them. (The `simulate_native_*` helpers
   were removed for exactly this reason — do not let them come back in a new
   shape.)

### 7.2 Command surface

```
ra parse <sql>                 RelExpr tree from the Lime parser
  --compare-pg                 diff against PostgreSQL's lowered Query node (§5.2)
  --tokens                     tokenizer output
  --explain-error              structured "expected one of ..." diagnostics

ra route <sql>                 the 16 OptimizationFeatures, their values, the
                               model's prediction, the chosen route, and the
                               budget assigned — with the counterfactual: what
                               the other routes would have produced

ra optimize <sql>              run the optimizer
  --trace                      per-iteration: rules fired, e-classes added,
                               cost delta, continuation-gate decision
  --step                       interactive single-stepping through iterations
  --diff                       before/after plan diff
  --route <tier>               force a route, bypassing the router
  --rules <filter>             enable only matching rules — bisect a bad plan

ra egraph <sql>                dump the e-graph: e-classes, e-nodes, growth per
                               iteration, saturation state
  --dot                        Graphviz output
  --extract-top <n>            the n lowest-cost equivalent plans with costs

ra rules                       list active rules, category, source .rra file
  show <rule>                  formal algebra, preconditions, proof obligations,
                               cost model, tests
  why <sql>                    which rules fired, in what order, on what
                               e-classes, and what each produced
  test <rule>                  run that rule's test cases

ra cost <sql>                  cost vector per candidate plan, all 12 dimensions,
                               model confidence, and the extraction decision

ra explain <sql>               final plan
  --compare-pg                 side-by-side with PostgreSQL's plan for the same
                               query, with a structural diff

ra verify                      run the correctness corpus (§5.3)
  --tier <0-4>
  --report                     the numbers that go in the README

ra bench                       end-to-end plan+execute against a live PG
  --workload tpch|tpcds|job
  --scale <sf>

ra replay <trace>              re-run a captured OptimizationTrace — this is how
                               a production bad plan gets reproduced locally

ra model                       inspect the BitNet model: weights, normalization,
                               MAPE history, training step count
```

### 7.3 What makes this tool worth building

`ra rules why` and `ra optimize --step` are the difference between "the plan is
wrong" and "rule `push-filter-through-outer-join` fired on e-class 47 without
proving null-rejection." Every hour spent on this tool pays back in Gate 1.

Build the tool *before* chasing the correctness backlog, not after.

---

## 8. Known defects to open as issues now

- **3 failing queries** in the 120-query qualification suite. These are wrong
  answers. Highest priority in the repository. Root-cause each and add a
  regression test — my expectation is that at least one traces to parse analysis
  or name resolution rather than to a rewrite rule.
- **Malformed `.rra` handling.** Two rules with metavariables in operator
  position previously panicked the entire generated batch through `catch_unwind`.
  The build script now rejects that pattern, but the underlying issue is that
  `.rra` files are not validated at authoring time. Add a `ra rules lint` that
  runs in CI and fails on any malformed rule.
- **~1,293 non-compiling `.rra` files.** Each is a claim that Ra implements
  something it does not. Move to `ra-lab` (§4.2).
- **CI red on `main`.** All five jobs cancelled on the v0.6.0 commit. Fix or
  delete every job; a permanently red pipeline trains everyone to ignore it.
- **Two different values published for the same IN-subquery fix** (22ms, 11ms).
  Determine which is real, publish the harness.
- **Copyright ambiguity.** The README states the author does not know whether
  copyright can be asserted, then triple-licenses anyway. No organization will
  adopt this. Take a clear position with reasoning, even if the position is
  uncertain — an explicit stance is adoptable, a shrug is not.
- **Codeberg Terms of Use.** LLM-generated content is now restricted on Codeberg.
  Ra is explicitly disclosed as heavily AI-generated. Verify compliance before it
  becomes a hosting problem rather than after.

---

## 9. Innovations to preserve and fully exploit

These are why Ra is worth finishing. Under no circumstances are they removed
during cleanup — but each is currently underexploited, and this section says how.

**1. Equality saturation for plan search.** Representing all equivalent plans
simultaneously and extracting the cheapest sidesteps the phase-ordering problem
that every sequential rewriter has. This is Ra's central idea and it is genuinely
strong. *Underexploited:* we do not currently report what the e-graph found that
a sequential optimizer would have missed. `ra egraph --extract-top` plus a
per-query "plans considered" count makes the value visible. That comparison — not
planning speed — is the paper.

**2. The speculative router.** An O(1) prediction of whether search is worth
running is the thing that makes e-graph optimization viable inside an OLTP
planner, where a 50ms plan search on a 1ms query is a catastrophe. This is a real
contribution and it is more interesting than the cost model. *Underexploited:*
publish route-decision quality (§6, Gate 3) and the latency distribution per
route. Show that the router buys back the search cost.

**3. The continuation gate.** Early stopping on marginal cost improvement is a
clean answer to "when do you stop saturating." *Underexploited:* report how much
work it saves and how often it stops too early — measured by forcing full
saturation and comparing extracted plans.

**4. Sub-microsecond quantized inference on the query path.** ~80ns for a full
forward pass means the model can be consulted per-query without a budget
conversation, with no heavyweight ML runtime in the backend. That property is
what makes the whole design possible. *Keep the property; stop leading with the
byte count.* 452 bytes is a headline that draws attention away from the question
that matters, which is whether it predicts anything (Gate 4).

**5. Online learning from execution feedback.** `executor_end_hook` → traces →
batched QAT → snapshot → router. Closing the predicted/actual loop with real
execution data is the right architecture for cost estimation, and it is the thing
PostgreSQL structurally cannot do. *Underexploited:* nothing is published about
whether it converges. MAPE over training steps is the chart the project needs.

**6. The `.rra` declarative rule format.** Rules as literate data with formal
algebra, preconditions, cost model, and tests — separately auditable and testable
from the engine. This is excellent design and it is Ra's best defense against the
correctness problem. *Underexploited:* the proof obligations in §5.4 belong here,
and the format should be the enforcement mechanism, not documentation.

**7. Direct SQL → RelExpr parsing with no intermediate AST.** Skipping a
representation is a real efficiency win. *This carries the project's largest
correctness risk* (§5.2). Preserve it, and pair it permanently with the PG
oracle.

**8. Template-based plan caching.** The right structure for OLTP. Needs a real
workload measurement rather than a synthetic one.

**9. TLA+ specification of rule semantics.** Rare and valuable. Point it at the
rules most likely to be wrong (§5.5).

**10. The ordering pass.** Sort elimination and IncrementalSort conversion is
solid, unglamorous optimizer work that pays off. Keep it, test it against PG's
own ordering decisions.

---

## 10. Working agreements

Much of Ra's output is agent-generated. The failure mode is characteristic and
recognizable: enormous volume, every task marked complete, documentation written
in the confident register of shipped software, and metrics chosen because they
are achievable rather than because they matter. None of that is a character flaw
— it is a specification problem. Agents optimize what they are pointed at, and
Ra has been pointed at "produce impressive-sounding artifacts." It has been
extremely good at that.

So we change the target.

1. **Two green lights exist in this repository:** differential correctness
   against PostgreSQL, and end-to-end latency on a real workload. Nothing merges
   that does not move one of them or directly support something that does.
2. **No task is complete without a test that failed before the change and passes
   after.** "Implemented X" with no failing-then-passing test is not a completed
   task; it is an untested claim.
3. **Commit messages state measured facts.** What changed, what was measured, by
   which harness. No superlatives (§3.2).
4. **A rewrite rule without proof obligations does not load** (§5.4).
5. **Do not add a feature during Gates 1–2.** Not a rule, not a dialect, not an
   RFC. The surface is already larger than the correctness we can demonstrate.
6. **When uncertain whether something is correct, it is not correct.** Write the
   differential test that settles it.
7. **Report failures in status updates.** A status update with no failures in it
   is not a status update, it is marketing. What is broken, what regressed, what
   we do not know — those are the useful contents.

---

## 11. Order of work

1. **§3 Truth reset** — correct the claims, remove the benchmark, fix the numbers. *One week.*
2. **§4 Shrink** — move to `ra-lab`, delete cruft, open the tracker, fix CI. *One week.*
3. **§7 Build `ra`** — the debugger, especially `parse --compare-pg`, `rules why`, `optimize --trace`. *Two to three weeks.*
4. **§5 Correctness machine** — oracle harness, corpus tiers, fallback instrumentation, proof obligations. *Concurrent with 3.*
5. **Gate 1** — grind correctness to 100% across all tiers. *This is the bulk of the project. Expect it to be long, and do not let anything jump the queue.*
6. **Gate 2** — delete the fallback. Restore the tagline. Announce.
7. **Gate 3 and 4** — performance and cost-model validation, with correctness locked behind regression tests.

---

## 12. What success looks like

A README whose first section is a correctness table showing 100% across five
tiers of corpus, with the harness command printed next to each number. A
`ra` binary that lets someone watch an e-graph saturate and understand why a plan
was chosen. An extension with no fallback path in the source tree. A benchmark
that reports total query time including the queries where we lose.

And underneath it, the thing that was always the actual contribution and has been
hard to see: **equality saturation as a plan search strategy for PostgreSQL, made
affordable by a learned router that decides when the search is worth running.**

That sentence is novel and defensible and interesting to the people whose
attention is worth having. It does not need an 89×.
