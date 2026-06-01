# Join-order search: insights from the PG starjoin thread

Source: pgsql-hackers "should we have a fast-path planning for OLTP
starjoins?" (Vondra, Feb 2025 → ongoing). Robert Haas's framing: the real
waste in join search is *commuting joins that do not change the row count*.

## What this means for Ra

Ra's architecture already addresses parts of this, and the thread suggests
two concrete improvements plus one announcement talking-point.

1. **Equality saturation already dedups equivalent join orders.** All join
   orders reachable by commutativity/associativity rewrites collapse into
   one e-class, so Ra does not enumerate N! plans — it saturates once and
   extracts. This is a genuine strength to highlight, *with the honest
   caveat* that saturation itself blows up on large dense joins (measured:
   5–7-table joins 45–73 ms — Ra's #1 planning-time weakness).

2. **1:1 "neutral" join pruning (action item, combats the blowup).** A join
   is provably 1:1 — neither adds nor removes rows — when it matches a PK /
   unique key on one side with a NOT-NULL referencing column on the other
   (inner), or unique + LEFT JOIN. This is a *structural* property, knowable
   without cardinality estimates. Ra could detect such edges and prune
   reordering among them (they commute freely), which directly shrinks the
   e-graph / left-deep search for star and snowflake shapes. This is the
   "neutral join" idea from the thread, applied to Ra's search rather than
   PG's DP.

3. **Generalize "star" to 1:1 pendant peeling.** Rather than detecting a
   fact + dimensions, peel degree-1 vertices connected by a 1:1 edge from
   the join graph, solve the reduced core, then re-insert pendants by cost.
   Handles snowflakes and non-star 1:1 chains uniformly. Candidate for
   `left_deep.rs` (the heuristic join-order path) as a pre-reduction.

4. **Cardinality estimation before search.** Tom Lane / Robert Haas both
   circle toward decoupling cardinality estimation from path construction so
   reduce/neutral/expand joins can be classified up front. Ra already keeps
   statistics/cost in a layer separate from plan construction, so it is a
   natural testbed; worth keeping that separation clean.

## Status

These are research/optimization items (the e-graph already reorders joins
correctly; #2/#3 are about *search cost*, not correctness). None outranks
the index-scan access-path gap (`docs/planner-fallback-backlog.md`), which
is the item that makes Ra measurably *worse* than PG on common queries.
Logged here so the join-order work is not lost.
