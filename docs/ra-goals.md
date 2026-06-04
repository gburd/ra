# Ra project goal and separation of concerns

## North-star goal

Ra is a **complete drop-in replacement for the PostgreSQL planner**. The bar:

- **Always correct** — identical results to PostgreSQL for every query PG
  accepts. Never wrong, never crash; if Ra cannot guarantee correctness it
  must fall back to PG (never fail a query PG could plan).
- **Always as fast or faster** — Ra's plans execute in time **≤** PG's, in
  **equal or less planning time**, without **excessive CPU or RAM**. Better
  than PG in all measurable ways.
- **Feature-complete** — support every PostgreSQL feature, including **Plan
  Advice**, perfectly. Falling back to PG means Ra has *not* met the bar for
  that query shape; track every fallback as a gap to close
  (`docs/planner-fallback-backlog.md`).

## Separation of concerns (must be preserved)

Three distinct layers; do not blur them:

1. **Rules** (`.rra` files, e-graph rewrites) — *logical* equivalences:
   predicate pushdown, join commutativity/associativity, projection pruning,
   expression simplification. Never physical/cost decisions.
2. **Cost model + dataflow + neural (BitNet)** — *plan selection* among
   equivalent plans: join order, **physical operator choice (hash vs
   nested-loop vs merge join, scan method)**, driven by hardware calibration
   and the live monitoring fingerprint.
3. **`plan_builder` (lowering)** — faithful translation of the *chosen*
   `RelExpr` into PostgreSQL `Plan` nodes. No optimization decisions.

### Resolved separation-of-concerns debt

- **Join method (HashJoin vs NestLoop)** is now chosen in layer 2: a
  cardinality-based decision in `ra-engine`
  (`plan_advice_physical::decide_join_strategy`, populated via
  `PhysicalChoices::augment_join_strategies_from_stats`) selects hash vs
  nested-loop per join from row counts and carries the choice on
  `PhysicalChoices` (keyed by the inner relation's alias). `plan_builder`
  renders that choice, applying only catalog *feasibility* (is the condition a
  hashable `=`? does the join type require hash?) — which is correctly a
  lowering concern. Supplied advice still wins over the cost-based default.
  **Future work:** merge-join selection when both inputs are pre-sorted, and a
  full hash-vs-nestloop cost crossover that accounts for inner indexes (the
  current rule prefers hash for any non-degenerate equi-join, matching the
  plain — no inner-index — nested loops the builder emits today).
