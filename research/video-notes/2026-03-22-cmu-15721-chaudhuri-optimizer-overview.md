# CMU 15-721 Lecture 13: Optimizer Implementation I - Chaudhuri Overview

**Source:** CMU 15-721 Spring 2024, Lecture 13
**Date:** 2024-03-18
**Topic:** Comprehensive overview of query optimization in relational systems
**Key Paper:** "An Overview of Query Optimization in Relational Systems" (Chaudhuri, PODS 1998)

## Key Points

This lecture provides the foundational framework for understanding query optimizer
architecture based on Chaudhuri's seminal survey paper. It identifies the complete
taxonomy of optimization techniques that a production optimizer should implement.

### Search Space Enumeration

**System R approach (bottom-up dynamic programming):**
1. Enumerate all single-table access paths
2. For each pair of tables, consider all join methods
3. Build up optimal plans for increasingly larger subsets
4. Use "interesting orders" to keep suboptimal plans that provide useful ordering
5. Complexity: O(3^n) for n tables, prohibitive for n > 15

**Cascades/Volcano approach (top-down with memoization):**
1. Start from the logical expression for the full query
2. Apply transformation rules to generate equivalent expressions
3. Apply implementation rules to map logical to physical operators
4. Memoize results in a hash table (memo table)
5. Branch-and-bound pruning with cost upper bounds
6. Complexity: similar worst case but better in practice due to pruning

**Equality saturation (egg/Ra approach):**
1. Represent all equivalent expressions in an e-graph
2. Apply rewrite rules until saturation (no new equivalences)
3. Extract optimal plan from e-graph using cost function
4. Key advantage: rules are order-independent
5. Key challenge: extraction must consider physical properties

### Transformation Rules Taxonomy

Chaudhuri categorizes all transformation rules into:

**1. Predicate-related:**
- Predicate pushdown (through joins, aggregates, set operations)
- Predicate pullup (for subquery decorrelation)
- Predicate simplification (constant folding, contradiction detection)
- Transitive closure (A=B AND B=C implies A=C)
- Predicate inference (A=B AND A>5 implies B>5)

**2. Join-related:**
- Commutativity: R join S = S join R
- Associativity: (R join S) join T = R join (S join T)
- Left/right join exchange: in some cases
- Outer join simplification (outer to inner)
- Semi-join reduction
- Join elimination (using foreign keys, unique constraints)

**3. Group-by/Aggregate-related:**
- Eager aggregation (aggregate before join when possible)
- Lazy aggregation (aggregate after join when cheaper)
- Double aggregation elimination
- Group-by pushdown through join
- Group-by pullup through join

**4. Subquery-related:**
- Decorrelation of correlated subqueries
- EXISTS to semi-join
- NOT EXISTS to anti-join
- Scalar subquery to left join + aggregate
- IN subquery to semi-join/join

**5. Set-operation-related:**
- UNION to UNION ALL + DISTINCT (when appropriate)
- INTERSECT to semi-join
- EXCEPT to anti-join
- Set operation pushdown through filter

### Cost Estimation Framework

Three components of cost:

1. **I/O cost**: Pages read from disk (sequential vs random)
   - Sequential: cost per page * number of pages
   - Random: cost per page * number of random accesses
   - Cache effects: reduce effective cost based on buffer pool hit ratio

2. **CPU cost**: Tuples processed
   - Comparison operations per tuple
   - Hash computation per tuple
   - Expression evaluation per tuple

3. **Memory cost**: Working memory required
   - Hash table size for hash joins/aggregates
   - Sort buffer size
   - Network buffers for distributed queries

**Startup vs total cost:**
- Startup cost: time before first output tuple
- Total cost: time for all output tuples
- Important for LIMIT queries and nested loop join inner sides
- Sort has high startup cost (must sort before producing output)
- Hash join build has high startup cost (must build hash table first)

### Access Path Selection

The optimizer must choose between:
1. **Sequential scan**: Read all pages, filter in memory
2. **Index scan**: Use index to find qualifying rows, fetch pages
3. **Index-only scan**: Answer query entirely from index (covering index)
4. **Bitmap scan**: Use index to build bitmap, then fetch pages in order

**Decision factors:**
- Selectivity of predicates
- Correlation between index order and physical data order
- Number of columns needed (covering index eligibility)
- Table size relative to buffer pool

**Key formulas:**
- Mackert-Lohman formula for index scan I/O cost considering correlation
- Effective pages fetched = min(pages, rows) for highly correlated indexes
- Break-even point: index scan cheaper when selectivity < ~15-30%

## Optimization Rules for Ra

### Rules Confirmed Present in Ra
- Predicate pushdown (comprehensive)
- Join commutativity and associativity
- Subquery decorrelation (extensive set)
- Set operation transformations
- Constant folding and simplification
- Semi-join / anti-join transformations

### Rules Identified as Missing or Incomplete

1. **transitive-predicate-closure** - Derive A=C from A=B AND B=C, then push A=C
   to table containing C. Checked: Ra has predicate inference in `rules/logical/
   predicate-pushdown/` but verify transitive closure specifically.

2. **aggregate-pullup-through-join** - When aggregation after join can be pulled up
   to operate on a pre-aggregated input. Complement to eager aggregation.

3. **mackert-lohman-index-cost** - Use column correlation to estimate random vs
   sequential I/O for index scans. Currently missing from cost model.

4. **startup-cost-optimization** - For LIMIT queries, prefer plans with low startup
   cost (index scan producing sorted output) over plans with low total cost (hash
   join + sort).

5. **buffer-pool-aware-costing** - Adjust I/O cost based on expected buffer pool
   hit ratio. Frequently accessed pages are likely cached.

6. **interesting-order-tracking** - Track sort orders through plan nodes. Keep
   "suboptimal" plans that provide useful ordering for downstream operators.

7. **branch-and-bound-pruning** - During plan enumeration, prune plans whose
   partial cost exceeds the best complete plan found so far.

### Ra Gap Analysis

Ra's e-graph approach handles many of these naturally (equivalence classes
represent interesting orders implicitly). However:

**Missing from cost model:**
- Mackert-Lohman correlation-based index I/O cost
- Buffer pool hit ratio modeling
- Startup cost preference for LIMIT queries

**Missing from rules:**
- Transitive predicate closure (verify -- may be partial)
- Aggregate pullup through join
- Formal interesting order tracking in e-graph extraction

## Relevance to Ra

**Priority:** Critical - this is the foundational framework. Most individual rules
exist in Ra, but the cost model gaps (correlation-aware I/O, startup cost, buffer
pool awareness) affect plan quality for the entire system.

**Proposed RFCs:**
1. Correlation-aware index scan costing (extend existing cost model)
2. Startup cost preference for LIMIT/FETCH FIRST queries
3. Transitive predicate inference (if not already complete)
